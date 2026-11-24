//! Sectator: local-first torrent meta-search engine.

pub mod commands;
pub mod engine;
pub mod indexers;
pub mod models;
pub mod parser;
pub mod scraper;
pub mod storage;

use std::sync::Arc;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            let store = storage::Store::open(&dir.join("sectator.db"))?;
            app.manage(store);
            let indexers = commands::build_indexers(&indexers::registry::default_configs());
            app.manage(Arc::new(engine::runner::Engine::new(indexers, Default::default())));
            if let Some(w) = app.get_webview_window("main") {
                w.show()?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![commands::app_info, commands::search, commands::cancel_search])
        .run(tauri::generate_context!())
        .expect("error while running Sectator");
}
