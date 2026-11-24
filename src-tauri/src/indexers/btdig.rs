//! BTDig via its HTML search page (`reqwest` + `scraper`). External DHT index;
//! also reachable as a Tor onion mirror. Pages are localised, so dates are ignored.

use async_trait::async_trait;
use once_cell::sync::Lazy;
use scraper::{ElementRef, Html, Selector};
use tokio_util::sync::CancellationToken;

use super::{http, text, Indexer, IndexerConfig, IndexerError, IndexerMeta, SearchContext};
use crate::engine::infohash;
use crate::models::{Category, IndexerHit, SourceType, TorrentFile};
use crate::parser::query::{parse_size, SearchQuery};

pub const ONION: &str = "http://btdigggink2pdqzqrik3blmqemsbntpzwxottujilcdjfz56jumzfsyd.onion";

pub struct BtDig {
    meta: IndexerMeta,
}

impl BtDig {
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

static RESULT: Lazy<Selector> = Lazy::new(|| sel(".one_result"));
static NAME: Lazy<Selector> = Lazy::new(|| sel(".torrent_name a"));
static SIZE: Lazy<Selector> = Lazy::new(|| sel(".torrent_size"));
static MAGNET: Lazy<Selector> = Lazy::new(|| sel(".torrent_magnet a"));
static EXCERPT: Lazy<Selector> = Lazy::new(|| sel(".torrent_excerpt"));

fn text_of(e: ElementRef<'_>) -> String {
    e.text().collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `padding-inline-start:3em` → 3.
fn depth(e: ElementRef<'_>) -> usize {
    e.value()
        .attr("style")
        .and_then(|s| s.split("padding-inline-start:").nth(1))
        .and_then(|s| s.trim().split("em").next())
        .and_then(|n| n.trim().parse::<f32>().ok())
        .map(|n| n.max(0.0) as usize)
        .unwrap_or(0)
}

/// Rebuild the (partial) file list from the excerpt's indented folder/file rows.
fn files(excerpt: ElementRef<'_>) -> Vec<TorrentFile> {
    let mut out: Vec<TorrentFile> = Vec::new();
    let mut folders: Vec<String> = Vec::new();
    let mut pending: Option<usize> = None; // index in `out` waiting for its size span
    for child in excerpt.children().filter_map(ElementRef::wrap) {
        let el = child.value();
        let classes: Vec<&str> = el.classes().collect();
        match el.name() {
            "div" if classes.contains(&"fa-folder-open") => {
                let d = depth(child);
                folders.truncate(d);
                folders.push(text_of(child));
                pending = None;
            }
            "div" if classes.iter().any(|c| c.starts_with("fa-file")) => {
                let d = depth(child);
                let mut parts: Vec<&str> = folders.iter().take(d).map(String::as_str).collect();
                let name = text_of(child);
                parts.push(&name);
                out.push(TorrentFile { path: parts.join("/"), size: 0 });
                pending = Some(out.len() - 1);
            }
            "span" => {
                if let Some(i) = pending.take() {
                    out[i].size = parse_size(&text_of(child)).unwrap_or(0);
                }
            }
            "br" => {}
            _ => pending = None, // e.g. the "N hidden files" link
        }
    }
    out
}

pub fn parse(html: &str) -> Result<Vec<IndexerHit>, IndexerError> {
    let doc = Html::parse_document(html);
    let rows: Vec<ElementRef<'_>> = doc.select(&RESULT).collect();
    if rows.is_empty() {
        // A real "no results" page still has the search form; a block page has neither.
        let lower = html.to_ascii_lowercase();
        if !lower.contains("btdig") && !lower.contains("<form") {
            return Err(IndexerError::Parse("unexpected page (no results container)".into()));
        }
    }
    Ok(rows
        .into_iter()
        .filter_map(|r| {
            let a = r.select(&NAME).next()?;
            let href = a.value().attr("href").unwrap_or_default();
            let magnet = r.select(&MAGNET).next().and_then(|m| m.value().attr("href")).and_then(infohash::parse_magnet);
            let from_href = href
                .split("://")
                .nth(1)
                .and_then(|rest| rest.split('/').nth(1))
                .and_then(infohash::normalize);
            let hash = from_href.or_else(|| magnet.as_ref().map(|m| m.info_hash.clone()))?;
            let file_list = r.select(&EXCERPT).next().map(files).unwrap_or_default();
            Some(IndexerHit {
                info_hash: hash,
                title: text_of(a),
                size: r.select(&SIZE).next().and_then(|s| parse_size(&text_of(s))).unwrap_or(0),
                seeders: None,
                leechers: None,
                completed: None,
                uploaded_at: None,
                uploader: None,
                verified_uploader: false,
                category: None,
                trackers: magnet.map(|m| m.trackers).unwrap_or_default(),
                files: file_list,
                details_url: Some(href.to_string()).filter(|h| !h.is_empty()),
            })
        })
        .collect())
}

#[async_trait]
impl Indexer for BtDig {
    fn meta(&self) -> &IndexerMeta {
        &self.meta
    }

    async fn search(&self, ctx: &SearchContext, q: &SearchQuery, cancel: &CancellationToken) -> Result<Vec<IndexerHit>, IndexerError> {
        let terms = q.indexer_text();
        if terms.is_empty() {
            return Ok(Vec::new());
        }
        http::with_mirrors(&self.meta.base_urls, cancel, |base| {
            let url = format!("{}/search?q={}&order=0", text::base(&base), text::enc(&terms));
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
        let hits = parse(include_str!("fixtures/btdig.html")).unwrap();
        assert_eq!(hits.len(), 3);
        let h = &hits[0];
        assert_eq!(h.title, "Learning Ubuntu Desktop");
        assert_eq!(h.info_hash, "210cc6f5d15ebeca8f29346ae9eddcc4f52527bf");
        assert_eq!(h.size, parse_size("569.16 MB").unwrap());
        assert!(h.trackers.contains(&"udp://tracker.opentrackr.org:1337/announce".to_string()));
        let mp4 = h.files.iter().find(|f| f.path.ends_with("03 - Native install Install Ubuntu on a PC.mp4")).unwrap();
        assert_eq!(mp4.size, parse_size("18.89 MB").unwrap());
        assert_eq!(mp4.path.matches('/').count(), 3, "three parent folders: {}", mp4.path);
        assert!(mp4.path.starts_with("Learning Ubuntu Desktop/"));

        let ua = &hits[2];
        assert_eq!(ua.title, "UAPack");
        assert_eq!(ua.info_hash, "466262e8672aa035114e8026583c975a70d9f9c1");
        assert_eq!(ua.size, parse_size("2.49 GB").unwrap());
        assert_eq!(ua.files.len(), 4);
        let iso = ua.files.iter().find(|f| f.path == "UAPack/ubuntu-9.04-desktop-i386.iso").unwrap();
        assert_eq!(iso.size, parse_size("698.96 MB").unwrap());
    }

    #[test]
    fn block_pages_are_errors() {
        assert!(parse("<html><body>Just a moment...</body></html>").is_err());
    }
}
