use crate::minecraft::java::JavaInstallation;
use crate::progress::{Progress, ProgressSink, Stage};
use crate::{paths, Error, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

const PROMOTIONS_URL: &str = "https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json";

#[derive(Debug, Deserialize)]
struct Promotions {
    promos: HashMap<String, String>,
}

/// Consulta la build "recommended" (o "latest" si no hay recommended) de
/// Forge para una versión de Minecraft dada. Ej: "1.20.1" -> "47.4.0".
pub async fn resolve_forge_version(client: &reqwest::Client, mc_version: &str) -> Result<String> {
    let promotions: Promotions = client
        .get(PROMOTIONS_URL)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let recommended = promotions.promos.get(&format!("{mc_version}-recommended"));
    let latest = promotions.promos.get(&format!("{mc_version}-latest"));

    recommended
        .or(latest)
        .cloned()
        .ok_or_else(|| Error::ForgeVersionNotFound(mc_version.to_string()))
}

fn installer_url(mc_version: &str, forge_version: &str) -> String {
    let full = format!("{mc_version}-{forge_version}");
    format!("https://maven.minecraftforge.net/net/minecraftforge/forge/{full}/forge-{full}-installer.jar")
}

/// Se asegura de que exista `versions/<mc>-forge-<forge>/...json`. Si ya está
/// instalado, no hace nada. Si no, baja el instalador oficial de Forge y lo
/// corre en modo headless (`--installClient`) contra nuestra carpeta de
/// instancia, dejando que el instalador resuelva librerías/patches como lo
/// haría el launcher oficial.
pub async fn ensure_forge_installed(
    client: &reqwest::Client,
    java: &JavaInstallation,
    mc_version: &str,
    forge_version: &str,
    progress: &ProgressSink,
) -> Result<String> {
    let expected_id = format!("{mc_version}-forge-{forge_version}");
    if paths::version_json_path(&expected_id).exists() {
        return Ok(expected_id);
    }

    progress(Progress::new(
        Stage::InstallingForge,
        format!("Instalando Forge {forge_version}..."),
        None,
    ));

    let versions_before: std::collections::HashSet<String> = list_version_dirs();

    let installer_jar = paths::cache_dir().join(format!("forge-{mc_version}-{forge_version}-installer.jar"));
    let url = installer_url(mc_version, forge_version);
    crate::minecraft::download::ensure_file(client, &url, &installer_jar, None, None).await?;

    std::fs::create_dir_all(paths::instance_dir())?;
    ensure_launcher_profile(&paths::instance_dir())?;

    let output = tokio::process::Command::new(&java.executable)
        .arg("-jar")
        .arg(&installer_jar)
        .arg("--installClient")
        .arg(paths::instance_dir())
        .current_dir(paths::cache_dir())
        .output()
        .await?;

    if !output.status.success() {
        let mut details = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.trim().is_empty() {
            if !details.is_empty() {
                details.push('\n');
            }
            details.push_str(stderr.trim());
        }
        return Err(Error::ForgeInstallFailed {
            code: output.status.code(),
            details,
        });
    }

    if paths::version_json_path(&expected_id).exists() {
        return Ok(expected_id);
    }

    // El id no vino con el formato esperado: buscamos qué carpeta nueva
    // apareció en versions/ y la usamos como fallback.
    let versions_after = list_version_dirs();
    let new_id = versions_after
        .difference(&versions_before)
        .next()
        .cloned()
        .ok_or_else(|| Error::ForgeVersionJsonMissing(paths::version_json_path(&expected_id)))?;

    Ok(new_id)
}

/// El instalador de Forge se niega a correr en modo headless si el directorio
/// destino no "parece" una carpeta de Minecraft (revisa que exista este
/// archivo). Un launcher_profiles.json vacío pero con el shape correcto
/// alcanza para que lo acepte.
fn ensure_launcher_profile(instance_dir: &std::path::Path) -> Result<()> {
    let path = instance_dir.join("launcher_profiles.json");
    if path.exists() {
        return Ok(());
    }
    let contents = serde_json::json!({
        "profiles": {},
        "settings": {},
        "version": 3
    });
    std::fs::write(path, serde_json::to_vec_pretty(&contents)?)?;
    Ok(())
}

fn list_version_dirs() -> std::collections::HashSet<String> {
    let dir: PathBuf = paths::versions_dir();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Default::default();
    };
    entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect()
}
