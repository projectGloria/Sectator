//! Tauri IPC commands. Thin wrappers: all logic lives in engine/, storage/, scraper/.

use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub name: &'static str,
    pub version: &'static str,
}

#[tauri::command]
pub fn app_info() -> AppInfo {
    AppInfo { name: "Sectator", version: env!("CARGO_PKG_VERSION") }
}

use std::sync::Arc;

use tauri::ipc::Channel;
use tauri::State;

use crate::engine::runner::{Engine, SearchEvent};
use crate::indexers::{registry, IndexerConfig};

/// Run a search, streaming `SearchEvent`s over `on_event`. A newer call cancels
/// this one; the returned id matches the events' `searchId`.
#[tauri::command]
pub async fn search(engine: State<'_, Arc<Engine>>, query: String, on_event: Channel<SearchEvent>) -> Result<u64, String> {
    let engine = engine.inner().clone();
    let sink = Arc::new(move |e: SearchEvent| {
        let _ = on_event.send(e);
    });
    Ok(engine.search(&query, sink).await)
}

#[tauri::command]
pub fn cancel_search(engine: State<'_, Arc<Engine>>) {
    engine.cancel();
}

/// Build the live indexer set from configs, skipping disabled or invalid entries.
pub fn build_indexers(configs: &[IndexerConfig]) -> Vec<Arc<dyn crate::indexers::Indexer>> {
    configs.iter().filter(|c| c.enabled).filter_map(|c| registry::build(c).ok()).collect()
}

/// Hand a magnet link to the system's default torrent client. Only `magnet:` URIs
/// are accepted so the webview can't use this to launch arbitrary URLs.
#[tauri::command]
pub fn open_magnet(app: tauri::AppHandle, magnet: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    if !magnet.starts_with("magnet:?") || magnet.chars().any(char::is_control) {
        return Err("not a magnet link".into());
    }
    app.opener().open_url(magnet, None::<&str>).map_err(|e| e.to_string())
}
