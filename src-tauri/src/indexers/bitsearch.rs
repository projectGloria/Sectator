//! Bitsearch via its HTML search page (`reqwest` + `scraper`). A DHT-crawled index
//! whose result cards carry the magnet (hash + trackers), size, date and swarm counts.

use async_trait::async_trait;
use chrono::NaiveDate;
use once_cell::sync::Lazy;
use scraper::{ElementRef, Html, Selector};
use tokio_util::sync::CancellationToken;

use super::{http, text, Indexer, IndexerConfig, IndexerError, IndexerMeta, SearchContext};
use crate::engine::infohash;
use crate::models::{Category, IndexerHit, SourceType};
use crate::parser::query::{parse_size, SearchQuery};

pub struct Bitsearch {
    meta: IndexerMeta,
}

impl Bitsearch {
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

fn sel(s: &str) -> Selector {
    Selector::parse(s).expect("static selector")
}

static CARD: Lazy<Selector> = Lazy::new(|| sel(".torrent-result-card"));
static TITLE: Lazy<Selector> = Lazy::new(|| sel("h3 a"));
static META: Lazy<Selector> = Lazy::new(|| sel(".torrent-result-meta"));
static META_VALUE: Lazy<Selector> = Lazy::new(|| sel("span.inline-flex > span"));
static COUNT: Lazy<Selector> = Lazy::new(|| sel("span.font-medium"));
static MAGNET: Lazy<Selector> = Lazy::new(|| sel("a[href^='magnet:']"));

fn text_of(e: ElementRef<'_>) -> String {
    e.text().collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Swarm counts; Bitsearch sometimes shows 32-bit overflow garbage (e.g. 4294967306).
fn count(s: &str) -> Option<u32> {
    s.trim().replace(',', "").parse::<u64>().ok().filter(|n| *n < 10_000_000).map(|n| n as u32)
}

/// `4/1/2024` (US month/day/year) → unix seconds at midnight UTC.
fn date(s: &str) -> Option<i64> {
    NaiveDate::parse_from_str(s.trim(), "%m/%d/%Y").ok()?.and_hms_opt(0, 0, 0).map(|d| d.and_utc().timestamp())
}

/// `TV/Episode`, `Movies`, `Games/PC` … → our coarse category.
fn category(s: &str) -> Option<Category> {
    Category::parse(s.split('/').next()?.trim())
}

pub fn parse(html: &str, base: &str) -> Result<Vec<IndexerHit>, IndexerError> {
    let doc = Html::parse_document(html);
    let cards: Vec<ElementRef<'_>> = doc.select(&CARD).collect();
    if cards.is_empty() {
        let lower = html.to_ascii_lowercase();
        if lower.contains("just a moment") || !lower.contains("bitsearch") {
            return Err(IndexerError::Parse("unexpected page (no results container)".into()));
        }
    }
    Ok(cards
        .into_iter()
        .filter_map(|c| {
            let a = c.select(&TITLE).next()?;
            let magnet = c.select(&MAGNET).next().and_then(|m| m.value().attr("href")).and_then(infohash::parse_magnet)?;
            let metas: Vec<ElementRef<'_>> = c.select(&META).collect();
            let info: Vec<String> = metas.first().map(|m| m.select(&META_VALUE).map(text_of).collect()).unwrap_or_default();
            let counts: Vec<String> = metas.get(1).map(|m| m.select(&COUNT).map(text_of).collect()).unwrap_or_default();
            let href = a.value().attr("href").unwrap_or_default();
            Some(IndexerHit {
                info_hash: magnet.info_hash,
                title: text_of(a),
                size: info.get(1).and_then(|s| parse_size(s)).unwrap_or(0),
                seeders: counts.first().and_then(|s| count(s)),
                leechers: counts.get(1).and_then(|s| count(s)),
                completed: counts.get(2).and_then(|s| count(s)),
                uploaded_at: info.get(2).and_then(|s| date(s)),
                uploader: None,
                verified_uploader: false,
                category: info.first().and_then(|s| category(s)),
                trackers: magnet.trackers,
                files: Vec::new(),
                details_url: href.starts_with('/').then(|| format!("{}{href}", text::base(base))),
            })
        })
        .collect())
}

#[async_trait]
impl Indexer for Bitsearch {
    fn meta(&self) -> &IndexerMeta {
        &self.meta
    }

    async fn search(&self, ctx: &SearchContext, q: &SearchQuery, cancel: &CancellationToken) -> Result<Vec<IndexerHit>, IndexerError> {
        let terms = q.indexer_text();
        if terms.is_empty() {
            return Ok(Vec::new());
        }
        http::with_mirrors(&self.meta.base_urls, cancel, |base| {
            let url = format!("{}/search?q={}&sort=seeders", text::base(&base), text::enc(&terms));
            let req = ctx.client.get(url);
            async move { parse(&http::fetch_text(req).await?, &base) }
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_live_fixture() {
        let hits = parse(include_str!("fixtures/bitsearch.html"), "https://bitsearch.to/").unwrap();
        assert_eq!(hits.len(), 3);
        let h = &hits[1];
        assert_eq!(h.title, "Fargo.S05E06.1080p.HEVC.x265-MeGusta[EZTVx.to].mkv");
        assert_eq!(h.info_hash, "a07304e2de2942458aff2f0040e05371c17853c1");
        assert_eq!(h.size, parse_size("352.47 MB").unwrap());
        assert_eq!((h.seeders, h.leechers, h.completed), (Some(585), Some(407), Some(10969)));
        assert_eq!(h.uploaded_at, date("12/20/2023"));
        assert_eq!(h.category, Some(Category::Tv));
        assert!(h.trackers.contains(&"udp://tracker.opentrackr.org:1337/announce".to_string()));
        assert_eq!(h.details_url.as_deref(), Some("https://bitsearch.to/torrent/65830af6442ae872f808552d"));
        // Overflowed seeder counts are dropped rather than trusted.
        assert_eq!((hits[0].seeders, hits[0].leechers), (None, Some(125)));
    }

    #[test]
    fn dates_and_block_pages() {
        assert_eq!(date("4/1/2024"), Some(1_711_929_600));
        assert_eq!(date("13/40/2024"), None);
        assert!(parse("<html><title>Just a moment...</title></html>", "https://bitsearch.to").is_err());
        assert!(parse("<html><title>Bitsearch</title><body>No results</body></html>", "https://bitsearch.to").unwrap().is_empty());
    }
}
