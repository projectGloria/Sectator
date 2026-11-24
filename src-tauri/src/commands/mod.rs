//! Tauri IPC commands. Thin wrappers: all logic lives in engine/, storage/, scraper/, clients/.

use std::sync::Arc;

use serde::Serialize;
use tauri::ipc::Channel;
use tauri::State;

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
pub fn save_settings(store: Db<'_>, engine: Eng<'_>, settings: AppSettings) -> Result<AppSettings, String> {
    let s = settings.sanitized();
    store.save_app_settings(&s).map_err(err)?;
    engine.set_config(engine_config(&s));
    engine.set_indexers(build_indexers(&s.indexers));
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
