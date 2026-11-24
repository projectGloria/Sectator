//! Live tracker scraping (BEP-15 UDP with HTTP scrape fallback).
//! `scrape_many` groups hashes by tracker, scrapes trackers concurrently under a
//! global deadline, and keeps the best counts any tracker reported per hash.

pub mod http;
pub mod udp;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use crate::models::LivePeers;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Swarm {
    pub seeders: u32,
    pub leechers: u32,
    pub completed: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum ScrapeError {
    #[error("io: {0}")]
    Io(String),
    #[error("timed out")]
    Timeout,
    #[error("protocol: {0}")]
    Protocol(String),
    #[error("tracker: {0}")]
    Tracker(String),
}

#[derive(Debug, Clone)]
pub struct ScrapeOptions {
    /// UDP can't go through an HTTP/SOCKS proxy here, so it's off when one is set.
    pub allow_udp: bool,
    pub max_trackers_per_hash: usize,
    pub concurrency: usize,
    pub per_request: Duration,
    pub deadline: Duration,
}

impl Default for ScrapeOptions {
    fn default() -> Self {
        Self { allow_udp: true, max_trackers_per_hash: 4, concurrency: 16, per_request: Duration::from_millis(1500), deadline: Duration::from_secs(5) }
    }
}

/// Tracker → hashes to ask it about, honouring the per-hash tracker cap and
/// skipping trackers this configuration can't reach.
pub fn plan(targets: &[(String, Vec<String>)], opts: &ScrapeOptions) -> Vec<(String, Vec<String>)> {
    let mut by_tracker: HashMap<String, Vec<String>> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for (hash, trackers) in targets {
        let usable = trackers.iter().filter(|t| {
            if t.starts_with("udp://") {
                opts.allow_udp && udp::host_port(t).is_some()
            } else {
                http::scrape_url(t).is_some()
            }
        });
        for t in usable.take(opts.max_trackers_per_hash) {
            let e = by_tracker.entry(t.clone()).or_insert_with(|| {
                order.push(t.clone());
                Vec::new()
            });
            if !e.contains(hash) {
                e.push(hash.clone());
            }
        }
    }
    order.into_iter().map(|t| { let h = by_tracker.remove(&t).unwrap_or_default(); (t, h) }).collect()
}

async fn scrape_tracker(client: reqwest::Client, tracker: String, hashes: Vec<String>, per_request: Duration) -> Vec<(String, Swarm)> {
    let mut out = Vec::new();
    for chunk in hashes.chunks(udp::MAX_HASHES) {
        if tracker.starts_with("udp://") {
            if let Ok(swarms) = udp::scrape(&tracker, chunk, per_request).await {
                out.extend(chunk.iter().cloned().zip(swarms));
            } else {
                break; // tracker unreachable: don't burn the deadline on more chunks
            }
        } else if let Some(base) = http::scrape_url(&tracker) {
            let raw: Vec<[u8; 20]> = chunk.iter().filter_map(|h| udp::hex20(h)).collect();
            let resp = client.get(http::request_url(&base, &raw)).timeout(per_request).send().await;
            let Ok(resp) = resp else { break };
            if !resp.status().is_success() {
                break;
            }
            let Ok(body) = resp.bytes().await else { break };
            let Ok(map) = http::parse_response(&body) else { break };
            for h in chunk {
                if let Some(s) = udp::hex20(h).and_then(|k| map.get(&k)) {
                    out.push((h.clone(), *s));
                }
            }
        }
    }
    out
}

/// Fold per-tracker answers into one `LivePeers` per hash (max of each count).
pub fn combine(answers: impl IntoIterator<Item = (String, Swarm)>, now: i64) -> HashMap<String, LivePeers> {
    let mut out: HashMap<String, LivePeers> = HashMap::new();
    for (hash, s) in answers {
        let e = out.entry(hash).or_insert(LivePeers { seeders: 0, leechers: 0, completed: 0, scraped_at: now, trackers_ok: 0 });
        e.seeders = e.seeders.max(s.seeders);
        e.leechers = e.leechers.max(s.leechers);
        e.completed = e.completed.max(s.completed);
        e.trackers_ok += 1;
    }
    out
}

/// Scrape `(lowercase hex hash, trackers)` targets. Returns whatever arrived before
/// the deadline or cancellation; hashes no tracker answered for are absent.
pub async fn scrape_many(
    client: reqwest::Client,
    targets: &[(String, Vec<String>)],
    opts: &ScrapeOptions,
    cancel: &CancellationToken,
) -> HashMap<String, LivePeers> {
    let jobs = plan(targets, opts);
    let sem = Arc::new(Semaphore::new(opts.concurrency.max(1)));
    let mut set = JoinSet::new();
    for (tracker, hashes) in jobs {
        let sem = sem.clone();
        let client = client.clone();
        let per = opts.per_request;
        set.spawn(async move {
            let _permit = sem.acquire_owned().await.ok()?;
            Some(scrape_tracker(client, tracker, hashes, per).await)
        });
    }
    let deadline = Instant::now() + opts.deadline;
    let mut answers = Vec::new();
    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = tokio::time::sleep_until(deadline) => break,
            next = set.join_next() => match next {
                Some(Ok(Some(batch))) => answers.extend(batch),
                Some(_) => {}
                None => break,
            },
        }
    }
    set.abort_all();
    combine(answers, chrono::Utc::now().timestamp())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_groups_and_caps() {
        let t = |s: &str| s.to_string();
        let targets = vec![
            (t("a"), vec![t("udp://x:1/announce"), t("http://h/announce"), t("wss://w/announce"), t("udp://y:2/announce")]),
            (t("b"), vec![t("udp://x:1/announce")]),
        ];
        let opts = ScrapeOptions { max_trackers_per_hash: 2, ..Default::default() };
        let p = plan(&targets, &opts);
        assert_eq!(p, vec![(t("udp://x:1/announce"), vec![t("a"), t("b")]), (t("http://h/announce"), vec![t("a")])]);

        let no_udp = ScrapeOptions { allow_udp: false, ..Default::default() };
        assert_eq!(plan(&targets, &no_udp), vec![(t("http://h/announce"), vec![t("a")])]);
    }

    #[test]
    fn combine_takes_maxima() {
        let s = |a, b, c| Swarm { seeders: a, leechers: b, completed: c };
        let m = combine(vec![("h".into(), s(5, 9, 1)), ("h".into(), s(7, 2, 3))], 42);
        assert_eq!(m["h"], LivePeers { seeders: 7, leechers: 9, completed: 3, scraped_at: 42, trackers_ok: 2 });
    }

    #[tokio::test]
    async fn deadline_bounds_unreachable_trackers() {
        let client = reqwest::Client::new();
        // TEST-NET-1 address: never answers.
        let targets = vec![("2c603fb78071e4933ca5c95bd1a77060053ff988".to_string(), vec!["udp://192.0.2.1:6969/announce".to_string()])];
        let opts = ScrapeOptions { deadline: Duration::from_millis(300), ..Default::default() };
        let started = std::time::Instant::now();
        let got = scrape_many(client, &targets, &opts, &CancellationToken::new()).await;
        assert!(got.is_empty());
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
