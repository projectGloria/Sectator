//! BT4G via its RSS search feed. External DHT index (no seeders; the scraper fills them).

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use super::{http, rss, text, Indexer, IndexerConfig, IndexerError, IndexerMeta, SearchContext};
use crate::engine::infohash;
use crate::models::{Category, IndexerHit, SourceType};
use crate::parser::query::{parse_size, SearchQuery};

pub struct Bt4g {
    meta: IndexerMeta,
}

impl Bt4g {
    pub fn new(cfg: &IndexerConfig) -> Self {
        Self {
            meta: IndexerMeta {
                id: cfg.id.clone(),
                name: cfg.name.clone(),
                source_type: SourceType::Dht,
                categories: Category::ALL.to_vec(),
                base_urls: cfg.base_urls.clone(),
                proxy: cfg.proxy.clone(),
                timeout_ms: cfg.timeout_ms,
            },
        }
    }
}

/// BT4G's coarse file-type label (`Movie`, `Audio`, `Doc`, `App`, `Archive file`).
/// Video is too coarse to split into movies/TV, so leave that to the release parser.
pub fn category(label: &str) -> Option<Category> {
    match label.trim().to_ascii_lowercase().as_str() {
        "audio" => Some(Category::Music),
        "doc" => Some(Category::Books),
        "app" => Some(Category::Software),
        _ => None,
    }
}

pub fn parse(xml: &str) -> Result<Vec<IndexerHit>, IndexerError> {
    Ok(rss::parse_items(xml)?
        .into_iter()
        .filter_map(|it| {
            let magnet = it.field("link").and_then(infohash::parse_magnet)?;
            // description: "title<br>8.52GB<br>Movie|Archive file<br>hash"
            let desc = it.field("description").unwrap_or_default();
            let parts: Vec<&str> = desc.split("<br>").map(str::trim).collect();
            let size = parts.get(1).and_then(|s| parse_size(s)).unwrap_or(0);
            let category = parts.get(2).and_then(|s| s.split('|').find_map(category));
            Some(IndexerHit {
                info_hash: magnet.info_hash,
                title: it.field("title").map(str::to_string).or(magnet.name).unwrap_or_default(),
                size,
                seeders: None,
                leechers: None,
                completed: None,
                uploaded_at: it.field("pubDate").and_then(text::parse_rfc2822),
                uploader: None,
                verified_uploader: false,
                category,
                trackers: magnet.trackers,
                files: Vec::new(),
                details_url: it.field("guid").map(str::to_string),
            })
        })
        .collect())
}

#[async_trait]
impl Indexer for Bt4g {
    fn meta(&self) -> &IndexerMeta {
        &self.meta
    }

    async fn search(&self, ctx: &SearchContext, q: &SearchQuery, cancel: &CancellationToken) -> Result<Vec<IndexerHit>, IndexerError> {
        let terms = q.indexer_text();
        if terms.is_empty() {
            return Ok(Vec::new());
        }
        http::with_mirrors(&self.meta.base_urls, cancel, |base| {
            let url = format!("{}/search?q={}&page=rss", text::base(&base), text::enc(&terms));
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
        let hits = parse(include_str!("fixtures/bt4g.xml")).unwrap();
        assert_eq!(hits.len(), 3);
        let h = &hits[0];
        assert_eq!(h.title, "Udemy - The Perfect Nginx Server - Ubuntu (24.04) Edition 1080 2026-10");
        assert_eq!(h.info_hash, "6649923bc479a3048dcc0001de72376f0b42d731");
        assert_eq!(h.size, parse_size("8.52GB").unwrap());
        assert_eq!(h.seeders, None);
        assert!(h.uploaded_at.is_some());
        assert!(h.trackers.iter().any(|t| t.contains("opentrackr")));
        assert!(h.details_url.is_some());
        assert_eq!(hits[1].info_hash, "2c603fb78071e4933ca5c95bd1a77060053ff988");
        assert_eq!(hits[2].info_hash, "98723ce67687c2db85eb2542be12fe008a82abdb");
    }

    #[test]
    fn categories() {
        assert_eq!(category("Audio"), Some(Category::Music));
        assert_eq!(category("Movie"), None);
    }
}
