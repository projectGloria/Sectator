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
use std::time::Duration;

use tauri::Manager;

use clients::download::Downloader;

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
            let downloads = app.path().download_dir().unwrap_or_else(|_| dir.join("downloads"));
            let ua = format!("Sectator/{}", env!("CARGO_PKG_VERSION"));
            let downloader = Arc::new(Downloader::new(dir.join("downloads"), downloads, settings.download.clone(), ua));
            spawn_download_upkeep(downloader.clone());
            app.manage(store);
            app.manage(engine);
            app.manage(downloader);
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
            commands::download_add,
            commands::download_list,
            commands::download_pause,
            commands::download_resume,
            commands::download_remove,
            commands::download_recheck,
            commands::download_files,
            commands::download_set_wanted,
            commands::download_reveal,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Sectator")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                if let Some(dl) = app.try_state::<Arc<Downloader>>() {
                    if let Err(e) = dl.persist(true, 3_000) {
                        eprintln!("saving downloads failed: {e}");
                    }
                }
            }
        });
}

/// Restore saved downloads at launch, then keep fast-resume files fresh and apply
/// the seed-ratio limit in the background.
fn spawn_download_upkeep(dl: Arc<Downloader>) {
    tauri::async_runtime::spawn(async move {
        if dl.has_saved() {
            let d = dl.clone();
            if let Ok(Err(e)) = tauri::async_runtime::spawn_blocking(move || d.start()).await {
                eprintln!("downloader failed to start: {e}");
            }
        }
        let mut tick = tokio::time::interval(Duration::from_secs(5));
        let mut n: u64 = 0;
        loop {
            tick.tick().await;
            n += 1;
            if !dl.started() {
                continue;
            }
            dl.enforce_ratio();
            if n % 6 == 0 {
                let d = dl.clone();
                if let Ok(Err(e)) = tauri::async_runtime::spawn_blocking(move || d.persist(false, 5_000)).await {
                    eprintln!("saving downloads failed: {e}");
                }
            }
        }
    });
}
