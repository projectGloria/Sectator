//! Sectator: local-first torrent meta-search engine.

pub mod clients;
pub mod commands;
pub mod engine;
pub mod indexers;
pub mod models;
pub mod parser;
pub mod scraper;
pub mod storage;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
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
            let quitting = Arc::new(AtomicBool::new(false));
            app.manage(store.clone());
            app.manage(engine);
            app.manage(downloader.clone());

            setup_tray(app, quitting.clone())?;

            if let Some(w) = app.get_webview_window("main") {
                let win = w.clone();
                let store_for_close = store.clone();
                let quitting_for_close = quitting.clone();
                w.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        if !quitting_for_close.load(Ordering::SeqCst) && store_for_close.app_settings().run_in_tray {
                            api.prevent_close();
                            let _ = win.hide();
                        }
                    }
                });
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
            commands::download_preview_start,
            commands::download_preview_status,
            commands::download_preview_confirm,
            commands::download_preview_cancel,
            commands::download_set_sequential,
            commands::download_list,
            commands::download_pause,
            commands::download_resume,
            commands::download_remove,
            commands::download_recheck,
            commands::download_files,
            commands::download_set_wanted,
            commands::download_reveal,
            commands::download_open,
            commands::download_open_file,
            commands::download_reveal_file,
            commands::file_icon,
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
            dl.enforce_sequential();
            if n % 6 == 0 {
                let d = dl.clone();
                if let Ok(Err(e)) = tauri::async_runtime::spawn_blocking(move || d.persist(false, 5_000)).await {
                    eprintln!("saving downloads failed: {e}");
                }
            }
        }
    });
}

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

fn setup_tray(app: &tauri::App, quitting: Arc<AtomicBool>) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Show Sectator", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;

    let mut builder = TrayIconBuilder::with_id("main-tray")
        .tooltip("Sectator")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "show" => show_main_window(app),
            "quit" => {
                quitting.store(true, Ordering::SeqCst);
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main_window(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}
