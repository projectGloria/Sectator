//! Deduplication: hits from every indexer are keyed by normalised infohash and merged
//! into one `TorrentResult` (titles, counts, trackers, file lists, reporting sources).

use std::collections::HashMap;

use crate::engine::infohash;
use crate::models::{IndexerHit, LivePeers, SourceHit, SourceType, TorrentResult};
use crate::parser::release;

/// Well-known open trackers injected into trackerless magnets (DHT-index results
/// often carry none) so clients can find peers immediately.
pub const DEFAULT_TRACKERS: &[&str] = &[
    "udp://tracker.opentrackr.org:1337/announce",
    "udp://open.stealth.si:80/announce",
    "udp://tracker.torrent.eu.org:451/announce",
    "udp://exodus.desync.com:6969/announce",
    "udp://open.demonii.com:1337/announce",
    "udp://explodie.org:6969/announce",
    "udp://tracker.openbittorrent.com:6969/announce",
];

/// Canonical tracker URL (lowercased scheme/host, no trailing slash), or `None` if unusable.
pub fn normalize_tracker(raw: &str) -> Option<String> {
    let u = url::Url::parse(raw.trim()).ok()?;
    if !matches!(u.scheme(), "udp" | "http" | "https" | "wss") || u.host_str().is_none_or(str::is_empty) {
        return None;
    }
    // Non-special schemes (udp) keep host case in `url`, so lowercase it explicitly.
    let host = u.host_str()?.to_ascii_lowercase();
    let port = u.port().map(|p| format!(":{p}")).unwrap_or_default();
    let query = u.query().map(|q| format!("?{q}")).unwrap_or_default();
    let mut s = format!("{}://{host}{port}{}{query}", u.scheme(), u.path());
    while s.ends_with('/') {
        s.pop();
    }
    Some(s)
}

/// Trackers to announce/scrape against: the reported ones, or the defaults when none.
pub fn effective_trackers(r: &TorrentResult) -> Vec<String> {
    if r.trackers.is_empty() {
        DEFAULT_TRACKERS.iter().map(|s| s.to_string()).collect()
    } else {
        r.trackers.clone()
    }
}

struct Entry {
    result: TorrentResult,
    title_trust: f64,
}

#[derive(Default)]
pub struct Merger {
    entries: HashMap<String, Entry>,
    order: Vec<String>,
}

impl Merger {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    pub fn get(&self, hash: &str) -> Option<&TorrentResult> {
        self.entries.get(hash).map(|e| &e.result)
    }

    pub fn get_mut(&mut self, hash: &str) -> Option<&mut TorrentResult> {
        self.entries.get_mut(hash).map(|e| &mut e.result)
    }

    /// All merged results in first-seen order.
    pub fn results(&self) -> impl Iterator<Item = &TorrentResult> {
        self.order.iter().filter_map(|h| self.get(h))
    }

    pub fn into_results(mut self) -> Vec<TorrentResult> {
        self.order.iter().filter_map(|h| self.entries.remove(h)).map(|e| e.result).collect()
    }

    pub fn set_live(&mut self, hash: &str, live: LivePeers) {
        if let Some(r) = self.get_mut(hash) {
            r.live = Some(live);
        }
    }

    /// Merge one hit. Returns the normalised hash it landed under, or `None` if the
    /// hit carried no usable infohash (it is dropped: we never invent identities).
    pub fn add(&mut self, indexer_id: &str, indexer_name: &str, source_type: SourceType, hit: IndexerHit) -> Option<String> {
        let hash = infohash::normalize(&hit.info_hash)?;
        let trust = source_type.trust();
        let source = SourceHit {
            indexer_id: indexer_id.to_string(),
            indexer_name: indexer_name.to_string(),
            source_type,
            details_url: hit.details_url.clone(),
            seeders: hit.seeders,
            leechers: hit.leechers,
        };
        let trackers: Vec<String> = hit.trackers.iter().filter_map(|t| normalize_tracker(t)).collect();

        match self.entries.get_mut(&hash) {
            None => {
                let title = hit.title.trim().to_string();
                let mut r = TorrentResult {
                    info_hash: hash.clone(),
                    meta: release::parse(&title),
                    title,
                    size: hit.size,
                    seeders: hit.seeders,
                    leechers: hit.leechers,
                    completed: hit.completed,
                    live: None,
                    uploaded_at: hit.uploaded_at,
                    uploader: hit.uploader,
                    verified_uploader: hit.verified_uploader,
                    category: hit.category,
                    trackers: dedup(trackers),
                    magnet: String::new(),
                    files: hit.files,
                    sources: vec![source],
                    score: 0.0,
                    flags: Vec::new(),
                    file_match: false,
                };
                fill_group(&mut r);
                r.magnet = magnet_for(&r);
                self.entries.insert(hash.clone(), Entry { result: r, title_trust: trust });
                self.order.push(hash.clone());
            }
            Some(e) => {
                let r = &mut e.result;
                let title = hit.title.trim();
                if !title.is_empty() && (trust > e.title_trust || r.title.is_empty()) {
                    r.title = title.to_string();
                    r.meta = release::parse(title);
                    e.title_trust = trust;
                }
                if r.size == 0 {
                    r.size = hit.size;
                }
                r.seeders = max_opt(r.seeders, hit.seeders);
                r.leechers = max_opt(r.leechers, hit.leechers);
                r.completed = max_opt(r.completed, hit.completed);
                r.uploaded_at = match (r.uploaded_at, hit.uploaded_at) {
                    (Some(a), Some(b)) => Some(a.min(b)),
                    (a, b) => a.or(b),
                };
                if r.uploader.is_none() {
                    r.uploader = hit.uploader;
                }
                r.verified_uploader |= hit.verified_uploader;
                if r.category.is_none() {
                    r.category = hit.category;
                }
                for t in trackers {
                    if !r.trackers.contains(&t) {
                        r.trackers.push(t);
                    }
                }
                if hit.files.len() > r.files.len() {
                    r.files = hit.files;
                }
                match r.sources.iter_mut().find(|s| s.indexer_id == source.indexer_id) {
                    Some(s) => *s = source,
                    None => r.sources.push(source),
                }
                fill_group(r);
                r.magnet = magnet_for(r);
            }
        }
        Some(hash)
    }
}

/// Cut-off titles lose their group; recover it from the file names when we have them.
fn fill_group(r: &mut TorrentResult) {
    if r.meta.group.is_none() {
        r.meta.group = release::group_for(&r.title, &r.files);
    }
}

fn magnet_for(r: &TorrentResult) -> String {
    infohash::build_magnet(&r.info_hash, &r.title, &effective_trackers(r))
}

fn max_opt(a: Option<u32>, b: Option<u32>) -> Option<u32> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.max(y)),
        (x, y) => x.or(y),
    }
}

fn dedup(v: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(v.len());
    for t in v {
        if !out.contains(&t) {
            out.push(t);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Category, TorrentFile};

    const HEX: &str = "dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c";
    const B32: &str = "3WBFL3G4PSSV7MF37AJSHWDQMLNR63I4";

    fn hit(hash: &str, title: &str) -> IndexerHit {
        IndexerHit { info_hash: hash.into(), title: title.into(), ..Default::default() }
    }

    #[test]
    fn merges_hex_and_base32_spellings() {
        let mut m = Merger::new();
        let mut a = hit(&HEX.to_uppercase(), "Big Buck Bunny 1080p");
        a.seeders = Some(10);
        a.trackers = vec!["udp://Tracker.Example.org:1337/announce".into()];
        let mut b = hit(B32, "big.buck.bunny");
        b.seeders = Some(25);
        b.leechers = Some(3);
        b.trackers = vec!["udp://tracker.example.org:1337/announce".into(), "http://t2.example.org/announce/".into()];
        b.files = vec![TorrentFile { path: "bbb.mkv".into(), size: 100 }];
        assert_eq!(m.add("apibay", "ThePirateBay", SourceType::Curated, a).as_deref(), Some(HEX));
        assert_eq!(m.add("btdig", "BTDig", SourceType::Dht, b).as_deref(), Some(HEX));
        assert_eq!(m.len(), 1);
        let r = m.get(HEX).unwrap();
        // Curated title kept over the lower-trust DHT title.
        assert_eq!(r.title, "Big Buck Bunny 1080p");
        assert_eq!(r.meta.resolution.as_deref(), Some("1080p"));
        assert_eq!(r.seeders, Some(25));
        assert_eq!(r.leechers, Some(3));
        assert_eq!(r.trackers, vec!["udp://tracker.example.org:1337/announce", "http://t2.example.org/announce"]);
        assert_eq!(r.files.len(), 1);
        assert_eq!(r.sources.len(), 2);
        assert!(r.has_source_type(SourceType::Dht) && r.has_source_type(SourceType::Curated));
        assert!(r.magnet.contains("tr=http%3A%2F%2Ft2.example.org%2Fannounce"));
    }

    #[test]
    fn higher_trust_title_replaces_lower() {
        let mut m = Merger::new();
        m.add("bt4g", "BT4G", SourceType::Dht, hit(HEX, "bunny"));
        let mut t = hit(HEX, "Big.Buck.Bunny.2008.2160p.WEB-DL");
        t.category = Some(Category::Movies);
        m.add("prowlarr", "Prowlarr", SourceType::Torznab, t);
        let r = m.get(HEX).unwrap();
        assert_eq!(r.title, "Big.Buck.Bunny.2008.2160p.WEB-DL");
        assert_eq!(r.meta.year, Some(2008));
        assert_eq!(r.category, Some(Category::Movies));
    }

    #[test]
    fn trackerless_magnet_gets_default_trackers() {
        let mut m = Merger::new();
        m.add("btdig", "BTDig", SourceType::Dht, hit(HEX, "x"));
        let r = m.get(HEX).unwrap();
        assert!(r.trackers.is_empty());
        assert_eq!(effective_trackers(r).len(), DEFAULT_TRACKERS.len());
        assert!(r.magnet.contains("opentrackr"));
        let parsed = infohash::parse_magnet(&r.magnet).unwrap();
        assert_eq!(parsed.info_hash, HEX);
    }

    #[test]
    fn same_indexer_reporting_twice_updates_its_source() {
        let mut m = Merger::new();
        let mut a = hit(HEX, "x");
        a.seeders = Some(1);
        m.add("nyaa", "Nyaa", SourceType::Curated, a);
        let mut b = hit(HEX, "x");
        b.seeders = Some(9);
        m.add("nyaa", "Nyaa", SourceType::Curated, b);
        let r = m.get(HEX).unwrap();
        assert_eq!(r.sources.len(), 1);
        assert_eq!(r.sources[0].seeders, Some(9));
    }

    #[test]
    fn invalid_hashes_are_dropped() {
        let mut m = Merger::new();
        assert!(m.add("x", "X", SourceType::Curated, hit("nope", "t")).is_none());
        assert!(m.is_empty());
    }

    #[test]
    fn earliest_upload_date_wins() {
        let mut m = Merger::new();
        let mut a = hit(HEX, "x");
        a.uploaded_at = Some(2_000);
        let mut b = hit(HEX, "x");
        b.uploaded_at = Some(1_000);
        m.add("a", "A", SourceType::Curated, a);
        m.add("b", "B", SourceType::Curated, b);
        assert_eq!(m.get(HEX).unwrap().uploaded_at, Some(1_000));
    }

    #[test]
    fn tracker_normalisation() {
        assert_eq!(normalize_tracker("UDP://Open.Stealth.SI:80/announce").as_deref(), Some("udp://open.stealth.si:80/announce"));
        assert_eq!(normalize_tracker("ftp://x.org/announce"), None);
        assert_eq!(normalize_tracker("garbage"), None);
    }
}
