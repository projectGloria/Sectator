//! Nyaa via its RSS search feed (`?page=rss`). Curated; anime-focused.

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use super::{http, rss, text, Indexer, IndexerConfig, IndexerError, IndexerMeta, SearchContext};
use crate::models::{Category, IndexerHit, SourceType};
use crate::parser::query::{parse_size, SearchQuery};

pub const NYAA_TRACKER: &str = "http://nyaa.tracker.wf:7777/announce";

pub struct Nyaa {
    meta: IndexerMeta,
}

impl Nyaa {
    pub fn new(cfg: &IndexerConfig) -> Self {
        Self {
            meta: IndexerMeta {
                id: cfg.id.clone(),
                name: cfg.name.clone(),
                source_type: SourceType::Curated,
                categories: vec![Category::Anime, Category::Music, Category::Books, Category::Software, Category::Games, Category::Tv],
                base_urls: cfg.base_urls.clone(),
                proxy: cfg.proxy.clone(),
                timeout_ms: cfg.timeout_ms,
            },
        }
    }
}

/// Nyaa `categoryId` (`1_2` = Anime / English-translated, …).
pub fn category(id: &str) -> Option<Category> {
    let (main, sub) = id.split_once('_').unwrap_or((id, "0"));
    Some(match (main, sub) {
        ("1", _) => Category::Anime,
        ("2", _) => Category::Music,
        ("3", _) => Category::Books,
        ("6", "1") => Category::Software,
        ("6", "2") => Category::Games,
        ("5", _) => Category::Other,
        _ => return None, // live action: let the release parser decide
    })
}

fn cat_param(c: Option<Category>) -> &'static str {
    match c {
        Some(Category::Anime) => "1_0",
        Some(Category::Music) => "2_0",
        Some(Category::Books) => "3_0",
        Some(Category::Software) => "6_1",
        Some(Category::Games) => "6_2",
        Some(Category::Tv | Category::Movies) => "4_0",
        _ => "0_0",
    }
}

pub fn parse(xml: &str) -> Result<Vec<IndexerHit>, IndexerError> {
    Ok(rss::parse_items(xml)?
        .into_iter()
        .filter_map(|it| {
            let hash = it.field("nyaa:infoHash")?.to_string();
            Some(IndexerHit {
                info_hash: hash,
                title: it.field("title").unwrap_or_default().to_string(),
                size: it.field("nyaa:size").and_then(parse_size).unwrap_or(0),
                seeders: it.field("nyaa:seeders").and_then(|s| s.parse().ok()),
                leechers: it.field("nyaa:leechers").and_then(|s| s.parse().ok()),
                completed: it.field("nyaa:downloads").and_then(|s| s.parse().ok()),
                uploaded_at: it.field("pubDate").and_then(text::parse_rfc2822),
                uploader: None,
                verified_uploader: it.field("nyaa:trusted").is_some_and(|t| t.eq_ignore_ascii_case("yes")),
                category: it.field("nyaa:categoryId").and_then(category),
                trackers: vec![NYAA_TRACKER.to_string()],
                files: Vec::new(),
                details_url: it.field("guid").map(str::to_string),
            })
        })
        .collect())
}

#[async_trait]
impl Indexer for Nyaa {
    fn meta(&self) -> &IndexerMeta {
        &self.meta
    }

    async fn search(&self, ctx: &SearchContext, q: &SearchQuery, cancel: &CancellationToken) -> Result<Vec<IndexerHit>, IndexerError> {
        let terms = q.indexer_text();
        if terms.is_empty() {
            return Ok(Vec::new());
        }
        http::with_mirrors(&self.meta.base_urls, cancel, |base| {
            let url = format!("{}/?page=rss&q={}&c={}&f=0", text::base(&base), text::enc(&terms), cat_param(q.category));
            let req = ctx.client.get(url);
            async move { parse(&http::fetch_text(req).await?) }
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_live_fixture() {
        let hits = parse(include_str!("fixtures/nyaa.xml")).unwrap();
        assert_eq!(hits.len(), 3);
        let h = &hits[0];
        assert!(h.title.starts_with("Frieren - Oltre la Fine del Viaggio (2023)"));
        assert_eq!(h.info_hash, "444d6b7665bf616065d4c3793642bc41c17e767e");
        assert_eq!((h.seeders, h.leechers, h.completed), (Some(6), Some(3), Some(97)));
        assert_eq!(h.size, (42.6 * 1024.0 * 1024.0 * 1024.0_f64).round() as u64);
        assert_eq!(h.uploaded_at, Some(1_793_374_320));
        assert_eq!(h.category, Some(Category::Anime));
        assert!(!h.verified_uploader);
        assert_eq!(h.details_url.as_deref(), Some("https://nyaa.si/view/2180356"));
        assert_eq!(h.trackers, vec![NYAA_TRACKER]);
        assert!(hits[1].title.contains("Sousou no Frieren"));
    }

    #[test]
    fn categories() {
        assert_eq!(category("1_2"), Some(Category::Anime));
        assert_eq!(category("6_2"), Some(Category::Games));
        assert_eq!(category("4_1"), None);
    }
}
