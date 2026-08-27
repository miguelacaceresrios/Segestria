use super::version_manifest::{DownloadInfo, Library, ResolvedVersion};
use crate::progress::{Progress, ProgressSink, Stage};
use crate::{paths, Error, Result};
use futures_util::stream::{self, StreamExt};
use sha1::{Digest, Sha1};
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

const CONCURRENT_DOWNLOADS: usize = 24;
const ASSET_BASE_URL: &str = "https://resources.download.minecraft.net";

fn sha1_hex(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha1::new();
    let mut buf = [0u8; 1 << 16];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn file_matches(path: &Path, expected_sha1: Option<&str>, expected_size: Option<u64>) -> bool {
    if !path.exists() {
        return false;
    }
    if let Some(size) = expected_size {
        match std::fs::metadata(path) {
            Ok(meta) if meta.len() == size => {}
            _ => return false,
        }
    }
    if let Some(expected) = expected_sha1 {
        match sha1_hex(path) {
            Ok(actual) => actual.eq_ignore_ascii_case(expected),
            Err(_) => false,
        }
    } else {
        true
    }
}

/// Descarga `url` a `dest` si hace falta (no existe o el checksum no matchea).
/// Escribe primero a un archivo temporal en la misma carpeta y lo renombra al
/// final, para no dejar archivos a medio bajar si el proceso se corta.
pub async fn ensure_file(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    expected_sha1: Option<&str>,
    expected_size: Option<u64>,
) -> Result<()> {
    if file_matches(dest, expected_sha1, expected_size) {
        return Ok(());
    }

    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let response = client.get(url).send().await?.error_for_status()?;
    let bytes = response.bytes().await?;

    let tmp_path = dest.with_extension(format!(
        "{}.part",
        dest.extension().and_then(|e| e.to_str()).unwrap_or("tmp")
    ));
    tokio::fs::write(&tmp_path, &bytes).await?;

    if let Some(expected) = expected_sha1 {
        let actual = sha1_hex(&tmp_path)?;
        if !actual.eq_ignore_ascii_case(expected) {
            let _ = tokio::fs::remove_file(&tmp_path).await;
            return Err(Error::ChecksumMismatch {
                path: dest.to_path_buf(),
                expected: expected.to_string(),
                actual,
            });
        }
    }

    tokio::fs::rename(&tmp_path, dest).await?;
    Ok(())
}

pub async fn download_client_jar(
    client: &reqwest::Client,
    version: &ResolvedVersion,
    progress: &ProgressSink,
) -> Result<PathBuf> {
    let dest = paths::version_jar_path(&version.id);
    let download = version
        .client_download
        .as_ref()
        .ok_or_else(|| Error::Other(format!("{}: no tiene downloads.client (¿instalaste Forge?)", version.id)))?;

    progress(Progress::new(
        Stage::DownloadingClient,
        "Descargando Minecraft...",
        None,
    ));

    ensure_file(client, &download.url, &dest, Some(&download.sha1), Some(download.size)).await?;
    Ok(dest)
}

fn library_artifact_download(lib: &Library) -> Option<(String, DownloadInfo)> {
    if let Some(downloads) = &lib.downloads {
        if let Some(artifact) = &downloads.artifact {
            let path = artifact.path.clone().unwrap_or_else(|| maven_path(&lib.name, None));
            return Some((path, artifact.clone()));
        }
    }

    // Formato viejo: sólo hay "name" + "url" (base del repo Maven), hay que
    // armar la ruta y no tenemos sha1/size para verificar.
    let base_url = lib.url.as_ref()?;
    let path = maven_path(&lib.name, None);
    let url = format!("{}/{path}", base_url.trim_end_matches('/'));
    Some((
        path,
        DownloadInfo {
            url,
            sha1: String::new(),
            size: 0,
            path: None,
        },
    ))
}

fn maven_path(coordinate: &str, extra_classifier: Option<&str>) -> String {
    let parts: Vec<&str> = coordinate.split(':').collect();
    let (group, artifact, version) = (parts[0], parts[1], parts[2]);
    let classifier = extra_classifier.or_else(|| parts.get(3).copied());
    let group_path = group.replace('.', "/");
    match classifier {
        Some(c) => format!("{group_path}/{artifact}/{version}/{artifact}-{version}-{c}.jar"),
        None => format!("{group_path}/{artifact}/{version}/{artifact}-{version}.jar"),
    }
}

/// Descarga las librerías aplicables al SO actual y extrae las nativas
/// (LWJGL, etc.) a `natives_dir`. Devuelve la lista de jars para el classpath.
pub async fn download_libraries(
    client: &reqwest::Client,
    version: &ResolvedVersion,
    natives_dir: &Path,
    progress: &ProgressSink,
) -> Result<Vec<PathBuf>> {
    progress(Progress::new(
        Stage::DownloadingLibraries,
        "Descargando librerías...",
        None,
    ));

    let features = HashMap::new();
    let applicable: Vec<&Library> = version
        .libraries
        .iter()
        .filter(|lib| lib.is_allowed(&features))
        .collect();

    let mut classpath = Vec::new();
    let libraries_dir = paths::libraries_dir();

    let downloads: Vec<_> = applicable
        .iter()
        .filter_map(|lib| library_artifact_download(lib).map(|(path, info)| (libraries_dir.join(&path), info)))
        .collect();

    for (dest, _) in &downloads {
        classpath.push(dest.clone());
    }

    let results: Vec<Result<()>> = stream::iter(downloads.into_iter().map(|(dest, info)| {
        let client = client.clone();
        async move {
            let sha1 = if info.sha1.is_empty() { None } else { Some(info.sha1.as_str()) };
            let size = if info.size == 0 { None } else { Some(info.size) };
            ensure_file(&client, &info.url, &dest, sha1, size).await
        }
    }))
    .buffer_unordered(CONCURRENT_DOWNLOADS)
    .collect()
    .await;

    for r in results {
        r?;
    }

    // Nativas: jars con clasificador por SO (p.ej. lwjgl natives-windows) que
    // hay que descomprimir directo en la carpeta de nativas de la versión.
    tokio::fs::create_dir_all(natives_dir).await?;
    for lib in &applicable {
        let Some(classifier) = lib.native_classifier() else { continue };
        let Some(downloads) = &lib.downloads else { continue };
        let Some(classifiers) = &downloads.classifiers else { continue };
        let Some(info) = classifiers.get(&classifier) else { continue };

        let cache_path = libraries_dir.join(maven_path(&lib.name, Some(classifier.as_str())));
        ensure_file(client, &info.url, &cache_path, Some(&info.sha1), Some(info.size)).await?;
        extract_natives(&cache_path, natives_dir, lib.extract.as_ref().map(|e| e.exclude.as_slice()).unwrap_or(&[]))?;
    }

    Ok(classpath)
}

fn extract_natives(jar_path: &Path, dest_dir: &Path, exclude: &[String]) -> Result<()> {
    let file = std::fs::File::open(jar_path)?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| Error::Zip {
        path: jar_path.to_path_buf(),
        source: e,
    })?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| Error::Zip {
            path: jar_path.to_path_buf(),
            source: e,
        })?;
        let name = entry.name().to_string();

        if name.ends_with('/') {
            continue;
        }
        if exclude.iter().any(|prefix| name.starts_with(prefix.as_str())) {
            continue;
        }
        // Las nativas son .dll/.so/.dylib sueltas; nos salteamos metadata del jar.
        if name.starts_with("META-INF/") {
            continue;
        }

        let out_path = dest_dir.join(Path::new(&name).file_name().unwrap_or_default());
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out_file = std::fs::File::create(&out_path)?;
        std::io::copy(&mut entry, &mut out_file)?;
    }

    Ok(())
}

#[derive(Debug, serde::Deserialize)]
struct AssetIndex {
    objects: HashMap<String, AssetObject>,
}

#[derive(Debug, serde::Deserialize)]
struct AssetObject {
    hash: String,
    size: u64,
}

pub async fn download_assets(client: &reqwest::Client, version: &ResolvedVersion, progress: &ProgressSink) -> Result<()> {
    progress(Progress::new(
        Stage::DownloadingAssets,
        "Descargando assets (texturas, sonidos)...",
        None,
    ));

    let assets_dir = paths::assets_dir();
    let index_path = assets_dir.join("indexes").join(format!("{}.json", version.asset_index.id));

    ensure_file(
        client,
        &version.asset_index.url,
        &index_path,
        Some(&version.asset_index.sha1),
        Some(version.asset_index.size),
    )
    .await?;

    let raw = tokio::fs::read_to_string(&index_path).await?;
    let index: AssetIndex = serde_json::from_str(&raw)?;

    let objects_dir = assets_dir.join("objects");
    let total = index.objects.len();
    let done = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));

    let tasks = index.objects.into_values().map(|obj| {
        let client = client.clone();
        let objects_dir = objects_dir.clone();
        let progress = progress.clone();
        let done = done.clone();
        async move {
            let prefix = &obj.hash[0..2];
            let dest = objects_dir.join(prefix).join(&obj.hash);
            let url = format!("{ASSET_BASE_URL}/{prefix}/{}", obj.hash);
            let result = ensure_file(&client, &url, &dest, Some(&obj.hash), Some(obj.size)).await;

            let finished = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            if finished.is_multiple_of(25) || finished == total {
                progress(Progress::new(
                    Stage::DownloadingAssets,
                    format!("Assets: {finished}/{total}"),
                    Some(finished as f32 / total.max(1) as f32),
                ));
            }
            result
        }
    });

    let results: Vec<Result<()>> = stream::iter(tasks).buffer_unordered(CONCURRENT_DOWNLOADS).collect().await;
    for r in results {
        r?;
    }

    Ok(())
}
