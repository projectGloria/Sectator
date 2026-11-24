//! LimeTorrents via its HTML search page (`reqwest` + `scraper`). The infohash comes
//! from each row's `itorrents.net/torrent/<HASH>.torrent` link; dates are relative.

use async_trait::async_trait;
use once_cell::sync::Lazy;
use regex::Regex;
use scraper::{ElementRef, Html, Selector};
use tokio_util::sync::CancellationToken;

use super::{http, text, Indexer, IndexerConfig, IndexerError, IndexerMeta, SearchContext};
use crate::engine::infohash;
use crate::models::{Category, IndexerHit, SourceType};
use crate::parser::query::{parse_size, SearchQuery};

pub struct LimeTorrents {
    meta: IndexerMeta,
}

impl LimeTorrents {
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

fn sel(s: &str) -> Selector {
    Selector::parse(s).expect("static selector")
}

static ROW: Lazy<Selector> = Lazy::new(|| sel("table.table2 tr"));
static LINKS: Lazy<Selector> = Lazy::new(|| sel(".tt-name a"));
static CELLS: Lazy<Selector> = Lazy::new(|| sel("td"));
static HASH: Lazy<Regex> = Lazy::new(|| Regex::new(r"/torrent/([0-9A-Fa-f]{40})\.torrent").expect("valid regex"));
static AGO: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)^(\d+)\s+(minute|hour|day|week|month|year)s?\s+ago").expect("valid regex"));

fn text_of(e: ElementRef<'_>) -> String {
    e.text().collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `2 days ago - in Movies` → (approximate unix time, category). `1 Year+` has no usable date.
fn added(cell: &str, now: i64) -> (Option<i64>, Option<Category>) {
    let (when, cat) = cell.split_once(" - in ").unwrap_or((cell, ""));
    let when = when.trim();
    let at = if when.eq_ignore_ascii_case("yesterday") {
        Some(now - 86_400)
    } else if when.eq_ignore_ascii_case("today") || when.eq_ignore_ascii_case("just now") {
        Some(now)
    } else {
        AGO.captures(when).and_then(|c| {
            let n: i64 = c[1].parse().ok()?;
            let unit = match c[2].to_ascii_lowercase().as_str() {
                "minute" => 60,
                "hour" => 3_600,
                "day" => 86_400,
                "week" => 7 * 86_400,
                "month" => 30 * 86_400,
                _ => 365 * 86_400,
            };
            Some(now - n * unit)
        })
    };
    let cat = match cat.trim().to_ascii_lowercase().as_str() {
        "movies" => Some(Category::Movies),
        "tv shows" | "tv" => Some(Category::Tv),
        "music" => Some(Category::Music),
        "games" => Some(Category::Games),
        "applications" | "software" => Some(Category::Software),
        "anime" => Some(Category::Anime),
        "e-books" | "ebooks" | "books" => Some(Category::Books),
        "" => None,
        _ => Some(Category::Other),
    };
    (at, cat)
}

pub fn parse(html: &str, base: &str, now: i64) -> Result<Vec<IndexerHit>, IndexerError> {
    let doc = Html::parse_document(html);
    let lower = html.to_ascii_lowercase();
    if lower.contains("just a moment") || (!lower.contains("limetorrents") && !lower.contains("table2")) {
        return Err(IndexerError::Parse("unexpected page (no results table)".into()));
    }
    Ok(doc
        .select(&ROW)
        .filter_map(|row| {
            let links: Vec<ElementRef<'_>> = row.select(&LINKS).collect();
            let hash = links
                .iter()
                .filter_map(|a| a.value().attr("href"))
                .find_map(|h| HASH.captures(h).and_then(|c| infohash::normalize(&c[1])))?;
            let title_a = links.iter().find(|a| a.value().attr("href").is_some_and(|h| h.starts_with('/')))?;
            let cells: Vec<String> = row.select(&CELLS).map(text_of).collect();
            let (uploaded_at, category) = cells.get(1).map(|c| added(c, now)).unwrap_or((None, None));
            let href = title_a.value().attr("href").unwrap_or_default();
            Some(IndexerHit {
                info_hash: hash,
                title: text_of(*title_a),
                size: cells.get(2).and_then(|s| parse_size(s)).unwrap_or(0),
                seeders: cells.get(3).and_then(|s| s.replace(',', "").parse().ok()),
                leechers: cells.get(4).and_then(|s| s.replace(',', "").parse().ok()),
                completed: None,
                uploaded_at,
                uploader: None,
                verified_uploader: false,
                category,
                trackers: Vec::new(),
                files: Vec::new(),
                details_url: Some(format!("{}{href}", text::base(base))),
            })
        })
        .collect())
}

/// LimeTorrents search paths use dashes between words: `/search/all/fargo-2014/seeds/1/`.
fn search_path(terms: &str, cat: Option<Category>) -> String {
    let words: Vec<String> = terms.split_whitespace().map(text::enc).collect();
    let section = match cat {
        Some(Category::Movies) => "movies",
        Some(Category::Tv) => "tv",
        Some(Category::Music) => "music",
        Some(Category::Games) => "games",
        Some(Category::Software) => "applications",
        Some(Category::Anime) => "anime",
        _ => "all",
    };
    format!("/search/{section}/{}/seeds/1/", words.join("-"))
}

#[async_trait]
impl Indexer for LimeTorrents {
    fn meta(&self) -> &IndexerMeta {
        &self.meta
    }

    async fn search(&self, ctx: &SearchContext, q: &SearchQuery, cancel: &CancellationToken) -> Result<Vec<IndexerHit>, IndexerError> {
        let terms = q.indexer_text();
        if terms.is_empty() {
            return Ok(Vec::new());
        }
        let path = search_path(&terms, q.category);
        http::with_mirrors(&self.meta.base_urls, cancel, |base| {
            let req = ctx.client.get(format!("{}{path}", text::base(&base)));
            async move { parse(&http::fetch_text(req).await?, &base, chrono::Utc::now().timestamp()) }
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_795_000_000;

    #[test]
    fn parses_live_fixture() {
        let hits = parse(include_str!("fixtures/limetorrents.html"), "https://www.limetorrents.lol", NOW).unwrap();
        assert_eq!(hits.len(), 3, "the header row is skipped");
        let h = &hits[1];
        assert_eq!(h.title, "Fargo 1996 BDRip D");
        assert_eq!(h.info_hash, "1cdea2529d7fac197e42247adb105708bff15d10");
        assert_eq!(h.size, parse_size("1.46 GB").unwrap());
        assert_eq!((h.seeders, h.leechers), (Some(7), Some(15)));
        assert_eq!(h.uploaded_at, Some(NOW - 86_400));
        assert_eq!(h.category, Some(Category::Movies));
        assert_eq!(h.details_url.as_deref(), Some("https://www.limetorrents.lol/Fargo-1996-BDRip-D-torrent-20188753.html"));
        assert_eq!(hits[0].uploaded_at, Some(NOW - 2 * 86_400));
        assert_eq!((hits[2].uploaded_at, hits[2].category), (None, Some(Category::Tv)));
    }

    #[test]
    fn relative_dates_and_paths() {
        assert_eq!(added("15 minutes ago", NOW).0, Some(NOW - 900));
        assert_eq!(added("3 Hours ago - in Music", NOW), (Some(NOW - 10_800), Some(Category::Music)));
        assert_eq!(search_path("fargo 2014", None), "/search/all/fargo-2014/seeds/1/");
        assert_eq!(search_path("c++ book", Some(Category::Software)), "/search/applications/c%2B%2B-book/seeds/1/");
        assert!(parse("<html><title>Just a moment...</title></html>", "https://x", NOW).is_err());
    }
}
