//! Shared HTTP plumbing: a cached client factory (User-Agent, per-indexer proxy routing,
//! optional DNS-over-HTTPS), mirror failover, and checked GET/POST helpers.

use std::collections::HashMap;
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use tokio_util::sync::CancellationToken;

use super::{IndexerError, NetworkSettings, ProxyOverride};

/// The proxy URL an indexer should use, after applying its override to the global setting.
pub fn resolve_proxy(net: &NetworkSettings, ov: &ProxyOverride) -> Option<String> {
    match ov {
        ProxyOverride::Inherit => net.proxy.clone().filter(|p| !p.trim().is_empty()),
        ProxyOverride::Direct => None,
        ProxyOverride::Custom(p) => Some(p.clone()).filter(|p| !p.trim().is_empty()),
    }
}

/// SOCKS proxies (Tor and friends) are slow; they get the longer timeout.
pub fn is_socks(proxy: &str) -> bool {
    proxy.trim().to_ascii_lowercase().starts_with("socks")
}

/// Effective per-request timeout for an indexer.
pub fn timeout_for(net: &NetworkSettings, proxy: Option<&str>, explicit: Option<u64>) -> Duration {
    let ms = explicit.unwrap_or(if proxy.is_some_and(is_socks) { net.tor_timeout_ms } else { net.clearnet_timeout_ms });
    Duration::from_millis(ms.max(250))
}

/// Builds and caches one `reqwest::Client` per (User-Agent, proxy, DoH) combination,
/// so connection pools are reused across searches.
#[derive(Default)]
pub struct ClientCache {
    clients: Mutex<HashMap<String, reqwest::Client>>,
}

impl ClientCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, net: &NetworkSettings, proxy: Option<&str>) -> Result<reqwest::Client, IndexerError> {
        // DoH only applies to direct connections: a proxy resolves names itself
        // (socks5h:// even does it remotely, which is the point for Tor).
        let doh = if proxy.is_none() { net.doh_url.clone().filter(|u| !u.trim().is_empty()) } else { None };
        let key = format!("{}|{}|{}", net.user_agent, proxy.unwrap_or(""), doh.as_deref().unwrap_or(""));
        let mut map = self.clients.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(c) = map.get(&key) {
            return Ok(c.clone());
        }
        let client = build_client(&net.user_agent, proxy, doh.as_deref())?;
        map.insert(key, client.clone());
        Ok(client)
    }

    pub fn clear(&self) {
        self.clients.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }
}

pub fn build_client(user_agent: &str, proxy: Option<&str>, doh: Option<&str>) -> Result<reqwest::Client, IndexerError> {
    let mut b = reqwest::Client::builder()
        .user_agent(user_agent)
        .connect_timeout(Duration::from_secs(8))
        // Hard ceiling; the engine enforces the real per-indexer budget.
        .timeout(Duration::from_secs(30))
        .pool_idle_timeout(Duration::from_secs(90))
        .redirect(reqwest::redirect::Policy::limited(5));
    match proxy {
        Some(p) => {
            let px = reqwest::Proxy::all(p).map_err(|e| IndexerError::Config(format!("bad proxy: {e}")))?;
            b = b.proxy(px);
        }
        None => b = b.no_proxy(),
    }
    if let Some(endpoint) = doh {
        b = b.dns_resolver(DohResolver::new(endpoint, user_agent)?);
    }
    b.build().map_err(|e| IndexerError::Config(e.to_string()))
}

/// DNS-over-HTTPS (JSON API, as served by Cloudflare/Google/Quad9) with a small TTL cache.
/// Falls back to the system resolver if the DoH endpoint fails.
#[derive(Clone)]
pub struct DohResolver {
    inner: Arc<DohInner>,
}

struct DohInner {
    endpoint: String,
    client: reqwest::Client,
    cache: Mutex<HashMap<String, (Instant, Vec<IpAddr>)>>,
}

impl DohResolver {
    pub fn new(endpoint: &str, user_agent: &str) -> Result<Self, IndexerError> {
        // The DoH endpoint itself is resolved by the system resolver (bootstrap).
        let client = reqwest::Client::builder()
            .user_agent(user_agent)
            .no_proxy()
            .timeout(Duration::from_secs(4))
            .build()
            .map_err(|e| IndexerError::Config(e.to_string()))?;
        Ok(Self {
            inner: Arc::new(DohInner { endpoint: endpoint.trim().to_string(), client, cache: Mutex::new(HashMap::new()) }),
        })
    }

    async fn query(&self, host: &str, rtype: &str) -> Option<(Vec<IpAddr>, u64)> {
        let resp = self
            .inner
            .client
            .get(format!("{}?name={}&type={}", self.inner.endpoint, super::text::enc(host), rtype))
            .header(reqwest::header::ACCEPT, "application/dns-json")
            .send()
            .await
            .ok()?;
        if !resp.status().is_success() {
            return None;
        }
        let body = resp.text().await.ok()?;
        let parsed = parse_doh_json(&body);
        (!parsed.0.is_empty()).then_some(parsed)
    }

    async fn lookup(&self, host: String) -> Vec<IpAddr> {
        if let Ok(ip) = host.parse::<IpAddr>() {
            return vec![ip];
        }
        if let Some((at, ips)) = self.inner.cache.lock().unwrap_or_else(|e| e.into_inner()).get(&host) {
            if Instant::now() < *at {
                return ips.clone();
            }
        }
        let (a, aaaa) = tokio::join!(self.query(&host, "A"), self.query(&host, "AAAA"));
        let mut ips = Vec::new();
        let mut ttl = u64::MAX;
        for (list, t) in [a, aaaa].into_iter().flatten() {
            ips.extend(list);
            ttl = ttl.min(t);
        }
        if !ips.is_empty() {
            let ttl = ttl.clamp(30, 3600);
            self.inner
                .cache
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(host, (Instant::now() + Duration::from_secs(ttl), ips.clone()));
        }
        ips
    }
}

impl Resolve for DohResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let this = self.clone();
        let host = name.as_str().to_string();
        Box::pin(async move {
            let mut ips = this.lookup(host.clone()).await;
            if ips.is_empty() {
                ips = tokio::net::lookup_host((host.as_str(), 0)).await?.map(|a| a.ip()).collect();
            }
            let addrs: Addrs = Box::new(ips.into_iter().map(|ip| SocketAddr::new(ip, 0)));
            Ok(addrs)
        })
    }
}

/// Parse a DoH JSON answer: A (1) and AAAA (28) records plus the smallest TTL.
pub fn parse_doh_json(body: &str) -> (Vec<IpAddr>, u64) {
    let v: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => return (Vec::new(), 0),
    };
    let mut ips = Vec::new();
    let mut ttl = u64::MAX;
    for ans in v.get("Answer").and_then(|a| a.as_array()).into_iter().flatten() {
        let t = ans.get("type").and_then(|t| t.as_u64());
        if !matches!(t, Some(1) | Some(28)) {
            continue;
        }
        if let Some(ip) = ans.get("data").and_then(|d| d.as_str()).and_then(|d| d.parse::<IpAddr>().ok()) {
            ips.push(ip);
            ttl = ttl.min(ans.get("TTL").and_then(|t| t.as_u64()).unwrap_or(300));
        }
    }
    (ips, if ttl == u64::MAX { 0 } else { ttl })
}

/// Try `f` against each mirror in order and return the first success. Every failure
/// (network, bad status, or an unparseable block page) moves on to the next mirror.
pub async fn with_mirrors<T, F, Fut>(bases: &[String], cancel: &CancellationToken, mut f: F) -> Result<T, IndexerError>
where
    F: FnMut(String) -> Fut,
    Fut: Future<Output = Result<T, IndexerError>>,
{
    let mut last = IndexerError::Config("no base URL configured".into());
    for base in bases.iter().map(|b| b.trim()).filter(|b| !b.is_empty()) {
        if cancel.is_cancelled() {
            return Err(IndexerError::Cancelled);
        }
        let attempt = tokio::select! {
            _ = cancel.cancelled() => return Err(IndexerError::Cancelled),
            r = f(base.trim_end_matches('/').to_string()) => r,
        };
        match attempt {
            Ok(v) => return Ok(v),
            Err(IndexerError::Cancelled) => return Err(IndexerError::Cancelled),
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// Send a request and return the body, mapping non-2xx to `Status`.
pub async fn fetch_text(req: reqwest::RequestBuilder) -> Result<String, IndexerError> {
    let resp = req.send().await?;
    let status = resp.status();
    if !status.is_success() {
        return Err(IndexerError::Status(status.as_u16()));
    }
    Ok(resp.text().await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn proxy_resolution() {
        let mut net = NetworkSettings::default();
        assert_eq!(resolve_proxy(&net, &ProxyOverride::Inherit), None);
        net.proxy = Some("http://127.0.0.1:8080".into());
        assert_eq!(resolve_proxy(&net, &ProxyOverride::Inherit).as_deref(), Some("http://127.0.0.1:8080"));
        assert_eq!(resolve_proxy(&net, &ProxyOverride::Direct), None);
        let tor = ProxyOverride::Custom("socks5h://127.0.0.1:9050".into());
        assert_eq!(resolve_proxy(&net, &tor).as_deref(), Some("socks5h://127.0.0.1:9050"));
    }

    #[test]
    fn tor_gets_the_long_timeout() {
        let net = NetworkSettings::default();
        assert_eq!(timeout_for(&net, None, None), Duration::from_millis(5_000));
        assert_eq!(timeout_for(&net, Some("socks5h://127.0.0.1:9050"), None), Duration::from_millis(11_000));
        assert_eq!(timeout_for(&net, Some("http://p:1"), None), Duration::from_millis(5_000));
        assert_eq!(timeout_for(&net, None, Some(2_000)), Duration::from_millis(2_000));
    }

    #[test]
    fn client_cache_reuses_and_validates() {
        let cache = ClientCache::new();
        let net = NetworkSettings::default();
        cache.get(&net, None).unwrap();
        cache.get(&net, Some("socks5h://127.0.0.1:9050")).unwrap();
        cache.get(&net, None).unwrap();
        assert_eq!(cache.clients.lock().unwrap().len(), 2);
        assert!(matches!(cache.get(&net, Some("::not a url::")), Err(IndexerError::Config(_))));
    }

    #[test]
    fn doh_answer_parsing() {
        let body = r#"{"Status":0,"Answer":[
            {"name":"nyaa.si","type":5,"TTL":60,"data":"alias.example."},
            {"name":"nyaa.si","type":1,"TTL":120,"data":"104.21.4.1"},
            {"name":"nyaa.si","type":28,"TTL":90,"data":"2606:4700::1"}]}"#;
        let (ips, ttl) = parse_doh_json(body);
        assert_eq!(ips, vec!["104.21.4.1".parse::<IpAddr>().unwrap(), "2606:4700::1".parse().unwrap()]);
        assert_eq!(ttl, 90);
        assert!(parse_doh_json("{\"Status\":3}").0.is_empty());
        assert!(parse_doh_json("not json").0.is_empty());
    }

    #[tokio::test]
    async fn mirrors_fail_over_in_order() {
        let calls = AtomicUsize::new(0);
        let bases = vec!["https://dead.example/".to_string(), "https://alive.example".to_string(), "https://never.example".into()];
        let cancel = CancellationToken::new();
        let r = with_mirrors(&bases, &cancel, |b| {
            calls.fetch_add(1, Ordering::SeqCst);
            async move {
                if b.contains("dead") {
                    Err(IndexerError::Status(503))
                } else {
                    Ok(b)
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(r, "https://alive.example");
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn mirrors_report_last_error_and_honour_cancel() {
        let bases = vec!["a".to_string(), "b".to_string()];
        let cancel = CancellationToken::new();
        let r: Result<(), _> = with_mirrors(&bases, &cancel, |_| async { Err(IndexerError::Status(502)) }).await;
        assert!(matches!(r, Err(IndexerError::Status(502))));
        cancel.cancel();
        let r: Result<(), _> = with_mirrors(&bases, &cancel, |_| async { Ok(()) }).await;
        assert!(matches!(r, Err(IndexerError::Cancelled)));
        let none: Result<(), _> = with_mirrors(&[], &CancellationToken::new(), |_| async { Ok(()) }).await;
        assert!(matches!(none, Err(IndexerError::Config(_))));
    }
}
