//! Core data models shared by indexers, the engine, storage, and IPC.
//! Everything that crosses the IPC boundary is `Serialize` with camelCase fields.

use serde::{Deserialize, Serialize};

/// Which class of source produced a hit. Drives trust weighting and proxy defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceType {
    Curated,
    Dht,
    Torznab,
}

impl SourceType {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "curated" => Some(Self::Curated),
            "dht" => Some(Self::Dht),
            "torznab" | "prowlarr" | "jackett" => Some(Self::Torznab),
            _ => None,
        }
    }

    /// Base trust weight used by the ranking engine.
    pub fn trust(self) -> f64 {
        match self {
            Self::Torznab => 1.0,
            Self::Curated => 0.9,
            Self::Dht => 0.55,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Movies,
    Tv,
    Software,
    Games,
    Music,
    Books,
    Anime,
    Other,
}

impl Category {
    pub const ALL: [Category; 8] = [
        Self::Movies,
        Self::Tv,
        Self::Software,
        Self::Games,
        Self::Music,
        Self::Books,
        Self::Anime,
        Self::Other,
    ];

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.to_ascii_lowercase().as_str() {
            "movies" | "movie" | "film" | "films" => Self::Movies,
            "tv" | "series" | "show" | "shows" => Self::Tv,
            "software" | "apps" | "app" | "programs" => Self::Software,
            "games" | "game" => Self::Games,
            "music" | "audio" => Self::Music,
            "books" | "book" | "ebooks" | "ebook" => Self::Books,
            "anime" => Self::Anime,
            "other" | "misc" => Self::Other,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Movies => "movies",
            Self::Tv => "tv",
            Self::Software => "software",
            Self::Games => "games",
            Self::Music => "music",
            Self::Books => "books",
            Self::Anime => "anime",
            Self::Other => "other",
        }
    }

    /// Video categories where tiny archives and executables are suspicious.
    pub fn is_video(self) -> bool {
        matches!(self, Self::Movies | Self::Tv | Self::Anime)
    }

    /// Categories where executables inside the payload are a red flag.
    pub fn is_media(self) -> bool {
        matches!(self, Self::Movies | Self::Tv | Self::Anime | Self::Music | Self::Books)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TorrentFile {
    pub path: String,
    pub size: u64,
}

/// Structured tags extracted from a raw release name.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedMeta {
    pub resolution: Option<String>,
    pub source: Option<String>,
    pub codec: Option<String>,
    pub dynamic_range: Vec<String>,
    pub audio: Vec<String>,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub complete: bool,
    pub year: Option<u32>,
    pub group: Option<String>,
    pub version: Option<String>,
    pub os: Option<String>,
    pub arch: Option<String>,
    pub format: Vec<String>,
}

/// One result exactly as a single indexer reported it, before merging.
#[derive(Debug, Clone, Default)]
pub struct IndexerHit {
    pub info_hash: String,
    pub title: String,
    pub size: u64,
    pub seeders: Option<u32>,
    pub leechers: Option<u32>,
    pub completed: Option<u32>,
    /// Unix seconds.
    pub uploaded_at: Option<i64>,
    pub uploader: Option<String>,
    pub verified_uploader: bool,
    pub category: Option<Category>,
    pub trackers: Vec<String>,
    pub files: Vec<TorrentFile>,
    pub details_url: Option<String>,
}

/// Which indexer reported a merged torrent, with its own counts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceHit {
    pub indexer_id: String,
    pub indexer_name: String,
    pub source_type: SourceType,
    pub details_url: Option<String>,
    pub seeders: Option<u32>,
    pub leechers: Option<u32>,
}

/// Live swarm counts pulled from trackers (BEP-15 / HTTP scrape).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LivePeers {
    pub seeders: u32,
    pub leechers: u32,
    pub completed: u32,
    /// Unix seconds.
    pub scraped_at: i64,
    /// How many trackers answered.
    pub trackers_ok: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SpamFlag {
    TinyForCategory,
    ExecutableInMedia,
    DeadSwarm,
    KeywordStuffed,
    SpoofedSeeders,
    NoSeedData,
}

/// A deduplicated torrent, merged across every indexer that reported its infohash.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TorrentResult {
    pub info_hash: String,
    pub title: String,
    pub size: u64,
    pub seeders: Option<u32>,
    pub leechers: Option<u32>,
    pub completed: Option<u32>,
    pub live: Option<LivePeers>,
    pub uploaded_at: Option<i64>,
    pub uploader: Option<String>,
    pub verified_uploader: bool,
    pub category: Option<Category>,
    pub trackers: Vec<String>,
    pub magnet: String,
    pub files: Vec<TorrentFile>,
    pub sources: Vec<SourceHit>,
    pub meta: ParsedMeta,
    pub score: f64,
    pub flags: Vec<SpamFlag>,
    /// True when a `file:` pattern or query term matched an inner file path.
    pub file_match: bool,
}

impl TorrentResult {
    /// Best-known seeders: live scrape beats indexer-reported counts.
    pub fn best_seeders(&self) -> Option<u32> {
        self.live.map(|l| l.seeders).or(self.seeders)
    }

    pub fn best_leechers(&self) -> Option<u32> {
        self.live.map(|l| l.leechers).or(self.leechers)
    }

    pub fn has_source_type(&self, t: SourceType) -> bool {
        self.sources.iter().any(|s| s.source_type == t)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IndexerState {
    Searching,
    Ok,
    Timeout,
    Error,
    Cancelled,
    /// Circuit breaker open: skipped without a request.
    Skipped,
}

/// Per-indexer telemetry reported to the UI for every search.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexerStatus {
    pub indexer_id: String,
    pub name: String,
    pub source_type: SourceType,
    pub state: IndexerState,
    pub latency_ms: Option<u64>,
    pub count: usize,
    pub error: Option<String>,
    pub proxied: bool,
}
