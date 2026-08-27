pub mod error;
pub mod forge;
pub mod minecraft;
pub mod mods;
pub mod paths;
pub mod profile;
pub mod progress;
pub mod server_status;

pub use error::{Error, Result};

use profile::Settings;
use progress::{Progress, ProgressSink, Stage};

/// Por ahora el launcher apunta a una sola versión/modpack fijo. El día que
/// haga falta más de uno, esto se vuelve un parámetro.
pub const MC_VERSION: &str = "1.20.1";
const REQUIRED_JAVA_MAJOR: u32 = 17;

pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(concat!("SegestriaLauncher/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("no se pudo armar el cliente http")
}

/// Pipeline completo de "jugar": Java -> vanilla -> Forge -> mods -> lanzar.
/// Cada paso reporta su avance a través de `progress` para que la UI muestre
/// algo con sentido en vez de una barra congelada.
pub async fn play(settings: &Settings, mods_manifest: &mods::ModsManifest, progress: ProgressSink) -> Result<()> {
    paths::ensure_dirs()?;
    let client = http_client();

    progress(Progress::new(Stage::CheckingJava, "Buscando Java...", None));
    let java = minecraft::java::ensure_java(REQUIRED_JAVA_MAJOR).await?;

    minecraft::version_manifest::fetch_and_cache_vanilla_version(&client, MC_VERSION).await?;

    let forge_version = forge::resolve_forge_version(&client, MC_VERSION).await?;
    let forge_id = forge::ensure_forge_installed(&client, &java, MC_VERSION, &forge_version, &progress).await?;

    let resolved = minecraft::version_manifest::resolve_version(&forge_id)?;

    let client_jar = minecraft::download::download_client_jar(&client, &resolved, &progress).await?;
    let natives_dir = paths::natives_dir(&resolved.id);
    let library_classpath =
        minecraft::download::download_libraries(&client, &resolved, &natives_dir, &progress).await?;
    minecraft::download::download_assets(&client, &resolved, &progress).await?;

    mods::sync_mods(&client, mods_manifest, &progress).await?;

    progress(Progress::new(Stage::Launching, "Iniciando Minecraft...", None));

    let opts = minecraft::launch::LaunchOptions {
        natives_dir,
        library_classpath,
        client_jar,
    };
    let cmd = minecraft::launch::build_command(&resolved, &java.executable, &opts, settings)?;
    minecraft::launch::spawn_detached(cmd)?;

    Ok(())
}
