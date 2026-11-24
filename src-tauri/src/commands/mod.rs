//! Tauri IPC commands. Thin wrappers: all logic lives in engine/, storage/, scraper/, clients/.

use std::sync::Arc;

use serde::Serialize;
use tauri::ipc::Channel;
use tauri::State;

use crate::clients::download::{Download, DownloadFile, DownloadPreview, Downloader, PreviewStart};
use crate::clients::{self, ClientConfig, PushOutcome};
use crate::engine::runner::{Engine, EngineConfig, Origin, SearchEvent};
use crate::indexers::{registry, IndexerConfig};
use crate::models::TorrentResult;
use crate::parser::query::SearchQuery;
use crate::storage::library::{Bookmark, HistoryEntry, IndexerHealth, Preset};
use crate::storage::settings::AppSettings;
use crate::storage::Store;

type Db<'a> = State<'a, Arc<Store>>;
type Eng<'a> = State<'a, Arc<Engine>>;
type Dl<'a> = State<'a, Arc<Downloader>>;

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

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

/// Build the live indexer set from configs, skipping disabled or invalid entries.
pub fn build_indexers(configs: &[IndexerConfig]) -> Vec<Arc<dyn crate::indexers::Indexer>> {
    configs.iter().filter(|c| c.enabled).filter_map(|c| registry::build(c).ok()).collect()
}

pub fn engine_config(s: &AppSettings) -> EngineConfig {
    EngineConfig { net: s.net.clone(), scrape_top: s.scrape_top, ..Default::default() }
}

/// Run a search, streaming `SearchEvent`s over `on_event`. Within the cache TTL a
/// repeated query is replayed from SQLite unless `force` is set. A newer call
/// cancels this one; the returned id matches the events' `searchId`.
#[tauri::command]
pub async fn search(engine: Eng<'_>, store: Db<'_>, query: String, force: Option<bool>, on_event: Channel<SearchEvent>) -> Result<u64, String> {
    let (engine, store) = (engine.inner().clone(), store.inner().clone());
    let sink = Arc::new(move |e: SearchEvent| {
        let _ = on_event.send(e);
    });
    let q = SearchQuery::parse(&query);
    let ttl = store.app_settings().cache_ttl_secs;
    let key = engine.cache_key(&q);
    if !q.is_empty() {
        let _ = store.record_history(&query, now());
    }
    if !force.unwrap_or(false) && ttl > 0 && !q.is_empty() {
        if let Ok(Some(hit)) = store.cached_search(&key, ttl, now()) {
            if !hit.is_empty() {
                return Ok(engine.replay(&query, hit, Origin::Cache, sink));
            }
        }
    }
    let out = engine.search(&query, sink).await;
    let _ = store.record_indexer_statuses(&out.statuses, now());
    if out.origin == Origin::Network && !out.cancelled && !out.results.is_empty() {
        if let Err(e) = store.save_search(&query, &key, &out.results, now()) {
            eprintln!("cache write failed: {e}");
        }
    }
    Ok(out.id)
}

#[tauri::command]
pub fn cancel_search(engine: Eng<'_>) {
    engine.cancel();
}

// ---- settings ----

#[tauri::command]
pub fn get_settings(store: Db<'_>) -> AppSettings {
    store.app_settings()
}

/// Persist settings and apply them to the running engine; returns what was stored.
#[tauri::command]
pub fn save_settings(store: Db<'_>, engine: Eng<'_>, dl: Dl<'_>, settings: AppSettings) -> Result<AppSettings, String> {
    let s = settings.sanitized();
    store.save_app_settings(&s).map_err(err)?;
    engine.set_config(engine_config(&s));
    engine.set_indexers(build_indexers(&s.indexers));
    dl.apply_settings(s.download.clone());
    Ok(s)
}

#[tauri::command]
pub fn indexer_health(store: Db<'_>) -> Result<Vec<IndexerHealth>, String> {
    store.indexer_health().map_err(err)
}

// ---- history & presets ----

#[tauri::command]
pub fn history_list(store: Db<'_>, limit: Option<usize>) -> Result<Vec<HistoryEntry>, String> {
    store.history(limit.unwrap_or(100).min(500)).map_err(err)
}

/// Delete one entry, or all history when `query` is omitted.
#[tauri::command]
pub fn history_delete(store: Db<'_>, query: Option<String>) -> Result<(), String> {
    store.delete_history(query.as_deref()).map_err(err)
}

#[tauri::command]
pub fn preset_save(store: Db<'_>, name: String, query: String) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() || query.trim().is_empty() {
        return Err("preset needs a name and a query".into());
    }
    store.save_preset(name, query.trim(), now()).map_err(err)
}

#[tauri::command]
pub fn preset_list(store: Db<'_>) -> Result<Vec<Preset>, String> {
    store.presets().map_err(err)
}

#[tauri::command]
pub fn preset_delete(store: Db<'_>, name: String) -> Result<(), String> {
    store.delete_preset(&name).map_err(err)
}

// ---- bookmarks ----

#[tauri::command]
pub fn bookmark_add(store: Db<'_>, result: TorrentResult) -> Result<(), String> {
    store.add_bookmark(&result, now()).map_err(err)
}

#[tauri::command]
pub fn bookmark_remove(store: Db<'_>, info_hash: String) -> Result<(), String> {
    store.remove_bookmark(&info_hash).map_err(err)
}

#[tauri::command]
pub fn bookmark_list(store: Db<'_>) -> Result<Vec<Bookmark>, String> {
    store.bookmarks().map_err(err)
}

// ---- cache ----

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheStats {
    pub torrents: i64,
    pub searches: i64,
}

#[tauri::command]
pub fn cache_stats(store: Db<'_>) -> Result<CacheStats, String> {
    let (torrents, searches) = store.cache_stats().map_err(err)?;
    Ok(CacheStats { torrents, searches })
}

#[tauri::command]
pub fn cache_clear(store: Db<'_>) -> Result<(), String> {
    store.clear_cache().map_err(err)
}

// ---- magnets & torrent clients ----

/// Hand a magnet link to the system's default torrent client. Only `magnet:` URIs
/// are accepted so the webview can't use this to launch arbitrary URLs.
#[tauri::command]
pub fn open_magnet(app: tauri::AppHandle, magnet: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    if !magnet.starts_with("magnet:?") || magnet.chars().any(char::is_control) {
        return Err("not a magnet link".into());
    }
    app.opener().open_url(magnet, None::<&str>).map_err(err)
}

/// Push a magnet to a configured client (`client_id`, else the default one).
#[tauri::command]
pub async fn push_to_client(store: Db<'_>, client_id: Option<String>, magnet: String) -> Result<PushOutcome, String> {
    let s = store.app_settings();
    let id = client_id.or(s.default_client.clone()).ok_or("no torrent client configured")?;
    let cfg = s.clients.into_iter().find(|c| c.id == id).ok_or("that torrent client no longer exists")?;
    clients::push(&cfg, &magnet).await.map_err(err)
}

/// Check an (unsaved) client config; returns the client's version string.
#[tauri::command]
pub async fn test_client(config: ClientConfig) -> Result<String, String> {
    clients::test(&config).await.map_err(err)
}

// ---- built-in downloader (clients/download) ----

/// Run a blocking downloader call off the async runtime (libtorrent calls wait on
/// its network thread; the first one also starts the session).
async fn blocking<T: Send + 'static>(dl: &Arc<Downloader>, f: impl FnOnce(&Downloader) -> Result<T, String> + Send + 'static) -> Result<T, String> {
    let dl = dl.clone();
    tauri::async_runtime::spawn_blocking(move || f(&dl)).await.map_err(err)?
}

/// Start downloading a magnet; returns its infohash. `save_path` overrides the folder.
#[tauri::command]
pub async fn download_add(dl: Dl<'_>, magnet: String, save_path: Option<String>) -> Result<String, String> {
    let hash = blocking(dl.inner(), move |d| d.add_magnet(&magnet, save_path.as_deref(), false).map_err(err)).await?;
    // Write its resume file right away so a crash doesn't lose the new download.
    let _ = blocking(dl.inner(), |d| d.persist(false, 3_000).map_err(err)).await;
    Ok(hash)
}

/// Stage a magnet to fetch its metadata before starting the download.
#[tauri::command]
pub async fn download_preview_start(dl: Dl<'_>, magnet: String, save_path: Option<String>) -> Result<PreviewStart, String> {
    blocking(dl.inner(), move |d| d.preview_start(&magnet, save_path.as_deref()).map_err(err)).await
}

/// Poll metadata & file list of a staged (or existing) torrent.
#[tauri::command]
pub fn download_preview_status(dl: Dl<'_>, info_hash: String) -> Result<DownloadPreview, String> {
    dl.preview_status(&info_hash).map_err(err)
}

/// Confirm a staged download with file selection, sequential mode, and optional save folder.
#[tauri::command]
pub async fn download_preview_confirm(
    dl: Dl<'_>,
    info_hash: String,
    save_path: Option<String>,
    files: Option<Vec<(i32, bool)>>,
    sequential: bool,
) -> Result<(), String> {
    blocking(dl.inner(), move |d| {
        d.preview_confirm(&info_hash, save_path.as_deref(), files.as_deref(), sequential).map_err(err)
    })
    .await?;
    let _ = blocking(dl.inner(), |d| d.persist(false, 3_000).map_err(err)).await;
    Ok(())
}

/// Cancel a staged preview (removes it only if it was staged, never an already-active download).
#[tauri::command]
pub fn download_preview_cancel(dl: Dl<'_>, info_hash: String) -> Result<(), String> {
    dl.preview_cancel(&info_hash).map_err(err)
}

#[tauri::command]
pub fn download_set_sequential(dl: Dl<'_>, info_hash: String, sequential: bool) -> Result<(), String> {
    dl.set_sequential(&info_hash, sequential).map_err(err)
}

#[tauri::command]
pub fn download_list(dl: Dl<'_>) -> Vec<Download> {
    dl.list()
}

#[tauri::command]
pub fn download_pause(dl: Dl<'_>, info_hash: String) -> Result<(), String> {
    dl.pause(&info_hash).map_err(err)
}

#[tauri::command]
pub fn download_resume(dl: Dl<'_>, info_hash: String) -> Result<(), String> {
    dl.resume(&info_hash).map_err(err)
}

#[tauri::command]
pub fn download_recheck(dl: Dl<'_>, info_hash: String) -> Result<(), String> {
    dl.recheck(&info_hash).map_err(err)
}

/// Remove a download; `delete_files` also deletes what it downloaded.
#[tauri::command]
pub fn download_remove(dl: Dl<'_>, info_hash: String, delete_files: bool) -> Result<(), String> {
    dl.remove(&info_hash, delete_files).map_err(err)
}

#[tauri::command]
pub fn download_files(dl: Dl<'_>, info_hash: String) -> Result<Vec<DownloadFile>, String> {
    dl.files(&info_hash).map_err(err)
}

/// Pick files to fetch: `[fileIndex, wanted]` pairs.
#[tauri::command]
pub fn download_set_wanted(dl: Dl<'_>, info_hash: String, files: Vec<(i32, bool)>) -> Result<(), String> {
    dl.set_wanted(&info_hash, &files).map_err(err)
}

/// Show a download's files in Explorer.
#[tauri::command]
pub fn download_reveal(app: tauri::AppHandle, dl: Dl<'_>, info_hash: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let path = dl.content_path(&info_hash).ok_or("no such download")?;
    app.opener().reveal_item_in_dir(path).map_err(err)
}

/// Open / run a downloaded item (file or folder) with the system default handler.
#[tauri::command]
pub fn download_open(app: tauri::AppHandle, dl: Dl<'_>, info_hash: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let path = dl.content_path(&info_hash).ok_or("no such download")?;
    if !path.exists() {
        return Err("files aren't on disk yet".into());
    }
    app.opener().open_path(path.to_string_lossy(), None::<&str>).map_err(err)
}

/// Open / run a specific file inside a download.
#[tauri::command]
pub fn download_open_file(app: tauri::AppHandle, dl: Dl<'_>, info_hash: String, file_index: i32) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let path = dl.file_path(&info_hash, file_index).ok_or("file isn't on disk yet")?;
    app.opener().open_path(path.to_string_lossy(), None::<&str>).map_err(err)
}

/// Reveal a specific file inside a download in Explorer.
#[tauri::command]
pub fn download_reveal_file(app: tauri::AppHandle, dl: Dl<'_>, info_hash: String, file_index: i32) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let path = dl.file_path(&info_hash, file_index).ok_or("file isn't on disk yet")?;
    app.opener().reveal_item_in_dir(path).map_err(err)
}

/// The Windows shell icon (the default app's icon) for a file extension, as PNG bytes.
#[tauri::command]
pub async fn file_icon(ext: String) -> Result<tauri::ipc::Response, String> {
    let png = tauri::async_runtime::spawn_blocking(move || crate::clients::download::icon::for_extension(&ext)).await.map_err(err)?;
    png.map(tauri::ipc::Response::new).ok_or_else(|| "no icon".to_string())
}
