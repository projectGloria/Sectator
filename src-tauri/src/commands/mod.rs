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
