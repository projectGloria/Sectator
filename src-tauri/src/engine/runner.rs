//! Concurrent search runner: fans a query out to every eligible indexer on a
//! `JoinSet`, merges and scores hits as each batch lands, streams changes through
//! a sink, cancels superseded searches, then live-scrapes the top results.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

use serde::Serialize;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::engine::breaker::{Admission, Breaker};
use crate::engine::{merge, merge::Merger, score};
use crate::indexers::{http, Indexer, IndexerError, NetworkSettings, SearchContext};
use crate::models::{IndexerState, IndexerStatus, TorrentResult};
use crate::parser::query::SearchQuery;
use crate::scraper::{self, ScrapeOptions};

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum SearchEvent {
    Started { search_id: u64, query: SearchQuery, indexers: Vec<IndexerStatus> },
    Status { search_id: u64, status: IndexerStatus },
    /// New or changed results (upsert by infohash).
    Results { search_id: u64, results: Vec<TorrentResult> },
    /// Previously sent results that no longer pass the filters.
    Removed { search_id: u64, hashes: Vec<String> },
    Scraping { search_id: u64, count: usize },
    Done { search_id: u64, total: usize, elapsed_ms: u64, cancelled: bool, origin: Origin },
}

/// Where a search's results came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    Network,
    /// Replayed from the local result cache (within the TTL).
    Cache,
    /// Every source failed; results come from the local FTS index.
    Offline,
}

pub type Sink = Arc<dyn Fn(SearchEvent) + Send + Sync>;

/// Local lookup used when the network yields nothing because sources failed.
pub type OfflineSource = Arc<dyn Fn(&SearchQuery) -> Vec<TorrentResult> + Send + Sync>;

/// What a finished search produced, for caching and stats.
#[derive(Debug, Clone)]
pub struct SearchOutcome {
    pub id: u64,
    /// Every merged result, before query filters (the cache re-filters on replay).
    pub results: Vec<TorrentResult>,
    pub statuses: Vec<IndexerStatus>,
    pub cancelled: bool,
    pub origin: Origin,
}

#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub net: NetworkSettings,
    /// How many top results get a live tracker scrape (0 disables scraping).
    pub scrape_top: usize,
    pub scrape: ScrapeOptions,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self { net: NetworkSettings::default(), scrape_top: 60, scrape: ScrapeOptions::default() }
    }
}

pub struct Engine {
    indexers: RwLock<Vec<Arc<dyn Indexer>>>,
    config: RwLock<EngineConfig>,
    clients: http::ClientCache,
    breaker: Breaker,
    current: Mutex<Option<(u64, CancellationToken)>>,
    next_id: AtomicU64,
    offline: RwLock<Option<OfflineSource>>,
}

/// Does this indexer have anything to contribute to the query?
fn eligible(ix: &dyn Indexer, q: &SearchQuery) -> bool {
    let m = ix.meta();
    if !q.sites.is_empty() {
        let (id, name) = (m.id.to_lowercase(), m.name.to_lowercase());
        if !q.sites.iter().any(|s| id.contains(s.as_str()) || name.contains(s.as_str())) {
            return false;
        }
    }
    if !q.source_types.is_empty() && !q.source_types.contains(&m.source_type) {
        return false;
    }
    q.category.is_none_or(|c| m.categories.contains(&c))
}

fn status(ix: &dyn Indexer, state: IndexerState, proxied: bool) -> IndexerStatus {
    let m = ix.meta();
    IndexerStatus {
        indexer_id: m.id.clone(),
        name: m.name.clone(),
        source_type: m.source_type,
        state,
        latency_ms: None,
        count: 0,
        error: None,
        proxied,
    }
}

/// Re-score `hashes` and split them into results to (re)send and ones to retract.
fn refresh(merger: &mut Merger, hashes: &HashSet<String>, q: &SearchQuery, sent: &mut HashSet<String>, now: i64) -> (Vec<TorrentResult>, Vec<String>) {
    let mut upserts = Vec::new();
    let mut removed = Vec::new();
    for h in hashes {
        let Some(r) = merger.get_mut(h) else { continue };
        score::evaluate(r, q, now);
        if score::passes_filters(r, q) {
            sent.insert(h.clone());
            upserts.push(r.clone());
        } else if sent.remove(h) {
            removed.push(h.clone());
        }
    }
    upserts.sort_by(|a, b| b.score.total_cmp(&a.score));
    (upserts, removed)
}

impl Engine {
    pub fn new(indexers: Vec<Arc<dyn Indexer>>, config: EngineConfig) -> Self {
        Self {
            indexers: RwLock::new(indexers),
            config: RwLock::new(config),
            clients: http::ClientCache::new(),
            breaker: Breaker::new(),
            current: Mutex::new(None),
            next_id: AtomicU64::new(1),
            offline: RwLock::new(None),
        }
    }

    pub fn set_offline_source(&self, src: OfflineSource) {
        *self.offline.write().unwrap() = Some(src);
    }

    /// Result-cache key: the network-relevant query text plus the exact set of
    /// indexers it fans out to, so site/cat scoping or a settings change never
    /// replays a narrower or stale result set.
    pub fn cache_key(&self, q: &SearchQuery) -> String {
        let mut ids: Vec<String> = self.indexers.read().unwrap().iter().filter(|ix| eligible(ix.as_ref(), q)).map(|ix| ix.meta().id.clone()).collect();
        ids.sort();
        format!("{}#{}", q.cache_key(), ids.join(","))
    }

    fn begin(&self) -> (u64, CancellationToken) {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let cancel = CancellationToken::new();
        if let Some((_, prev)) = self.current.lock().unwrap().replace((id, cancel.clone())) {
            prev.cancel();
        }
        (id, cancel)
    }

    fn finish(&self, id: u64) {
        // Still the active search: clear the slot so a later `cancel()` is a no-op.
        let mut guard = self.current.lock().unwrap();
        if guard.as_ref().is_some_and(|(cur, _)| *cur == id) {
            *guard = None;
        }
    }

    /// Stream previously stored results as if they had just arrived: re-scored and
    /// re-filtered against this exact query. Cancels any running search.
    pub fn replay(&self, raw: &str, cached: Vec<TorrentResult>, origin: Origin, sink: Sink) -> u64 {
        let started = Instant::now();
        let (id, _) = self.begin();
        let q = SearchQuery::parse(raw);
        sink(SearchEvent::Started { search_id: id, query: q.clone(), indexers: vec![] });
        let shown = filter_ranked(cached, &q, chrono::Utc::now().timestamp());
        let total = shown.len();
        if !shown.is_empty() {
            sink(SearchEvent::Results { search_id: id, results: shown });
        }
        self.finish(id);
        sink(SearchEvent::Done { search_id: id, total, elapsed_ms: started.elapsed().as_millis() as u64, cancelled: false, origin });
        id
    }

    pub fn set_indexers(&self, indexers: Vec<Arc<dyn Indexer>>) {
        *self.indexers.write().unwrap() = indexers;
        self.breaker.reset();
    }

    pub fn set_config(&self, config: EngineConfig) {
        *self.config.write().unwrap() = config;
        self.clients.clear();
    }

    pub fn config(&self) -> EngineConfig {
        self.config.read().unwrap().clone()
    }

    /// Cancel whatever search is running (e.g. the user cleared the bar).
    pub fn cancel(&self) {
        if let Some((_, t)) = self.current.lock().unwrap().take() {
            t.cancel();
        }
    }

    /// Run one search to completion, streaming events to `sink`. Starting a new
    /// search cancels the previous one. Returns the search id.
    pub async fn search(&self, raw: &str, sink: Sink) -> SearchOutcome {
        let (id, cancel) = self.begin();
        let started = Instant::now();
        let q = SearchQuery::parse(raw);
        let cfg = self.config();
        let indexers: Vec<Arc<dyn Indexer>> = self.indexers.read().unwrap().iter().filter(|ix| eligible(ix.as_ref(), &q)).cloned().collect();

        // Admission + initial statuses.
        let mut set = JoinSet::new();
        let mut initial = Vec::new();
        let mut trials: HashSet<String> = HashSet::new();
        let mut info: HashMap<String, (Arc<dyn Indexer>, bool)> = HashMap::new();
        for ix in &indexers {
            let m = ix.meta();
            let proxy = http::resolve_proxy(&cfg.net, &m.proxy);
            let proxied = proxy.is_some();
            info.insert(m.id.clone(), (ix.clone(), proxied));
            if q.indexer_text().is_empty() {
                continue;
            }
            match self.breaker.admit(&m.id) {
                Admission::Skip { retry_in } => {
                    let mut s = status(ix.as_ref(), IndexerState::Skipped, proxied);
                    s.error = Some(format!("circuit open, retry in {}s", retry_in.as_secs()));
                    initial.push(s);
                    continue;
                }
                Admission::Trial => {
                    trials.insert(m.id.clone());
                }
                Admission::Allow => {}
            }
            initial.push(status(ix.as_ref(), IndexerState::Searching, proxied));
            let client = match self.clients.get(&cfg.net, proxy.as_deref()) {
                Ok(c) => c,
                Err(e) => {
                    let last = initial.last_mut().unwrap();
                    last.state = IndexerState::Error;
                    last.error = Some(e.to_string());
                    continue;
                }
            };
            let dur = http::timeout_for(&cfg.net, proxy.as_deref(), m.timeout_ms);
            let (ix, q, cancel) = (ix.clone(), q.clone(), cancel.clone());
            set.spawn(async move {
                let t0 = Instant::now();
                let ctx = SearchContext { client };
                let r = match tokio::time::timeout(dur, ix.search(&ctx, &q, &cancel)).await {
                    Ok(r) => r,
                    Err(_) => Err(IndexerError::Timeout),
                };
                (ix.meta().id.clone(), r, t0.elapsed().as_millis() as u64)
            });
        }
        let mut statuses: Vec<IndexerStatus> = initial.clone();
        sink(SearchEvent::Started { search_id: id, query: q.clone(), indexers: initial });

        // Fan-in: merge each batch as it lands.
        let mut merger = Merger::new();
        let mut sent: HashSet<String> = HashSet::new();
        let now = chrono::Utc::now().timestamp();
        let mut cancelled = false;
        loop {
            let next = tokio::select! {
                _ = cancel.cancelled() => { cancelled = true; break; }
                n = set.join_next() => n,
            };
            let Some(joined) = next else { break };
            let Ok((ix_id, result, latency)) = joined else { continue };
            let Some((ix, proxied)) = info.get(&ix_id).cloned() else { continue };
            let m = ix.meta();
            let mut st = status(ix.as_ref(), IndexerState::Ok, proxied);
            st.latency_ms = Some(latency);
            match result {
                Ok(hits) => {
                    self.breaker.record_success(&m.id);
                    st.count = hits.len();
                    let mut touched = HashSet::new();
                    for hit in hits {
                        if let Some(h) = merger.add(&m.id, &m.name, m.source_type, hit) {
                            touched.insert(h);
                        }
                    }
                    let (up, rm) = refresh(&mut merger, &touched, &q, &mut sent, now);
                    if !rm.is_empty() {
                        sink(SearchEvent::Removed { search_id: id, hashes: rm });
                    }
                    if !up.is_empty() {
                        sink(SearchEvent::Results { search_id: id, results: up });
                    }
                }
                Err(IndexerError::Cancelled) => {
                    self.breaker.record_neutral(&m.id);
                    st.state = IndexerState::Cancelled;
                }
                Err(e) => {
                    self.breaker.record_failure(&m.id);
                    st.state = if matches!(e, IndexerError::Timeout) { IndexerState::Timeout } else { IndexerState::Error };
                    st.error = Some(e.to_string());
                }
            }
            if let Some(slot) = statuses.iter_mut().find(|s| s.indexer_id == st.indexer_id) {
                *slot = st.clone();
            }
            sink(SearchEvent::Status { search_id: id, status: st });
        }
        if cancelled {
            set.abort_all();
            for t in &trials {
                self.breaker.record_neutral(t);
            }
            for s in statuses.iter_mut().filter(|s| s.state == IndexerState::Searching) {
                s.state = IndexerState::Cancelled;
            }
            sink(SearchEvent::Done { search_id: id, total: sent.len(), elapsed_ms: started.elapsed().as_millis() as u64, cancelled: true, origin: Origin::Network });
            return SearchOutcome { id, results: merger.into_results(), statuses, cancelled: true, origin: Origin::Network };
        }

        // Nothing came back and at least one source failed: fall back to the local index.
        let failed = statuses.iter().any(|s| matches!(s.state, IndexerState::Error | IndexerState::Timeout | IndexerState::Skipped));
        let offline = self.offline.read().unwrap().clone();
        if merger.is_empty() && failed && !q.is_empty() {
            if let Some(local) = offline {
                let shown = filter_ranked(local(&q), &q, now);
                let total = shown.len();
                if !shown.is_empty() {
                    sink(SearchEvent::Results { search_id: id, results: shown });
                }
                self.finish(id);
                sink(SearchEvent::Done { search_id: id, total, elapsed_ms: started.elapsed().as_millis() as u64, cancelled: false, origin: Origin::Offline });
                return SearchOutcome { id, results: vec![], statuses, cancelled: false, origin: Origin::Offline };
            }
        }

        // Live scrape: DHT-index results (no seed data) first, then the best-ranked.
        if cfg.scrape_top > 0 && !sent.is_empty() {
            let mut visible: Vec<&TorrentResult> = merger.results().filter(|r| sent.contains(&r.info_hash)).collect();
            visible.sort_by(|a, b| (a.seeders.is_some()).cmp(&b.seeders.is_some()).then(b.score.total_cmp(&a.score)));
            let targets: Vec<(String, Vec<String>)> = visible
                .iter()
                .take(cfg.scrape_top)
                .filter(|r| r.info_hash.len() == 40)
                .map(|r| (r.info_hash.clone(), merge::effective_trackers(r)))
                .collect();
            if !targets.is_empty() {
                sink(SearchEvent::Scraping { search_id: id, count: targets.len() });
                let proxy = cfg.net.proxy.clone();
                let opts = ScrapeOptions { allow_udp: proxy.is_none(), ..cfg.scrape.clone() };
                if let Ok(client) = self.clients.get(&cfg.net, proxy.as_deref()) {
                    let live = scraper::scrape_many(client, &targets, &opts, &cancel).await;
                    if cancel.is_cancelled() {
                        cancelled = true;
                    }
                    let mut touched = HashSet::new();
                    for (h, lp) in live {
                        merger.set_live(&h, lp);
                        touched.insert(h);
                    }
                    let (up, rm) = refresh(&mut merger, &touched, &q, &mut sent, now);
                    if !rm.is_empty() {
                        sink(SearchEvent::Removed { search_id: id, hashes: rm });
                    }
                    if !up.is_empty() {
                        sink(SearchEvent::Results { search_id: id, results: up });
                    }
                }
            }
        }

        self.finish(id);
        sink(SearchEvent::Done { search_id: id, total: sent.len(), elapsed_ms: started.elapsed().as_millis() as u64, cancelled, origin: Origin::Network });
        SearchOutcome { id, results: merger.into_results(), statuses, cancelled, origin: Origin::Network }
    }
}

/// Score, filter and rank stored results against `q`.
fn filter_ranked(results: Vec<TorrentResult>, q: &SearchQuery, now: i64) -> Vec<TorrentResult> {
    let mut out: Vec<TorrentResult> = results
        .into_iter()
        .map(|mut r| {
            score::evaluate(&mut r, q, now);
            r
        })
        .filter(|r| score::passes_filters(r, q))
        .collect();
    score::rank(&mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    use async_trait::async_trait;

    use crate::indexers::{apibay, knaben, nyaa, IndexerMeta, ProxyOverride};
    use crate::models::{Category, IndexerHit, SourceType};

    /// Serves a real captured response through the real adapter parser, after a delay.
    struct FixtureIndexer {
        meta: IndexerMeta,
        delay: Duration,
        body: Result<fn() -> Result<Vec<IndexerHit>, IndexerError>, ()>,
    }

    impl FixtureIndexer {
        fn new(id: &str, st: SourceType, delay_ms: u64, parse: fn() -> Result<Vec<IndexerHit>, IndexerError>) -> Arc<dyn Indexer> {
            Arc::new(Self {
                meta: IndexerMeta {
                    id: id.into(),
                    name: id.into(),
                    source_type: st,
                    categories: Category::ALL.to_vec(),
                    base_urls: vec![],
                    proxy: ProxyOverride::Direct,
                    timeout_ms: Some(400),
                },
                delay: Duration::from_millis(delay_ms),
                body: Ok(parse),
            })
        }
        fn failing(id: &str) -> Arc<dyn Indexer> {
            Arc::new(Self {
                meta: IndexerMeta {
                    id: id.into(),
                    name: id.into(),
                    source_type: SourceType::Curated,
                    categories: Category::ALL.to_vec(),
                    base_urls: vec![],
                    proxy: ProxyOverride::Direct,
                    timeout_ms: Some(400),
                },
                delay: Duration::ZERO,
                body: Err(()),
            })
        }
    }

    #[async_trait]
    impl Indexer for FixtureIndexer {
        fn meta(&self) -> &IndexerMeta {
            &self.meta
        }
        async fn search(&self, _: &SearchContext, _: &SearchQuery, cancel: &CancellationToken) -> Result<Vec<IndexerHit>, IndexerError> {
            tokio::select! {
                _ = cancel.cancelled() => return Err(IndexerError::Cancelled),
                _ = tokio::time::sleep(self.delay) => {}
            }
            match self.body {
                Ok(f) => f(),
                Err(()) => Err(IndexerError::Status(503)),
            }
        }
    }

    fn apibay_fx() -> Result<Vec<IndexerHit>, IndexerError> {
        apibay::parse(include_str!("../indexers/fixtures/apibay.json"))
    }
    fn knaben_fx() -> Result<Vec<IndexerHit>, IndexerError> {
        knaben::parse(include_str!("../indexers/fixtures/knaben.json"))
    }
    fn nyaa_fx() -> Result<Vec<IndexerHit>, IndexerError> {
        nyaa::parse(include_str!("../indexers/fixtures/nyaa.xml"))
    }

    fn collector() -> (Sink, Arc<Mutex<Vec<SearchEvent>>>) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let e2 = events.clone();
        (Arc::new(move |e| e2.lock().unwrap().push(e)), events)
    }

    fn no_scrape() -> EngineConfig {
        EngineConfig { scrape_top: 0, ..Default::default() }
    }

    #[tokio::test]
    async fn streams_batches_and_statuses() {
        let engine = Engine::new(
            vec![
                FixtureIndexer::new("fast", SourceType::Curated, 0, apibay_fx),
                FixtureIndexer::new("slow", SourceType::Curated, 80, knaben_fx),
                FixtureIndexer::new("hang", SourceType::Dht, 5_000, nyaa_fx),
                FixtureIndexer::failing("broken"),
            ],
            no_scrape(),
        );
        let (sink, events) = collector();
        engine.search("ubuntu", sink).await;
        let ev = events.lock().unwrap();

        assert!(matches!(&ev[0], SearchEvent::Started { indexers, .. } if indexers.len() == 4));
        let statuses: HashMap<String, IndexerState> = ev
            .iter()
            .filter_map(|e| match e {
                SearchEvent::Status { status, .. } => Some((status.indexer_id.clone(), status.state)),
                _ => None,
            })
            .collect();
        assert_eq!(statuses["fast"], IndexerState::Ok);
        assert_eq!(statuses["slow"], IndexerState::Ok);
        assert_eq!(statuses["hang"], IndexerState::Timeout, "per-indexer timeout fires");
        assert_eq!(statuses["broken"], IndexerState::Error);

        // The fast batch is streamed before the slow one arrives.
        let batches: Vec<usize> = ev.iter().filter_map(|e| if let SearchEvent::Results { results, .. } = e { Some(results.len()) } else { None }).collect();
        assert!(batches.len() >= 2, "{batches:?}");
        let all: HashSet<String> = ev
            .iter()
            .flat_map(|e| match e {
                SearchEvent::Results { results, .. } => results.iter().map(|r| r.info_hash.clone()).collect(),
                _ => vec![],
            })
            .collect();
        assert!(all.iter().all(|h| h.len() == 40 && h.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())));
        assert!(matches!(ev.last(), Some(SearchEvent::Done { cancelled: false, .. })));
    }

    #[tokio::test]
    async fn new_search_cancels_previous() {
        let engine = Arc::new(Engine::new(vec![FixtureIndexer::new("slow", SourceType::Curated, 300, apibay_fx)], no_scrape()));
        let (s1, e1) = collector();
        let (s2, e2) = collector();
        let first = {
            let engine = engine.clone();
            tokio::spawn(async move { engine.search("ubuntu", s1).await })
        };
        tokio::time::sleep(Duration::from_millis(50)).await;
        engine.search("ubuntu", s2).await;
        first.await.unwrap();
        assert!(matches!(e1.lock().unwrap().last(), Some(SearchEvent::Done { cancelled: true, .. })));
        assert!(!e1.lock().unwrap().iter().any(|e| matches!(e, SearchEvent::Results { .. })));
        assert!(matches!(e2.lock().unwrap().last(), Some(SearchEvent::Done { cancelled: false, total, .. }) if *total > 0));
    }

    #[tokio::test]
    async fn breaker_skips_repeat_failures_and_filters_apply() {
        let engine = Engine::new(vec![FixtureIndexer::failing("broken"), FixtureIndexer::new("tpb", SourceType::Curated, 0, apibay_fx)], no_scrape());
        for _ in 0..crate::engine::breaker::FAILURE_THRESHOLD {
            engine.search("ubuntu", Arc::new(|_| {})).await;
        }
        let (sink, events) = collector();
        engine.search("ubuntu cat:books site:tpb", sink).await;
        let ev = events.lock().unwrap();
        // site: limited the fan-out to tpb only.
        assert!(matches!(&ev[0], SearchEvent::Started { indexers, .. } if indexers.len() == 1 && indexers[0].indexer_id == "tpb"));
        for e in ev.iter() {
            if let SearchEvent::Results { results, .. } = e {
                assert!(results.iter().all(|r| score::effective_category(r) == Some(Category::Books)));
            }
        }

        let (sink, events) = collector();
        engine.search("ubuntu", sink).await;
        let ev = events.lock().unwrap();
        let SearchEvent::Started { indexers, .. } = &ev[0] else { panic!() };
        let broken = indexers.iter().find(|s| s.indexer_id == "broken").unwrap();
        assert_eq!(broken.state, IndexerState::Skipped);
    }

    #[tokio::test]
    async fn outcome_replay_and_offline_fallback() {
        let engine = Engine::new(vec![FixtureIndexer::new("tpb", SourceType::Curated, 0, apibay_fx)], no_scrape());
        let out = engine.search("ubuntu", Arc::new(|_| {})).await;
        assert_eq!(out.origin, Origin::Network);
        assert!(!out.results.is_empty());
        assert_eq!(out.statuses[0].state, IndexerState::Ok);

        // Replay re-applies filters that the cache key ignores.
        let (sink, events) = collector();
        engine.replay("ubuntu cat:books", out.results.clone(), Origin::Cache, sink);
        let ev = events.lock().unwrap();
        assert!(matches!(ev.last(), Some(SearchEvent::Done { origin: Origin::Cache, .. })));
        for e in ev.iter() {
            if let SearchEvent::Results { results, .. } = e {
                assert!(results.iter().all(|r| score::effective_category(r) == Some(Category::Books)));
            }
        }
        assert_ne!(engine.cache_key(&SearchQuery::parse("ubuntu")), engine.cache_key(&SearchQuery::parse("ubuntu site:nyaa")));
        assert_eq!(engine.cache_key(&SearchQuery::parse("ubuntu")), engine.cache_key(&SearchQuery::parse("ubuntu seeds:>5")));

        // Every source failing hands over to the local index.
        let down = Engine::new(vec![FixtureIndexer::failing("broken")], no_scrape());
        let stored = out.results.clone();
        down.set_offline_source(Arc::new(move |_| stored.clone()));
        let (sink, events) = collector();
        let o = down.search("ubuntu", sink).await;
        assert_eq!(o.origin, Origin::Offline);
        let ev = events.lock().unwrap();
        assert!(ev.iter().any(|e| matches!(e, SearchEvent::Results { results, .. } if !results.is_empty())));
        assert!(matches!(ev.last(), Some(SearchEvent::Done { origin: Origin::Offline, total, .. }) if *total > 0));
    }

    /// Live: `by:` finds TPB titles cut short before their encoder. `cargo test -- --ignored live_`.
    #[tokio::test]
    #[ignore]
    async fn live_by_crew_finds_cut_off_titles() {
        // TPB cuts QxR season packs at 64 characters, losing the encoder; their file
        // lists still carry it, so `by:qxr` should find them anyway.
        let ixs: Vec<Arc<dyn Indexer>> = crate::indexers::registry::default_configs()
            .iter()
            .filter(|c| c.enabled && c.id == "apibay")
            .filter_map(|c| crate::indexers::registry::build(c).ok())
            .collect();
        assert_eq!(ixs.len(), 1, "the TPB indexer is a default");
        let engine = Engine::new(ixs, EngineConfig::default());
        let (sink, events) = collector();
        engine.search("fargo by:qxr", sink).await;
        let mut found: Vec<(String, Option<String>)> = Vec::new();
        for e in events.lock().unwrap().iter() {
            if let SearchEvent::Results { results, .. } = e {
                for r in results {
                    if !found.iter().any(|(t, _)| *t == r.title) {
                        found.push((r.title.clone(), r.meta.group.clone()));
                    }
                }
            }
        }
        for (t, g) in &found {
            eprintln!("{g:?}  {t}");
        }
        assert!(found.iter().any(|(t, _)| crate::parser::release::looks_truncated(t)), "a cut-off title matched through its files");
    }

    /// Live network smoke test against the default indexer set: `cargo test -- --ignored live_`.
    #[tokio::test]
    #[ignore]
    async fn live_default_indexers() {
        let ixs: Vec<Arc<dyn Indexer>> = crate::indexers::registry::default_configs()
            .iter()
            .filter(|c| c.enabled)
            .filter_map(|c| crate::indexers::registry::build(c).ok())
            .collect();
        let engine = Engine::new(ixs, EngineConfig::default());
        let (sink, events) = collector();
        engine.search("ubuntu 24.04", sink).await;
        let ev = events.lock().unwrap();
        let mut total = 0;
        let mut live = 0;
        for e in ev.iter() {
            match e {
                SearchEvent::Status { status, .. } => eprintln!("{:<8} {:?} {:?}ms n={} {:?}", status.indexer_id, status.state, status.latency_ms, status.count, status.error),
                SearchEvent::Results { results, .. } => live += results.iter().filter(|r| r.live.is_some()).count(),
                SearchEvent::Done { total: t, elapsed_ms, .. } => {
                    total = *t;
                    eprintln!("done: {t} results in {elapsed_ms}ms");
                }
                _ => {}
            }
        }
        eprintln!("live-scraped upserts: {live}");
        assert!(total > 0);
    }
}
