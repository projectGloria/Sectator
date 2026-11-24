//! The Pirate Bay via its JSON API (`apibay.org/q.php`). Curated.

use async_trait::async_trait;
use serde::Deserialize;
use tokio_util::sync::CancellationToken;

use super::{http, text, Indexer, IndexerConfig, IndexerError, IndexerMeta, SearchContext};
use crate::models::{Category, IndexerHit, SourceType};
use crate::parser::query::SearchQuery;

pub struct ApiBay {
    meta: IndexerMeta,
}

impl ApiBay {
    pub fn new(cfg: &IndexerConfig) -> Self {
        Self {
            meta: IndexerMeta {
                id: cfg.id.clone(),
                name: cfg.name.clone(),
                source_type: SourceType::Curated,
                categories: vec![Category::Movies, Category::Tv, Category::Music, Category::Software, Category::Games, Category::Books, Category::Other],
                base_urls: cfg.base_urls.clone(),
                proxy: cfg.proxy.clone(),
                timeout_ms: cfg.timeout_ms,
            },
        }
    }
}

/// apibay returns every value as a string.
#[derive(Deserialize)]
struct Row {
    id: String,
    name: String,
    info_hash: String,
    #[serde(default)]
    leechers: String,
    #[serde(default)]
    seeders: String,
    #[serde(default)]
    size: String,
    #[serde(default)]
    username: String,
    #[serde(default)]
    added: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    category: String,
}

/// TPB numeric categories (1xx audio, 2xx video, 3xx apps, 4xx games, 5xx adult, 6xx other).
pub fn category(code: u32) -> Option<Category> {
    Some(match code {
        102 => Category::Books, // audio books
        100..=199 => Category::Music,
        201 | 202 | 204 | 207 | 209 | 210 | 211 => Category::Movies,
        205 | 208 | 212 => Category::Tv,
        200..=299 => return None,
        300..=399 => Category::Software,
        400..=499 => Category::Games,
        601 | 602 => Category::Books,
        500..=699 => Category::Other,
        _ => return None,
    })
}

/// Top-level `cat` filter for the request.
fn cat_param(c: Option<Category>) -> &'static str {
    match c {
        Some(Category::Movies | Category::Tv | Category::Anime) => "200",
        Some(Category::Music) => "100",
        Some(Category::Software) => "300",
        Some(Category::Games) => "400",
        Some(Category::Books | Category::Other) => "600",
        None => "0",
    }
}

pub fn parse(body: &str) -> Result<Vec<IndexerHit>, IndexerError> {
    let rows: Vec<Row> = serde_json::from_str(body).map_err(|e| IndexerError::Parse(e.to_string()))?;
    Ok(rows
        .into_iter()
        // "No results returned" placeholder row.
        .filter(|r| r.id != "0" && r.info_hash.bytes().any(|b| b != b'0'))
        .map(|r| IndexerHit {
            title: text::html_unescape(&r.name),
            info_hash: r.info_hash,
            size: r.size.parse().unwrap_or(0),
            seeders: r.seeders.parse().ok(),
            leechers: r.leechers.parse().ok(),
            completed: None,
            uploaded_at: r.added.parse().ok().filter(|t: &i64| *t > 0),
            verified_uploader: matches!(r.status.as_str(), "vip" | "trusted" | "moderator" | "supermod" | "admin"),
            uploader: Some(r.username).filter(|u| !u.is_empty()),
            category: r.category.parse().ok().and_then(category),
            trackers: Vec::new(),
            files: Vec::new(),
            details_url: Some(format!("https://thepiratebay.org/description.php?id={}", r.id)),
        })
        .collect())
}

#[async_trait]
impl Indexer for ApiBay {
    fn meta(&self) -> &IndexerMeta {
        &self.meta
    }

    async fn search(&self, ctx: &SearchContext, q: &SearchQuery, cancel: &CancellationToken) -> Result<Vec<IndexerHit>, IndexerError> {
        let terms = q.indexer_text();
        if terms.is_empty() {
            return Ok(Vec::new());
        }
        http::with_mirrors(&self.meta.base_urls, cancel, |base| {
            let url = format!("{}/q.php?q={}&cat={}", text::base(&base), text::enc(&terms), cat_param(q.category));
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
        let hits = parse(include_str!("fixtures/apibay.json")).unwrap();
        assert_eq!(hits.len(), 5);
        let bible = &hits[0];
        assert_eq!(bible.title, "Clinton D., Negus C. Ubuntu Linux Bible 11ed 2025");
        assert_eq!(bible.info_hash, "232CD67EB3FFBD7C37BF9EC3EE887417E5AE1EE6");
        assert_eq!((bible.seeders, bible.leechers), (Some(35), Some(3)));
        assert_eq!(bible.size, 8_554_738);
        assert_eq!(bible.uploaded_at, Some(1_765_526_528));
        assert_eq!(bible.category, Some(Category::Books));
        assert!(bible.verified_uploader);
        assert_eq!(bible.uploader.as_deref(), Some("andryold1"));
        assert_eq!(bible.details_url.as_deref(), Some("https://thepiratebay.org/description.php?id=81462446"));
        assert_eq!(hits[1].category, Some(Category::Software));
        // HTML entities in names are decoded.
        assert!(hits.iter().any(|h| h.title.contains("Ubuntu Linux & Linux Scripting")));
    }

    #[test]
    fn no_results_placeholder_is_dropped() {
        assert!(parse(include_str!("fixtures/apibay_empty.json")).unwrap().is_empty());
        assert!(parse("<html>blocked</html>").is_err());
    }

    #[test]
    fn category_codes() {
        assert_eq!(category(207), Some(Category::Movies));
        assert_eq!(category(208), Some(Category::Tv));
        assert_eq!(category(101), Some(Category::Music));
        assert_eq!(category(401), Some(Category::Games));
        assert_eq!(category(299), None);
    }
}
