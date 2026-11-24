//! Search history, saved query presets, bookmarks and per-indexer health.

use rusqlite::params;
use serde::{Deserialize, Serialize};

use super::{Result, Store};
use crate::models::{IndexerState, IndexerStatus, TorrentResult};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub query: String,
    pub last_used: i64,
    pub uses: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Preset {
    pub name: String,
    pub query: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bookmark {
    pub info_hash: String,
    pub title: String,
    pub magnet: String,
    pub size: u64,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexerHealth {
    pub indexer_id: String,
    pub searches: i64,
    pub failures: i64,
    pub avg_latency_ms: Option<f64>,
    pub last_ok_at: Option<i64>,
    pub last_error: Option<String>,
}

const HISTORY_KEEP: i64 = 500;

impl Store {
    pub fn record_history(&self, query: &str, now: i64) -> Result<()> {
        let q = query.trim();
        if q.is_empty() {
            return Ok(());
        }
        let conn = self.conn();
        conn.execute(
            "INSERT INTO history(raw_query, last_used, uses) VALUES (?1, ?2, 1)
             ON CONFLICT(raw_query) DO UPDATE SET last_used = excluded.last_used, uses = uses + 1",
            params![q, now],
        )?;
        conn.execute(
            "DELETE FROM history WHERE raw_query NOT IN (SELECT raw_query FROM history ORDER BY last_used DESC LIMIT ?1)",
            [HISTORY_KEEP],
        )?;
        Ok(())
    }

    pub fn history(&self, limit: usize) -> Result<Vec<HistoryEntry>> {
        let conn = self.conn();
        let mut stmt = conn.prepare("SELECT raw_query, last_used, uses FROM history ORDER BY last_used DESC LIMIT ?1")?;
        let rows = stmt.query_map([limit as i64], |r| Ok(HistoryEntry { query: r.get(0)?, last_used: r.get(1)?, uses: r.get(2)? }))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn delete_history(&self, query: Option<&str>) -> Result<()> {
        let conn = self.conn();
        match query {
            Some(q) => conn.execute("DELETE FROM history WHERE raw_query = ?1", [q])?,
            None => conn.execute("DELETE FROM history", [])?,
        };
        Ok(())
    }

    pub fn save_preset(&self, name: &str, query: &str, now: i64) -> Result<()> {
        self.conn().execute(
            "INSERT INTO presets(name, raw_query, created_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(name) DO UPDATE SET raw_query = excluded.raw_query",
            params![name.trim(), query.trim(), now],
        )?;
        Ok(())
    }

    pub fn presets(&self) -> Result<Vec<Preset>> {
        let conn = self.conn();
        let mut stmt = conn.prepare("SELECT name, raw_query FROM presets ORDER BY name COLLATE NOCASE")?;
        let rows = stmt.query_map([], |r| Ok(Preset { name: r.get(0)?, query: r.get(1)? }))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn delete_preset(&self, name: &str) -> Result<()> {
        self.conn().execute("DELETE FROM presets WHERE name = ?1", [name])?;
        Ok(())
    }

    pub fn add_bookmark(&self, r: &TorrentResult, now: i64) -> Result<()> {
        self.conn().execute(
            "INSERT INTO bookmarks(info_hash, title, magnet, size, created_at) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(info_hash) DO UPDATE SET title = excluded.title, magnet = excluded.magnet, size = excluded.size",
            params![r.info_hash, r.title, r.magnet, r.size as i64, now],
        )?;
        Ok(())
    }

    pub fn remove_bookmark(&self, info_hash: &str) -> Result<()> {
        self.conn().execute("DELETE FROM bookmarks WHERE info_hash = ?1", [info_hash])?;
        Ok(())
    }

    pub fn bookmarks(&self) -> Result<Vec<Bookmark>> {
        let conn = self.conn();
        let mut stmt = conn.prepare("SELECT info_hash, title, magnet, size, created_at FROM bookmarks ORDER BY created_at DESC")?;
        let rows = stmt.query_map([], |r| {
            Ok(Bookmark { info_hash: r.get(0)?, title: r.get(1)?, magnet: r.get(2)?, size: r.get::<_, i64>(3)? as u64, created_at: r.get(4)? })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Fold one search's indexer outcomes into rolling health (EWMA latency).
    pub fn record_indexer_statuses(&self, statuses: &[IndexerStatus], now: i64) -> Result<()> {
        let conn = self.conn();
        for s in statuses {
            let failed = matches!(s.state, IndexerState::Error | IndexerState::Timeout);
            if !failed && s.state != IndexerState::Ok {
                continue;
            }
            conn.execute(
                "INSERT INTO indexer_stats(indexer_id, searches, failures, avg_latency, last_ok_at, last_error)
                 VALUES (?1, 1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(indexer_id) DO UPDATE SET
                   searches = searches + 1,
                   failures = failures + excluded.failures,
                   avg_latency = CASE WHEN excluded.avg_latency IS NULL THEN avg_latency
                                      WHEN avg_latency IS NULL THEN excluded.avg_latency
                                      ELSE avg_latency * 0.8 + excluded.avg_latency * 0.2 END,
                   last_ok_at = COALESCE(excluded.last_ok_at, last_ok_at),
                   last_error = CASE WHEN excluded.failures > 0 THEN excluded.last_error ELSE last_error END",
                params![
                    s.indexer_id,
                    failed as i64,
                    if failed { None } else { s.latency_ms.map(|l| l as f64) },
                    if failed { None } else { Some(now) },
                    s.error
                ],
            )?;
        }
        Ok(())
    }

    pub fn indexer_health(&self) -> Result<Vec<IndexerHealth>> {
        let conn = self.conn();
        let mut stmt = conn.prepare("SELECT indexer_id, searches, failures, avg_latency, last_ok_at, last_error FROM indexer_stats")?;
        let rows = stmt.query_map([], |r| {
            Ok(IndexerHealth {
                indexer_id: r.get(0)?,
                searches: r.get(1)?,
                failures: r.get(2)?,
                avg_latency_ms: r.get(3)?,
                last_ok_at: r.get(4)?,
                last_error: r.get(5)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::merge::Merger;
    use crate::indexers::nyaa;
    use crate::models::SourceType;

    #[test]
    fn history_is_mru_and_counts_uses() {
        let s = Store::open_in_memory().unwrap();
        s.record_history("ubuntu", 1).unwrap();
        s.record_history("debian", 2).unwrap();
        s.record_history(" ubuntu ", 3).unwrap();
        s.record_history("   ", 4).unwrap();
        let h = s.history(10).unwrap();
        assert_eq!(h.iter().map(|e| e.query.as_str()).collect::<Vec<_>>(), ["ubuntu", "debian"]);
        assert_eq!(h[0].uses, 2);
        s.delete_history(Some("ubuntu")).unwrap();
        assert_eq!(s.history(10).unwrap().len(), 1);
        s.delete_history(None).unwrap();
        assert!(s.history(10).unwrap().is_empty());
    }

    #[test]
    fn presets_and_bookmarks() {
        let s = Store::open_in_memory().unwrap();
        s.save_preset("4K remux", "res:2160p src:remux", 1).unwrap();
        s.save_preset("4K remux", "res:2160p src:remux seeds:>5", 2).unwrap();
        s.save_preset("anime", "cat:anime", 3).unwrap();
        let p = s.presets().unwrap();
        assert_eq!(p.len(), 2);
        assert_eq!(p[0], Preset { name: "4K remux".into(), query: "res:2160p src:remux seeds:>5".into() });
        s.delete_preset("anime").unwrap();
        assert_eq!(s.presets().unwrap().len(), 1);

        let hit = nyaa::parse(include_str!("../indexers/fixtures/nyaa.xml")).unwrap().remove(0);
        let mut m = Merger::new();
        let h = m.add("nyaa", "Nyaa", SourceType::Curated, hit).unwrap();
        let r = m.get(&h).unwrap().clone();
        s.add_bookmark(&r, 10).unwrap();
        s.add_bookmark(&r, 11).unwrap();
        let b = s.bookmarks().unwrap();
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].info_hash, r.info_hash);
        assert!(b[0].magnet.starts_with("magnet:?xt=urn:btih:"));
        s.remove_bookmark(&r.info_hash).unwrap();
        assert!(s.bookmarks().unwrap().is_empty());
    }

    #[test]
    fn indexer_health_rolls_up() {
        let s = Store::open_in_memory().unwrap();
        let st = |state, ms: Option<u64>, err: Option<&str>| IndexerStatus {
            indexer_id: "x".into(),
            name: "X".into(),
            source_type: SourceType::Curated,
            state,
            latency_ms: ms,
            count: 0,
            error: err.map(Into::into),
            proxied: false,
        };
        s.record_indexer_statuses(&[st(IndexerState::Ok, Some(100), None)], 1).unwrap();
        s.record_indexer_statuses(&[st(IndexerState::Ok, Some(200), None)], 2).unwrap();
        s.record_indexer_statuses(&[st(IndexerState::Timeout, Some(5000), Some("timed out"))], 3).unwrap();
        s.record_indexer_statuses(&[st(IndexerState::Skipped, None, None)], 4).unwrap();
        let h = &s.indexer_health().unwrap()[0];
        assert_eq!((h.searches, h.failures), (3, 1));
        assert!((h.avg_latency_ms.unwrap() - 120.0).abs() < 1e-9);
        assert_eq!(h.last_ok_at, Some(2));
        assert_eq!(h.last_error.as_deref(), Some("timed out"));
    }
}
