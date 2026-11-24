//! Generic Torznab endpoint (Jackett / Prowlarr / any compliant indexer).

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use super::{http, rss, text, Indexer, IndexerConfig, IndexerError, IndexerMeta, SearchContext};
use crate::engine::infohash;
use crate::models::{Category, IndexerHit, SourceType};
use crate::parser::query::SearchQuery;

pub struct Torznab {
    meta: IndexerMeta,
    api_key: Option<String>,
}

impl Torznab {
    pub fn new(cfg: &IndexerConfig) -> Self {
        Self {
            meta: IndexerMeta {
                id: cfg.id.clone(),
                name: cfg.name.clone(),
                source_type: SourceType::Torznab,
                categories: Category::ALL.to_vec(),
                base_urls: cfg.base_urls.clone(),
                proxy: cfg.proxy.clone(),
                timeout_ms: cfg.timeout_ms,
            },
            api_key: cfg.api_key.clone().filter(|k| !k.trim().is_empty()),
        }
    }
}

/// Newznab category number → our category. Subcategories that matter are checked first.
pub fn category(code: u32) -> Option<Category> {
    Some(match code {
        5070 => Category::Anime,
        3030 => Category::Books,
        4050 => Category::Games,
        1000..=1999 => Category::Games,
        2000..=2999 => Category::Movies,
        3000..=3999 => Category::Music,
        4000..=4999 => Category::Software,
        5000..=5999 => Category::Tv,
        7000..=7999 => Category::Books,
        6000..=6999 | 8000..=8999 => Category::Other,
        _ => return None,
    })
}

/// Our category → request `cat=` list.
fn cat_param(c: Option<Category>) -> Option<&'static str> {
    Some(match c? {
        Category::Movies => "2000",
        Category::Tv => "5000",
        Category::Anime => "5070",
        Category::Music => "3000",
        Category::Software => "4000",
        Category::Games => "1000,4050",
        Category::Books => "7000,3030",
        Category::Other => "8000",
    })
}

pub fn api_url(base: &str, terms: &str, api_key: Option<&str>, cat: Option<Category>) -> String {
    let base = text::base(base);
    let endpoint = if base.ends_with("/api") { base.to_string() } else { format!("{base}/api") };
    let mut url = format!("{endpoint}?t=search&extended=1&q={}", text::enc(terms));
    if let Some(k) = api_key {
        url.push_str("&apikey=");
        url.push_str(&text::enc(k));
    }
    if let Some(c) = cat_param(cat) {
        url.push_str("&cat=");
        url.push_str(c);
    }
    url
}

pub fn parse(xml: &str) -> Result<Vec<IndexerHit>, IndexerError> {
    Ok(rss::parse_items(xml)?
        .into_iter()
        .filter_map(|it| {
            let magnet = it
                .attr("magneturl")
                .or_else(|| it.field("link").filter(|l| l.starts_with("magnet:")))
                .or_else(|| it.enclosure_url.as_deref().filter(|l| l.starts_with("magnet:")))
                .and_then(infohash::parse_magnet);
            let hash = it
                .attr("infohash")
                .map(str::to_string)
                .or_else(|| magnet.as_ref().map(|m| m.info_hash.clone()))?;
            let seeders: Option<u32> = it.attr("seeders").and_then(|s| s.parse().ok());
            let peers: Option<u32> = it.attr("peers").and_then(|s| s.parse().ok());
            let leechers = it
                .attr("leechers")
                .and_then(|s| s.parse().ok())
                .or_else(|| peers.map(|p| p.saturating_sub(seeders.unwrap_or(0))));
            // Most specific category wins (largest code is usually the subcategory).
            let category = it
                .attrs_named("category")
                .filter_map(|c| c.parse::<u32>().ok())
                .filter(|c| *c < 100_000)
                .max()
                .and_then(category);
            let size = it
                .field("size")
                .and_then(|s| s.parse().ok())
                .or_else(|| it.attr("size").and_then(|s| s.parse().ok()))
                .or(it.enclosure_len)
                .unwrap_or(0);
            Some(IndexerHit {
                info_hash: hash,
                title: it.field("title").unwrap_or_default().to_string(),
                size,
                seeders,
                leechers,
                completed: it.attr("grabs").and_then(|s| s.parse().ok()),
                uploaded_at: it.field("pubDate").and_then(text::parse_rfc2822),
                uploader: it.attr("poster").map(str::to_string),
                verified_uploader: false,
                category,
                trackers: magnet.map(|m| m.trackers).unwrap_or_default(),
                files: Vec::new(),
                details_url: it.field("comments").or_else(|| it.field("guid").filter(|g| g.starts_with("http"))).map(str::to_string),
            })
        })
        .collect())
}

#[async_trait]
impl Indexer for Torznab {
    fn meta(&self) -> &IndexerMeta {
        &self.meta
    }

    async fn search(&self, ctx: &SearchContext, q: &SearchQuery, cancel: &CancellationToken) -> Result<Vec<IndexerHit>, IndexerError> {
        let terms = q.indexer_text();
        if terms.is_empty() {
            return Ok(Vec::new());
        }
        http::with_mirrors(&self.meta.base_urls, cancel, |base| {
            let req = ctx.client.get(api_url(&base, &terms, self.api_key.as_deref(), q.category));
            async move { parse(&http::fetch_text(req).await?) }
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shape of a Prowlarr/Jackett `t=search&extended=1` response.
    const FEED: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:atom="http://www.w3.org/2005/Atom" xmlns:torznab="http://torznab.com/schemas/2015/feed">
<channel><title>Prowlarr</title>
<item>
  <title>Some.Show.S01E02.1080p.WEB.h264-GRP</title>
  <guid>https://tracker.example/details/42</guid>
  <comments>https://tracker.example/details/42</comments>
  <pubDate>Mon, 23 Nov 2026 13:08:47 +0000</pubDate>
  <size>1610612736</size>
  <link>http://127.0.0.1:9696/1/download?apikey=x&amp;link=abc</link>
  <enclosure url="http://127.0.0.1:9696/1/download?apikey=x&amp;link=abc" length="1610612736" type="application/x-bittorrent"/>
  <torznab:attr name="category" value="5000"/>
  <torznab:attr name="category" value="5040"/>
  <torznab:attr name="seeders" value="40"/>
  <torznab:attr name="peers" value="55"/>
  <torznab:attr name="grabs" value="300"/>
  <torznab:attr name="infohash" value="2C603FB78071E4933CA5C95BD1A77060053FF988"/>
</item>
<item>
  <title>Anime Thing</title>
  <size>100</size>
  <torznab:attr name="category" value="5070"/>
  <torznab:attr name="category" value="105070"/>
  <torznab:attr name="magneturl" value="magnet:?xt=urn:btih:444d6b7665bf616065d4c3793642bc41c17e767e&amp;tr=udp%3A%2F%2Ftracker.example%3A80"/>
</item>
<item><title>no hash, dropped</title></item>
</channel></rss>"#;

    #[test]
    fn parses_extended_feed() {
        let hits = parse(FEED).unwrap();
        assert_eq!(hits.len(), 2);
        let h = &hits[0];
        assert_eq!(h.info_hash, "2C603FB78071E4933CA5C95BD1A77060053FF988");
        assert_eq!((h.seeders, h.leechers, h.completed), (Some(40), Some(15), Some(300)));
        assert_eq!(h.size, 1_610_612_736);
        assert_eq!(h.category, Some(Category::Tv));
        assert_eq!(h.details_url.as_deref(), Some("https://tracker.example/details/42"));
        assert!(h.uploaded_at.is_some());
        let a = &hits[1];
        assert_eq!(a.info_hash, "444d6b7665bf616065d4c3793642bc41c17e767e");
        assert_eq!(a.category, Some(Category::Anime));
        assert_eq!(a.trackers, vec!["udp://tracker.example:80"]);
    }

    #[test]
    fn api_errors_surface() {
        let e = parse(r#"<error code="100" description="Invalid API Key"/>"#);
        assert!(matches!(e, Err(IndexerError::Config(_))));
    }

    #[test]
    fn builds_urls() {
        assert_eq!(
            api_url("http://127.0.0.1:9696/1/", "a b", Some("k"), Some(Category::Tv)),
            "http://127.0.0.1:9696/1/api?t=search&extended=1&q=a+b&apikey=k&cat=5000"
        );
        assert_eq!(api_url("http://h/api", "x", None, None), "http://h/api?t=search&extended=1&q=x");
        assert_eq!(category(2040), Some(Category::Movies));
        assert_eq!(category(4050), Some(Category::Games));
    }
}
