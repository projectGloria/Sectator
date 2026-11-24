//! Persistent app settings, stored as JSON values in the `settings` table.

use rusqlite::{params, OptionalExtension};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use super::{Result, Store};
use crate::clients::download::DownloadSettings;
use crate::clients::ClientConfig;
use crate::indexers::{registry, IndexerConfig, NetworkSettings};

const APP_KEY: &str = "app";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    pub net: NetworkSettings,
    pub indexers: Vec<IndexerConfig>,
    /// How long a finished search is replayed from cache instead of re-run.
    pub cache_ttl_secs: i64,
    /// How many top results get a live tracker scrape (0 disables).
    pub scrape_top: usize,
    pub clients: Vec<ClientConfig>,
    /// Id of the client "Send to client" uses; `None` means the OS magnet handler.
    pub default_client: Option<String>,
    /// Built-in downloader (libtorrent).
    pub download: DownloadSettings,
    /// UI colour theme id; unknown ids fall back to the default.
    pub theme: String,
    /// Webview zoom factor (1.0 = 100%), set with Ctrl +/-/0 or in Appearance.
    pub ui_zoom: f64,
}

/// Allowed UI zoom range.
pub const ZOOM_RANGE: (f64, f64) = (0.5, 2.0);

/// Theme ids the frontend ships (`src/lib/theme.ts`); the first is the default.
pub const THEMES: [&str; 3] = ["claude", "claude-dark", "midnight"];

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            net: NetworkSettings::default(),
            indexers: registry::default_configs(),
            cache_ttl_secs: 1_800,
            scrape_top: 60,
            clients: Vec::new(),
            default_client: None,
            download: DownloadSettings::default(),
            theme: THEMES[0].to_string(),
            ui_zoom: 1.0,
        }
    }
}

impl AppSettings {
    /// Append any built-in indexer the stored list doesn't know yet (new releases
    /// ship new defaults without clobbering the user's edits).
    fn merge_new_defaults(&mut self) {
        for d in registry::default_configs() {
            if !self.indexers.iter().any(|c| c.id == d.id) {
                self.indexers.push(d);
            }
        }
    }

    /// Clamp values the UI could send out of range.
    pub fn sanitized(mut self) -> Self {
        self.cache_ttl_secs = self.cache_ttl_secs.clamp(0, 7 * 86_400);
        self.scrape_top = self.scrape_top.min(500);
        self.net.clearnet_timeout_ms = self.net.clearnet_timeout_ms.clamp(1_000, 30_000);
        self.net.tor_timeout_ms = self.net.tor_timeout_ms.clamp(2_000, 60_000);
        self.net.proxy = self.net.proxy.take().map(|p| p.trim().to_string()).filter(|p| !p.is_empty());
        self.net.doh_url = self.net.doh_url.take().map(|p| p.trim().to_string()).filter(|p| !p.is_empty());
        self.download = self.download.sanitized();
        self.ui_zoom = if self.ui_zoom.is_finite() { (self.ui_zoom.clamp(ZOOM_RANGE.0, ZOOM_RANGE.1) * 100.0).round() / 100.0 } else { 1.0 };
        if !THEMES.contains(&self.theme.as_str()) {
            self.theme = THEMES[0].to_string();
        }
        if self.default_client.as_ref().is_some_and(|id| !self.clients.iter().any(|c| &c.id == id)) {
            self.default_client = None;
        }
        self
    }
}

impl Store {
    pub fn get_json<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        let v: Option<String> = self.conn().query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0)).optional()?;
        Ok(match v {
            Some(s) => Some(serde_json::from_str(&s)?),
            None => None,
        })
    }

    pub fn set_json<T: Serialize>(&self, key: &str, value: &T) -> Result<()> {
        self.conn().execute(
            "INSERT INTO settings(key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, serde_json::to_string(value)?],
        )?;
        Ok(())
    }

    /// Stored settings (or defaults); an unreadable blob falls back to defaults.
    pub fn app_settings(&self) -> AppSettings {
        let mut s = self.get_json::<AppSettings>(APP_KEY).ok().flatten().unwrap_or_default();
        s.merge_new_defaults();
        s
    }

    pub fn save_app_settings(&self, s: &AppSettings) -> Result<()> {
        self.set_json(APP_KEY, s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clients::ClientKind;

    #[test]
    fn round_trip_and_defaults() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(store.app_settings(), AppSettings::default());

        let mut s = AppSettings::default();
        s.indexers.retain(|c| c.id != "nyaa");
        s.indexers[0].enabled = false;
        s.cache_ttl_secs = 60;
        s.net.proxy = Some("socks5h://127.0.0.1:9050".into());
        s.clients.push(ClientConfig { id: "q".into(), kind: ClientKind::Qbittorrent, name: "qB".into(), url: "http://127.0.0.1:8080".into(), ..Default::default() });
        store.save_app_settings(&s).unwrap();

        let back = store.app_settings();
        assert_eq!(back.cache_ttl_secs, 60);
        assert!(!back.indexers[0].enabled, "user edits survive");
        assert!(back.indexers.iter().any(|c| c.id == "nyaa"), "removed default is restored");
        assert_eq!(back.clients.len(), 1);
    }

    #[test]
    fn partial_and_corrupt_blobs() {
        let store = Store::open_in_memory().unwrap();
        store.conn().execute("INSERT INTO settings(key, value) VALUES ('app', '{\"scrapeTop\": 5}')", []).unwrap();
        let s = store.app_settings();
        assert_eq!(s.scrape_top, 5);
        assert_eq!(s.cache_ttl_secs, 1_800);
        store.conn().execute("UPDATE settings SET value = 'not json' WHERE key = 'app'", []).unwrap();
        assert_eq!(store.app_settings(), AppSettings::default());
    }

    #[test]
    fn sanitize_clamps() {
        let mut s = AppSettings { cache_ttl_secs: -5, scrape_top: 10_000, default_client: Some("gone".into()), theme: "neon".into(), ui_zoom: 7.0, ..Default::default() };
        s.net.proxy = Some("   ".into());
        let s = s.sanitized();
        assert_eq!((s.cache_ttl_secs, s.scrape_top), (0, 500));
        assert!(s.net.proxy.is_none() && s.default_client.is_none());
        assert_eq!(s.theme, "claude", "unknown theme falls back to the default");
        assert_eq!(s.ui_zoom, 2.0, "zoom clamped to its range");
        let z = AppSettings { ui_zoom: f64::NAN, ..Default::default() }.sanitized();
        assert_eq!(z.ui_zoom, 1.0, "NaN zoom resets to 100%");
        let z = AppSettings { ui_zoom: 1.1000000001, ..Default::default() }.sanitized();
        assert_eq!(z.ui_zoom, 1.1, "rounded to whole percent");
    }
}
