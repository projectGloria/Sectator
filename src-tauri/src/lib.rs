//! Sectator: local-first torrent meta-search engine.

pub mod clients;
pub mod commands;
pub mod engine;
pub mod indexers;
pub mod models;
pub mod parser;
pub mod scraper;
pub mod storage;

use std::sync::Arc;

use tauri::Manager;

/// Cached searches and torrents untouched for this long are dropped at startup.
const CACHE_RETENTION_SECS: i64 = 30 * 86_400;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            let store = Arc::new(storage::Store::open(&dir.join("sectator.db"))?);
            let _ = store.prune_cache(chrono::Utc::now().timestamp() - CACHE_RETENTION_SECS);
            let settings = store.app_settings();
            let engine = Arc::new(engine::runner::Engine::new(commands::build_indexers(&settings.indexers), commands::engine_config(&settings)));
            let local = store.clone();
            engine.set_offline_source(Arc::new(move |q| local.local_search(q, 200).unwrap_or_default()));
            app.manage(store);
            app.manage(engine);
            if let Some(w) = app.get_webview_window("main") {
                w.show()?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::search,
            commands::cancel_search,
            commands::get_settings,
            commands::save_settings,
            commands::indexer_health,
            commands::history_list,
            commands::history_delete,
            commands::preset_save,
            commands::preset_list,
            commands::preset_delete,
            commands::bookmark_add,
            commands::bookmark_remove,
            commands::bookmark_list,
            commands::cache_stats,
            commands::cache_clear,
            commands::open_magnet,
            commands::push_to_client,
            commands::test_client,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Sectator");
}
