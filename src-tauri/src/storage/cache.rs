//! Result cache: every completed search is stored with its merged results, so a
//! repeat within the TTL replays instantly, and the FTS5 mirror powers an offline
//! fallback over everything seen before.

use rusqlite::{params, OptionalExtension};

use super::{Result, Store};
use crate::models::TorrentResult;
use crate::parser::query::SearchQuery;

/// Upper bound on stored inner files per torrent (giant packs list tens of thousands).
const MAX_FILES: usize = 2_000;

/// `"term"` quoting for an FTS5 MATCH expression (double any embedded quotes).
fn fts_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// AND of every term and phrase; `None` when there's nothing to match on.
pub fn fts_expr(q: &SearchQuery) -> Option<String> {
    let parts: Vec<String> = q.terms.iter().chain(&q.phrases).filter(|t| !t.trim().is_empty()).map(|t| fts_quote(t)).collect();
    (!parts.is_empty()).then(|| parts.join(" AND "))
}

impl Store {
    pub fn save_search(&self, raw: &str, key: &str, results: &[TorrentResult], now: i64) -> Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        tx.execute("INSERT INTO searches(query_key, raw_query, created_at) VALUES (?1, ?2, ?3)", params![key, raw, now])?;
        let search_id = tx.last_insert_rowid();
        {
            let mut upsert = tx.prepare(
                "INSERT INTO torrents(info_hash, title, size, seeders, leechers, uploaded_at, category, payload, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(info_hash) DO UPDATE SET title=excluded.title, size=excluded.size, seeders=excluded.seeders,
                   leechers=excluded.leechers, uploaded_at=excluded.uploaded_at, category=excluded.category,
                   payload=excluded.payload, updated_at=excluded.updated_at",
            )?;
            let mut clear_files = tx.prepare("DELETE FROM files WHERE info_hash = ?1")?;
            let mut add_file = tx.prepare("INSERT OR IGNORE INTO files(info_hash, path, size) VALUES (?1, ?2, ?3)")?;
            let mut link = tx.prepare("INSERT OR IGNORE INTO search_results(search_id, info_hash) VALUES (?1, ?2)")?;
            for r in results {
                upsert.execute(params![
                    r.info_hash,
                    r.title,
                    r.size as i64,
                    r.best_seeders(),
                    r.best_leechers(),
                    r.uploaded_at,
                    r.category.map(|c| c.as_str()),
                    serde_json::to_string(r)?,
                    now
                ])?;
                if !r.files.is_empty() {
                    clear_files.execute([&r.info_hash])?;
                    for f in r.files.iter().take(MAX_FILES) {
                        add_file.execute(params![r.info_hash, f.path, f.size as i64])?;
                    }
                }
                link.execute(params![search_id, r.info_hash])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Results of the newest search with this cache key, if younger than `ttl_secs`.
    pub fn cached_search(&self, key: &str, ttl_secs: i64, now: i64) -> Result<Option<Vec<TorrentResult>>> {
        let conn = self.conn();
        let id: Option<i64> = conn
            .query_row(
                "SELECT id FROM searches WHERE query_key = ?1 AND created_at >= ?2 ORDER BY created_at DESC, id DESC LIMIT 1",
                params![key, now - ttl_secs],
                |r| r.get(0),
            )
            .optional()?;
        let Some(id) = id else { return Ok(None) };
        let mut stmt = conn.prepare("SELECT t.payload FROM search_results s JOIN torrents t ON t.info_hash = s.info_hash WHERE s.search_id = ?1")?;
        let rows = stmt.query_map([id], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for p in rows {
            if let Ok(r) = serde_json::from_str(&p?) {
                out.push(r);
            }
        }
        Ok(Some(out))
    }

    /// FTS5 search over every cached title and inner file path.
    pub fn local_search(&self, q: &SearchQuery, limit: usize) -> Result<Vec<TorrentResult>> {
        let Some(expr) = fts_expr(q) else { return Ok(vec![]) };
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT payload FROM torrents WHERE info_hash IN (
                SELECT info_hash FROM torrents_fts WHERE torrents_fts MATCH ?1
                UNION SELECT info_hash FROM files_fts WHERE files_fts MATCH ?1
             ) ORDER BY updated_at DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![expr, limit as i64], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for p in rows {
            if let Ok(r) = serde_json::from_str(&p?) {
                out.push(r);
            }
        }
        Ok(out)
    }

    /// Drop searches older than `cutoff` and torrents no remaining search references.
    pub fn prune_cache(&self, cutoff: i64) -> Result<usize> {
        let conn = self.conn();
        conn.execute("DELETE FROM searches WHERE created_at < ?1", [cutoff])?;
        Ok(conn.execute("DELETE FROM torrents WHERE info_hash NOT IN (SELECT info_hash FROM search_results)", [])?)
    }

    pub fn clear_cache(&self) -> Result<()> {
        let conn = self.conn();
        conn.execute_batch("DELETE FROM searches; DELETE FROM torrents;")?;
        Ok(())
    }

    /// (cached torrents, stored searches)
    pub fn cache_stats(&self) -> Result<(i64, i64)> {
        let conn = self.conn();
        let t = conn.query_row("SELECT count(*) FROM torrents", [], |r| r.get(0))?;
        let s = conn.query_row("SELECT count(*) FROM searches", [], |r| r.get(0))?;
        Ok((t, s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indexers::apibay;
    use crate::engine::merge::Merger;
    use crate::models::{SourceType, TorrentFile};

    /// Real adapter output merged by the real engine, not hand-built rows.
    fn results() -> Vec<TorrentResult> {
        let hits = apibay::parse(include_str!("../indexers/fixtures/apibay.json")).unwrap();
        let mut m = Merger::new();
        for h in hits {
            m.add("apibay", "The Pirate Bay", SourceType::Curated, h);
        }
        let mut v = m.into_results();
        v[0].files = vec![TorrentFile { path: "disk/Casper-RC2.iso".into(), size: 5 }];
        v
    }

    #[test]
    fn ttl_cache_round_trip() {
        let store = Store::open_in_memory().unwrap();
        let rs = results();
        store.save_search("ubuntu cat:software", "ubuntu|", &rs, 1_000).unwrap();
        let hit = store.cached_search("ubuntu|", 600, 1_500).unwrap().unwrap();
        assert_eq!(hit.len(), rs.len());
        assert!(hit.iter().any(|r| r == &rs[0]));
        assert!(store.cached_search("ubuntu|", 600, 1_700).unwrap().is_none(), "expired");
        assert!(store.cached_search("other|", 600, 1_500).unwrap().is_none());

        // A second save of the same torrents upserts rather than duplicating.
        store.save_search("ubuntu", "ubuntu|", &rs, 1_600).unwrap();
        assert_eq!(store.cache_stats().unwrap(), (rs.len() as i64, 2));
        assert_eq!(store.prune_cache(1_200).unwrap(), 0);
        assert_eq!(store.cache_stats().unwrap().1, 1);
        store.prune_cache(2_000).unwrap();
        assert_eq!(store.cache_stats().unwrap(), (0, 0));
    }

    #[test]
    fn offline_fts_matches_titles_and_files() {
        let store = Store::open_in_memory().unwrap();
        let rs = results();
        store.save_search("ubuntu", "ubuntu|", &rs, 1).unwrap();
        let by_title = store.local_search(&SearchQuery::parse(&rs[0].title.split_whitespace().next().unwrap().to_lowercase()), 50).unwrap();
        assert!(by_title.iter().any(|r| r.info_hash == rs[0].info_hash));
        let by_file = store.local_search(&SearchQuery::parse("casper"), 50).unwrap();
        assert_eq!(by_file.len(), 1);
        assert_eq!(by_file[0].info_hash, rs[0].info_hash);
        // Quotes in user input can't break the MATCH syntax.
        assert!(store.local_search(&SearchQuery::parse("\"a\"\"b"), 5).is_ok());
        assert!(store.local_search(&SearchQuery::parse("cat:books"), 5).unwrap().is_empty());
    }
}
