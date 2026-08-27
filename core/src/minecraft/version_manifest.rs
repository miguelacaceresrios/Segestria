use super::rules::{rules_allow, Rule};
use crate::{paths, Error, Result};
use serde::Deserialize;
use std::collections::HashMap;

const VERSION_MANIFEST_URL: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

#[derive(Debug, Clone, Deserialize)]
pub struct VersionManifest {
    pub versions: Vec<VersionManifestEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VersionManifestEntry {
    pub id: String,
    pub url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DownloadInfo {
    pub url: String,
    pub sha1: String,
    pub size: u64,
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Downloads {
    pub client: Option<DownloadInfo>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AssetIndexRef {
    pub id: String,
    pub url: String,
    pub sha1: String,
    pub size: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct JavaVersionInfo {
    #[serde(rename = "majorVersion")]
    pub major_version: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LibraryDownloads {
    pub artifact: Option<DownloadInfo>,
    pub classifiers: Option<HashMap<String, DownloadInfo>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExtractRules {
    #[serde(default)]
    pub exclude: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Library {
    pub name: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub downloads: Option<LibraryDownloads>,
    #[serde(default)]
    pub natives: Option<HashMap<String, String>>,
    #[serde(default)]
    pub rules: Option<Vec<Rule>>,
    #[serde(default)]
    pub extract: Option<ExtractRules>,
}

impl Library {
    /// "grupo:artefacto[:clasificador]" sin versión, para poder deduplicar
    /// cuando Forge pisa una librería vainilla con otra versión (típico:
    /// asm, guava). Ojo: el clasificador se preserva a propósito, porque
    /// vainilla 1.19+ lista cada variante nativa (natives-windows,
    /// natives-windows-x86, natives-linux, etc.) como una librería separada
    /// con el mismo grupo:artefacto — sin esto, todas esas variantes se
    /// pisan entre sí y sobrevive una sola al azar.
    pub fn coordinate_key(&self) -> String {
        let parts: Vec<&str> = self.name.split(':').collect();
        match parts.as_slice() {
            [group, artifact, ..] if parts.len() >= 4 => {
                format!("{group}:{artifact}:{}", parts[3])
            }
            [group, artifact, ..] => format!("{group}:{artifact}"),
            _ => self.name.clone(),
        }
    }

    pub fn is_allowed(&self, features: &HashMap<String, bool>) -> bool {
        if !self.matches_current_arch() {
            return false;
        }
        match &self.rules {
            Some(rules) => rules_allow(rules, features),
            None => true,
        }
    }

    /// Vainilla 1.19+ no distingue arquitectura vía "rules" para las
    /// variantes windows-x86/windows-arm64/macos-arm64 (las tres declaran la
    /// misma regla "os.name == windows/osx" sin más), así que el launcher
    /// oficial las filtra por sufijo de nombre en vez de por regla. Hacemos
    /// lo mismo acá.
    fn matches_current_arch(&self) -> bool {
        let arch = super::rules::current_arch();
        if self.name.ends_with("-arm64") {
            return arch == "arm64";
        }
        if self.name.ends_with("-x86") {
            return arch == "x86";
        }
        true
    }

    pub fn native_classifier(&self) -> Option<String> {
        let natives = self.natives.as_ref()?;
        let key = super::rules::current_os_name();
        let arch_placeholder = if cfg!(target_pointer_width = "64") { "64" } else { "32" };
        natives
            .get(key)
            .map(|v| v.replace("${arch}", arch_placeholder))
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum ArgValue {
    Single(String),
    Multiple(Vec<String>),
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConditionalArg {
    #[serde(default)]
    pub rules: Vec<Rule>,
    pub value: ArgValue,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum ArgEntry {
    Plain(String),
    Conditional(ConditionalArg),
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Arguments {
    #[serde(default)]
    pub game: Vec<ArgEntry>,
    #[serde(default)]
    pub jvm: Vec<ArgEntry>,
}

pub fn resolve_args(entries: &[ArgEntry], features: &HashMap<String, bool>) -> Vec<String> {
    let mut out = Vec::new();
    for entry in entries {
        match entry {
            ArgEntry::Plain(s) => out.push(s.clone()),
            ArgEntry::Conditional(c) => {
                if rules_allow(&c.rules, features) {
                    match &c.value {
                        ArgValue::Single(s) => out.push(s.clone()),
                        ArgValue::Multiple(v) => out.extend(v.iter().cloned()),
                    }
                }
            }
        }
    }
    out
}

/// Version.json tal como lo escupe Mojang (o el instalador de Forge) a disco.
#[derive(Debug, Clone, Deserialize)]
pub struct RawVersionInfo {
    pub id: String,
    #[serde(rename = "mainClass")]
    pub main_class: Option<String>,
    #[serde(default)]
    pub arguments: Option<Arguments>,
    #[serde(default)]
    pub libraries: Vec<Library>,
    #[serde(default)]
    pub downloads: Option<Downloads>,
    #[serde(rename = "assetIndex")]
    #[serde(default)]
    pub asset_index: Option<AssetIndexRef>,
    #[serde(default)]
    pub assets: Option<String>,
    #[serde(rename = "javaVersion")]
    #[serde(default)]
    pub java_version: Option<JavaVersionInfo>,
    #[serde(rename = "inheritsFrom")]
    #[serde(default)]
    pub inherits_from: Option<String>,
}

/// Version resuelta: mismo shape que RawVersionInfo pero ya con la cadena de
/// `inheritsFrom` aplanada (relevante para Forge, que hereda de vanilla).
#[derive(Debug, Clone)]
pub struct ResolvedVersion {
    pub id: String,
    pub main_class: String,
    pub arguments: Arguments,
    pub libraries: Vec<Library>,
    pub client_download: Option<DownloadInfo>,
    pub asset_index: AssetIndexRef,
    pub assets: String,
    pub java_major_version: u32,
}

pub async fn fetch_version_manifest(client: &reqwest::Client) -> Result<VersionManifest> {
    let manifest = client
        .get(VERSION_MANIFEST_URL)
        .send()
        .await?
        .error_for_status()?
        .json::<VersionManifest>()
        .await?;
    Ok(manifest)
}

/// Descarga (si hace falta) y cachea a disco el version.json de una versión
/// vanilla oficial. Devuelve el id (por conveniencia al encadenar llamadas).
pub async fn fetch_and_cache_vanilla_version(
    client: &reqwest::Client,
    version_id: &str,
) -> Result<String> {
    let dest = paths::version_json_path(version_id);
    if dest.exists() {
        return Ok(version_id.to_string());
    }

    let manifest = fetch_version_manifest(client).await?;
    let entry = manifest
        .versions
        .iter()
        .find(|v| v.id == version_id)
        .ok_or_else(|| Error::VersionNotFound(version_id.to_string()))?;

    let body = client.get(&entry.url).send().await?.error_for_status()?.bytes().await?;

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&dest, &body)?;

    Ok(version_id.to_string())
}

fn read_raw_version(version_id: &str) -> Result<RawVersionInfo> {
    let path = paths::version_json_path(version_id);
    let raw = std::fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&raw)?)
}

/// Lee de disco el version.json de `version_id` y, si hereda de otra versión,
/// la resuelve recursivamente y mergea (mismo criterio que usan los launchers
/// de terceros: el hijo pisa/agrega sobre el padre).
pub fn resolve_version(version_id: &str) -> Result<ResolvedVersion> {
    let raw = read_raw_version(version_id)?;

    let parent = match &raw.inherits_from {
        Some(parent_id) => Some(resolve_version(parent_id)?),
        None => None,
    };

    let main_class = raw
        .main_class
        .or_else(|| parent.as_ref().map(|p| p.main_class.clone()))
        .ok_or_else(|| Error::Other(format!("{version_id}: falta mainClass")))?;

    let mut arguments = parent.as_ref().map(|p| p.arguments.clone()).unwrap_or_default();
    if let Some(child_args) = &raw.arguments {
        arguments.game.extend(child_args.game.iter().cloned());
        arguments.jvm.extend(child_args.jvm.iter().cloned());
    }

    let mut libraries: Vec<Library> = parent.as_ref().map(|p| p.libraries.clone()).unwrap_or_default();
    for lib in raw.libraries {
        let key = lib.coordinate_key();
        libraries.retain(|existing| existing.coordinate_key() != key);
        libraries.push(lib);
    }

    let client_download = raw
        .downloads
        .and_then(|d| d.client)
        .or_else(|| parent.as_ref().and_then(|p| p.client_download.clone()));

    let asset_index = raw
        .asset_index
        .or_else(|| parent.as_ref().map(|p| p.asset_index.clone()))
        .ok_or_else(|| Error::Other(format!("{version_id}: falta assetIndex")))?;

    let assets = raw
        .assets
        .or_else(|| parent.as_ref().map(|p| p.assets.clone()))
        .unwrap_or_else(|| asset_index.id.clone());

    let java_major_version = raw
        .java_version
        .map(|j| j.major_version)
        .or_else(|| parent.as_ref().map(|p| p.java_major_version))
        .unwrap_or(17);

    Ok(ResolvedVersion {
        id: version_id.to_string(),
        main_class,
        arguments,
        libraries,
        client_download,
        asset_index,
        assets,
        java_major_version,
    })
}
