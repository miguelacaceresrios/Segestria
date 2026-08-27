use segestria_core::mods::ModsManifest;
use segestria_core::profile::Settings;
use segestria_core::progress::ProgressSink;
use segestria_core::server_status::{self, ServerStatus};
use serde::Deserialize;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};

/// Lista de mods embebida en el binario. Hoy arranca vacía (ver README);
/// el día que haya mods reales, esto se reemplaza por una descarga desde la
/// nube en vez de un archivo empaquetado en el instalador.
const DEFAULT_MANIFEST: &str = include_str!("../../../manifest.json");
const NEWS: &str = include_str!("../../../news.json");

#[derive(Debug, Deserialize, serde::Serialize)]
struct NewsItem {
    date: String,
    title: String,
    body: String,
}

#[derive(Debug, Deserialize, serde::Serialize)]
struct NewsFeed {
    items: Vec<NewsItem>,
}

#[tauri::command]
fn get_settings() -> Settings {
    Settings::load()
}

#[tauri::command]
fn save_settings(settings: Settings) -> Result<(), String> {
    settings.save().map_err(|e| e.to_string())
}

#[tauri::command]
fn get_news() -> Result<NewsFeed, String> {
    serde_json::from_str(NEWS).map_err(|e| e.to_string())
}

#[tauri::command]
async fn ping_server(ip: String, port: u16) -> ServerStatus {
    server_status::ping(&ip, port).await
}

#[tauri::command]
async fn play(app: AppHandle, settings: Settings) -> Result<(), String> {
    settings.save().map_err(|e| e.to_string())?;

    let manifest = ModsManifest::from_json(DEFAULT_MANIFEST).map_err(|e| e.to_string())?;

    let sink: ProgressSink = {
        let app = app.clone();
        Arc::new(move |progress| {
            let _ = app.emit("progress", progress);
        })
    };

    segestria_core::play(&settings, &manifest, sink)
        .await
        .map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            get_news,
            ping_server,
            play
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
