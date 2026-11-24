//! YTS via its public JSON API (`/api/v2/list_movies.json`). Movies only: each movie
//! carries several torrents (720p/1080p/2160p…), each becoming one hit with a synthesised
//! scene-style title so the release parser picks up resolution, codec and the `YTS` group.

use async_trait::async_trait;
use serde::Deserialize;
use tokio_util::sync::CancellationToken;

use super::{http, text, Indexer, IndexerConfig, IndexerError, IndexerMeta, SearchContext};
use crate::engine::infohash;
use crate::models::{Category, IndexerHit, SourceType};
use crate::parser::query::SearchQuery;

/// The trackers YTS lists for its own magnets.
const TRACKERS: [&str; 4] = [
    "udp://tracker.opentrackr.org:1337/announce",
    "udp://open.stealth.si:80/announce",
    "udp://tracker.torrent.eu.org:451/announce",
    "udp://exodus.desync.com:6969/announce",
];

pub struct Yts {
    meta: IndexerMeta,
}

impl Yts {
    pub fn new(cfg: &IndexerConfig) -> Self {
        Self {
            meta: IndexerMeta {
                id: cfg.id.clone(),
                name: cfg.name.clone(),
                source_type: SourceType::Curated,
                categories: vec![Category::Movies],
                base_urls: cfg.base_urls.clone(),
                proxy: cfg.proxy.clone(),
                timeout_ms: cfg.timeout_ms,
            },
        }
    }
}

#[derive(Deserialize)]
struct Envelope {
    status: String,
    #[serde(default)]
    data: Option<Data>,
}

#[derive(Deserialize)]
struct Data {
    #[serde(default)]
    movies: Vec<Movie>,
}

#[derive(Deserialize)]
struct Movie {
    #[serde(default)]
    url: String,
    title: String,
    #[serde(default)]
    title_english: Option<String>,
    #[serde(default)]
    year: Option<u32>,
    #[serde(default)]
    torrents: Vec<Torrent>,
}

#[derive(Deserialize)]
struct Torrent {
    hash: String,
    #[serde(default)]
    quality: String,
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default)]
    video_codec: Option<String>,
    #[serde(default)]
    bit_depth: Option<String>,
    #[serde(default)]
    audio_channels: Option<String>,
    #[serde(default)]
    seeds: Option<u32>,
    #[serde(default)]
    peers: Option<u32>,
    #[serde(default)]
    size_bytes: Option<u64>,
    #[serde(default)]
    date_uploaded_unix: Option<i64>,
}

/// `Fargo 1996 1080p BluRay x264 5.1-YTS`
fn title(m: &Movie, t: &Torrent) -> String {
    let name = m.title_english.as_deref().filter(|s| !s.trim().is_empty()).unwrap_or(&m.title);
    let mut parts: Vec<String> = vec![text::html_unescape(name.trim())];
    if let Some(y) = m.year.filter(|y| *y > 0) {
        parts.push(y.to_string());
    }
    parts.push(t.quality.clone());
    parts.push(
        match t.kind.to_ascii_lowercase().as_str() {
            "bluray" => "BluRay",
            "web" => "WEBRip",
            other => other,
        }
        .to_string(),
    );
    if let Some(c) = t.video_codec.as_deref() {
        parts.push(c.to_string());
    }
    if let Some(b) = t.bit_depth.as_deref().filter(|b| *b != "8") {
        parts.push(format!("{b}bit"));
    }
    if let Some(a) = t.audio_channels.as_deref() {
        parts.push(a.to_string());
    }
    format!("{}-YTS", parts.into_iter().filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" "))
}

pub fn parse(body: &str) -> Result<Vec<IndexerHit>, IndexerError> {
    let env: Envelope = serde_json::from_str(body).map_err(|e| IndexerError::Parse(e.to_string()))?;
    if env.status != "ok" {
        return Err(IndexerError::Parse(format!("api status `{}`", env.status)));
    }
    let movies = env.data.map(|d| d.movies).unwrap_or_default();
    Ok(movies
        .iter()
        .flat_map(|m| {
            m.torrents.iter().filter_map(move |t| {
                Some(IndexerHit {
                    info_hash: infohash::normalize(&t.hash)?,
                    title: title(m, t),
                    size: t.size_bytes.unwrap_or(0),
                    seeders: t.seeds,
                    leechers: t.peers,
                    completed: None,
                    uploaded_at: t.date_uploaded_unix.filter(|d| *d > 0),
                    uploader: Some("YTS".into()),
                    verified_uploader: true,
                    category: Some(Category::Movies),
                    trackers: TRACKERS.iter().map(|s| s.to_string()).collect(),
                    files: Vec::new(),
                    details_url: Some(m.url.clone()).filter(|u| !u.is_empty()),
                })
            })
        })
        .collect())
}

#[async_trait]
impl Indexer for Yts {
    fn meta(&self) -> &IndexerMeta {
        &self.meta
    }

    async fn search(&self, ctx: &SearchContext, q: &SearchQuery, cancel: &CancellationToken) -> Result<Vec<IndexerHit>, IndexerError> {
        let terms = q.indexer_text();
        if terms.is_empty() || q.category.is_some_and(|c| c != Category::Movies) {
            return Ok(Vec::new());
        }
        http::with_mirrors(&self.meta.base_urls, cancel, |base| {
            let url = format!("{}/api/v2/list_movies.json?query_term={}&limit=50&sort_by=seeds", text::base(&base), text::enc(&terms));
            let req = ctx.client.get(url);
            async move { parse(&http::fetch_text(req).await?) }
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::release;

    #[test]
    fn parses_live_fixture() {
        let hits = parse(include_str!("fixtures/yts.json")).unwrap();
        let fargo: Vec<&IndexerHit> = hits.iter().filter(|h| h.title.starts_with("Fargo 1996")).collect();
        assert_eq!(fargo.len(), 5, "one hit per torrent");
        let uhd = fargo.iter().find(|h| h.info_hash == "967d3ab2ab142834aa12a009eca3b80b30a19cd0").unwrap();
        assert_eq!(uhd.title, "Fargo 1996 2160p WEBRip x265 10bit 5.1-YTS");
        assert_eq!((uhd.seeders, uhd.leechers), (Some(56), Some(20)));
        assert_eq!(uhd.size, 4_692_251_771);
        assert_eq!(uhd.uploaded_at, Some(1_680_175_933));
        assert_eq!(uhd.category, Some(Category::Movies));
        assert!(!uhd.trackers.is_empty());
        let meta = release::parse(&uhd.title);
        assert_eq!(meta.group.as_deref(), Some("YTS"));
        assert_eq!(meta.resolution.as_deref(), Some("2160p"));
        assert_eq!(fargo[0].title, "Fargo 1996 720p BluRay x264 2.0-YTS");
    }

    #[test]
    fn empty_and_bad_payloads() {
        let empty = r#"{"status":"ok","status_message":"Query was successful","data":{"movie_count":0,"limit":20,"page_number":1}}"#;
        assert!(parse(empty).unwrap().is_empty());
        assert!(parse(r#"{"status":"error","status_message":"nope"}"#).is_err());
        assert!(parse("<html>blocked</html>").is_err());
    }
}
