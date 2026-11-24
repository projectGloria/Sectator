//! Indexer trait, per-indexer configuration, and the built-in adapters.

pub mod apibay;
pub mod bitsearch;
pub mod bt4g;
pub mod btdig;
pub mod http;
pub mod knaben;
pub mod limetorrents;
pub mod nyaa;
pub mod registry;
pub mod rss;
pub mod text;
pub mod torznab;
pub mod yts;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::models::{Category, IndexerHit, SourceType};
use crate::parser::query::SearchQuery;

/// How an indexer's traffic is routed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "mode", content = "url", rename_all = "lowercase")]
pub enum ProxyOverride {
    /// Use the global proxy (if any).
    #[default]
    Inherit,
    /// Always connect directly, even if a global proxy is set.
    Direct,
    /// Use this proxy URL, e.g. `socks5h://127.0.0.1:9050` for Tor.
    Custom(String),
}

/// User-editable configuration for one indexer instance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexerConfig {
    pub id: String,
    pub name: String,
    /// Adapter kind, e.g. `apibay`, `nyaa`, `knaben`, `bt4g`, `btdig`, `bitsearch`, `limetorrents`, `yts`, `torznab`.
    pub kind: String,
    pub enabled: bool,
    /// Mirrors tried in order; the first healthy one wins.
    pub base_urls: Vec<String>,
    #[serde(default)]
    pub proxy: ProxyOverride,
    /// Per-indexer timeout; falls back to the clearnet/Tor default.
    pub timeout_ms: Option<u64>,
    /// Torznab API key.
    #[serde(default)]
    pub api_key: Option<String>,
}

/// Global network settings applied to every indexer unless overridden.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NetworkSettings {
    pub user_agent: String,
    /// Global proxy, e.g. `http://127.0.0.1:8080` or `socks5h://127.0.0.1:9050`.
    pub proxy: Option<String>,
    /// DNS-over-HTTPS JSON endpoint, e.g. `https://cloudflare-dns.com/dns-query`.
    pub doh_url: Option<String>,
    pub clearnet_timeout_ms: u64,
    pub tor_timeout_ms: u64,
}

impl Default for NetworkSettings {
    fn default() -> Self {
        Self {
            user_agent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/142.0 Safari/537.36".into(),
            proxy: None,
            doh_url: None,
            clearnet_timeout_ms: 5_000,
            tor_timeout_ms: 11_000,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum IndexerError {
    #[error("http: {0}")]
    Http(String),
    #[error("parse: {0}")]
    Parse(String),
    #[error("status {0}")]
    Status(u16),
    #[error("timed out")]
    Timeout,
    #[error("cancelled")]
    Cancelled,
    #[error("config: {0}")]
    Config(String),
}

impl From<reqwest::Error> for IndexerError {
    fn from(e: reqwest::Error) -> Self {
        if e.is_timeout() {
            return Self::Timeout;
        }
        // Strip the URL so API keys never reach the UI or logs.
        Self::Http(e.without_url().to_string())
    }
}

/// Static description of one configured indexer instance.
#[derive(Debug, Clone)]
pub struct IndexerMeta {
    pub id: String,
    pub name: String,
    pub source_type: SourceType,
    pub categories: Vec<Category>,
    pub base_urls: Vec<String>,
    pub proxy: ProxyOverride,
    pub timeout_ms: Option<u64>,
}

/// Everything an adapter needs to run one search.
pub struct SearchContext {
    pub client: reqwest::Client,
}

#[async_trait]
pub trait Indexer: Send + Sync {
    fn meta(&self) -> &IndexerMeta;

    async fn search(
        &self,
        ctx: &SearchContext,
        query: &SearchQuery,
        cancel: &CancellationToken,
    ) -> Result<Vec<IndexerHit>, IndexerError>;
}
