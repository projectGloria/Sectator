//! Knaben aggregator via its JSON API (`POST api.knaben.org/v1`). Curated.

use async_trait::async_trait;
use serde::Deserialize;
use tokio_util::sync::CancellationToken;

use super::{http, text, Indexer, IndexerConfig, IndexerError, IndexerMeta, SearchContext};
use crate::engine::infohash;
use crate::models::{Category, IndexerHit, SourceType};
use crate::parser::query::SearchQuery;

pub struct Knaben {
    meta: IndexerMeta,
}

impl Knaben {
    pub fn new(cfg: &IndexerConfig) -> Self {
        Self {
            meta: IndexerMeta {
                id: cfg.id.clone(),
                name: cfg.name.clone(),
                source_type: SourceType::Curated,
                categories: Category::ALL.to_vec(),
                base_urls: cfg.base_urls.clone(),
                proxy: cfg.proxy.clone(),
                timeout_ms: cfg.timeout_ms,
            },
        }
    }
}

#[derive(Deserialize)]
struct Response {
    #[serde(default)]
    hits: Vec<Hit>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Hit {
    #[serde(default)]
    bytes: Option<u64>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    date: Option<String>,
    #[serde(default)]
    details: Option<String>,
    #[serde(default)]
    grabs: Option<u32>,
    #[serde(default)]
    hash: Option<String>,
    #[serde(default)]
    magnet_url: Option<String>,
    #[serde(default)]
    peers: Option<u32>,
    #[serde(default)]
    seeders: Option<u32>,
    #[serde(default)]
    title: String,
}

/// Knaben category labels look like `PC / Unix`, `TV / HD`, `Anime / Subbed`.
pub fn category(label: &str) -> Option<Category> {
    let l = label.to_ascii_lowercase();
    let has = |w: &str| l.split(|c: char| !c.is_ascii_alphanumeric()).any(|t| t == w);
    Some(if has("anime") {
        Category::Anime
    } else if has("tv") {
        Category::Tv
    } else if has("movies") || has("movie") {
        Category::Movies
    } else if has("games") || has("game") || has("console") {
        Category::Games
    } else if has("books") || has("ebooks") || has("comics") || has("audiobook") {
        Category::Books
    } else if has("audio") || has("music") {
        Category::Music
    } else if has("pc") || has("software") || has("apps") || has("mobile") {
        Category::Software
    } else if has("xxx") || has("other") {
        Category::Other
    } else {
        return None;
    })
}

pub fn request_body(terms: &str, size: u32) -> serde_json::Value {
    serde_json::json!({
        "search_type": "100%",
        "search_field": "title",
        "query": terms,
        "order_by": "seeders",
        "order_direction": "desc",
        "size": size,
        "hide_unsafe": true,
        "hide_xxx": true,
    })
}

pub fn parse(body: &str) -> Result<Vec<IndexerHit>, IndexerError> {
    let resp: Response = serde_json::from_str(body).map_err(|e| IndexerError::Parse(e.to_string()))?;
    Ok(resp
        .hits
        .into_iter()
        .filter_map(|h| {
            let magnet = h.magnet_url.as_deref().and_then(infohash::parse_magnet);
            let hash = h.hash.clone().or_else(|| magnet.as_ref().map(|m| m.info_hash.clone()))?;
            Some(IndexerHit {
                info_hash: hash,
                title: h.title,
                size: h.bytes.unwrap_or(0),
                seeders: h.seeders,
                leechers: h.peers,
                completed: h.grabs,
                uploaded_at: h.date.as_deref().and_then(text::parse_rfc3339),
                uploader: None,
                verified_uploader: false,
                category: h.category.as_deref().and_then(category),
                trackers: magnet.map(|m| m.trackers).unwrap_or_default(),
                files: Vec::new(),
                details_url: h.details,
            })
        })
        .collect())
}

#[async_trait]
impl Indexer for Knaben {
    fn meta(&self) -> &IndexerMeta {
        &self.meta
    }

    async fn search(&self, ctx: &SearchContext, q: &SearchQuery, cancel: &CancellationToken) -> Result<Vec<IndexerHit>, IndexerError> {
        let terms = q.indexer_text();
        if terms.is_empty() {
            return Ok(Vec::new());
        }
        let body = request_body(&terms, 100);
        http::with_mirrors(&self.meta.base_urls, cancel, |base| {
            // The base URL is the API endpoint itself.
            let req = ctx.client.post(base).json(&body);
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
        let hits = parse(include_str!("fixtures/knaben.json")).unwrap();
        assert_eq!(hits.len(), 3);
        let h = &hits[0];
        assert_eq!(h.title, "ubuntu-26.04-desktop-amd64.iso");
        assert_eq!(h.info_hash, "DAFC8C076CA2F3ED376EEAE7C76A0D6BE2415C45");
        assert_eq!(h.size, 6_517_612_871);
        assert_eq!((h.seeders, h.leechers, h.completed), (Some(116), Some(98), Some(167)));
        assert_eq!(h.uploaded_at, Some(1_777_131_300));
        assert_eq!(h.category, Some(Category::Software));
        assert!(h.trackers.len() > 5);
        assert!(h.trackers.iter().any(|t| t.contains("opentrackr")));
        assert_eq!(hits[2].category, Some(Category::Books));
    }

    #[test]
    fn category_labels() {
        assert_eq!(category("TV / HD"), Some(Category::Tv));
        assert_eq!(category("Anime / Subbed"), Some(Category::Anime));
        assert_eq!(category("Movies / UHD"), Some(Category::Movies));
        assert_eq!(category("Audio / Lossless"), Some(Category::Music));
        assert_eq!(category("PC / Games"), Some(Category::Games));
        assert_eq!(category("???"), None);
    }

    #[test]
    fn body_shape() {
        let b = request_body("ubuntu", 50);
        assert_eq!(b["query"], "ubuntu");
        assert_eq!(b["size"], 50);
        assert_eq!(b["hide_unsafe"], true);
    }
}
