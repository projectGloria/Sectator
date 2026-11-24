//! Local filtering (DSL operators the indexers can't apply) and the ranking engine.
//!
//! Score = weighted sum of token coverage, title position, peer health, recency,
//! cross-indexer consensus, source trust and size sanity (0–100), minus spam penalties.

use crate::models::{Category, SpamFlag, TorrentResult};
use crate::parser::query::SearchQuery;
use crate::parser::{groups, release};

const GB: u64 = 1 << 30;
const MB: u64 = 1 << 20;

const W_COVERAGE: f64 = 30.0;
const W_POSITION: f64 = 8.0;
const W_HEALTH: f64 = 25.0;
const W_RECENCY: f64 = 5.0;
const W_CONSENSUS: f64 = 10.0;
const W_TRUST: f64 = 12.0;
const W_SIZE: f64 = 10.0;

/// Seeder count reported by an indexer at or above which we stop believing it.
pub const SPOOF_SEEDERS: u32 = 9_999;

const EXECUTABLE_EXT: &[&str] = &["exe", "scr", "bat", "cmd", "msi", "lnk", "vbs", "ps1", "com", "pif", "jar", "js", "hta"];

fn penalty(f: SpamFlag) -> f64 {
    match f {
        SpamFlag::ExecutableInMedia => 45.0,
        SpamFlag::TinyForCategory => 30.0,
        SpamFlag::SpoofedSeeders => 25.0,
        SpamFlag::DeadSwarm => 20.0,
        SpamFlag::KeywordStuffed => 15.0,
        SpamFlag::NoSeedData => 0.0,
    }
}

/// Lowercase alphanumeric tokens joined by single spaces, padded so
/// `" term "` containment means a whole-token (or whole-token-run) match.
fn norm_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push(' ');
    for w in s.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()) {
        out.push_str(&w.to_lowercase());
        out.push(' ');
    }
    out
}

fn contains_tokens(haystack_norm: &str, needle: &str) -> bool {
    let n = norm_text(needle);
    n.len() > 2 && haystack_norm.contains(&n)
}

fn files_norm(r: &TorrentResult) -> Vec<String> {
    r.files.iter().map(|f| norm_text(&f.path)).collect()
}

/// Glob (`*`, `?`) or plain substring match, case-insensitive.
fn pattern_match(pattern: &str, text: &str) -> bool {
    let text = text.to_lowercase();
    if !pattern.contains(['*', '?']) {
        return text.contains(pattern);
    }
    // Unanchored glob: implicit `*` on both ends.
    let p: Vec<char> = format!("*{pattern}*").chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti, mut star, mut mark) = (0usize, 0usize, None::<usize>, 0usize);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

pub fn effective_category(r: &TorrentResult) -> Option<Category> {
    r.category.or_else(|| release::infer_category(&r.meta))
}

/// Whether a result survives every locally-applied operator.
pub fn passes_filters(r: &TorrentResult, q: &SearchQuery) -> bool {
    let title = norm_text(&r.title);
    if q.excluded.iter().any(|e| contains_tokens(&title, e)) {
        return false;
    }
    let files = files_norm(r);
    for p in &q.phrases {
        if !contains_tokens(&title, p) && !files.iter().any(|f| contains_tokens(f, p)) {
            return false;
        }
    }
    if !q.file_patterns.is_empty() {
        let ok = q.file_patterns.iter().all(|pat| {
            if r.files.is_empty() {
                pattern_match(pat, &r.title)
            } else {
                r.files.iter().any(|f| pattern_match(pat, &f.path))
            }
        });
        if !ok {
            return false;
        }
    }
    if let Some(want) = q.category {
        match effective_category(r) {
            Some(have) if have != want && !(want == Category::Tv && have == Category::Anime) => return false,
            _ => {}
        }
    }
    let tag_ok = |want: &[String], have: &Option<String>| {
        want.is_empty() || have.as_ref().is_some_and(|h| want.contains(&h.to_lowercase()))
    };
    if !tag_ok(&q.resolutions, &r.meta.resolution) || !tag_ok(&q.codecs, &r.meta.codec) || !tag_ok(&q.release_sources, &r.meta.source) {
        return false;
    }
    // Unknown seeders (unscraped DHT results) are kept until a live scrape says otherwise.
    if let (Some(f), Some(s)) = (q.seeds, r.best_seeders()) {
        if !f.accepts(u64::from(s)) {
            return false;
        }
    }
    if r.size > 0 && !q.sizes.iter().all(|f| f.accepts(r.size)) {
        return false;
    }
    if !q.sites.is_empty()
        && !r.sources.iter().any(|s| {
            let id = s.indexer_id.to_lowercase();
            let name = s.indexer_name.to_lowercase();
            q.sites.iter().any(|site| id.contains(site) || name.contains(site))
        })
    {
        return false;
    }
    if !q.source_types.is_empty() && !q.source_types.iter().any(|t| r.has_source_type(*t)) {
        return false;
    }
    if !q.by.is_empty() {
        // Re-derive the group: cached results keep the meta from when they were stored.
        let group = r.meta.group.clone().or_else(|| release::group_for(&r.title, &r.files));
        if !q.by.iter().any(|b| groups::matches(b, r.uploader.as_deref(), group.as_deref(), &r.title)) {
            return false;
        }
    }
    true
}

/// Expected size window for a video release; `None` when we can't judge.
fn size_window(r: &TorrentResult) -> Option<(u64, u64)> {
    if !effective_category(r).is_some_and(Category::is_video) {
        return None;
    }
    let (lo, hi) = match r.meta.resolution.as_deref()? {
        "4320p" | "2160p" => (2 * GB, 150 * GB),
        "1440p" | "1080p" => (500 * MB, 80 * GB),
        "720p" => (250 * MB, 20 * GB),
        _ => (100 * MB, 8 * GB),
    };
    Some(if r.meta.complete {
        (lo, hi * 20)
    } else if r.meta.episode.is_some() {
        (lo / 5, hi / 5)
    } else {
        (lo, hi)
    })
}

/// Spam heuristics. Pure function of the result, so it reruns after a live scrape.
pub fn spam_flags(r: &TorrentResult) -> Vec<SpamFlag> {
    let mut flags = Vec::new();
    let cat = effective_category(r);

    if cat.is_some_and(Category::is_video) && r.size > 0 {
        let tiny = r.size < 50 * MB || size_window(r).is_some_and(|(lo, _)| r.size < lo / 2);
        if tiny {
            flags.push(SpamFlag::TinyForCategory);
        }
    }

    let media = cat.is_some_and(Category::is_media) || r.meta.resolution.is_some();
    let has_exe = r.files.iter().any(|f| {
        f.path.rsplit_once('.').is_some_and(|(_, ext)| EXECUTABLE_EXT.contains(&ext.to_ascii_lowercase().as_str()))
    });
    if media && has_exe {
        flags.push(SpamFlag::ExecutableInMedia);
    }

    if r.best_seeders() == Some(0) {
        flags.push(SpamFlag::DeadSwarm);
    }

    if is_keyword_stuffed(&r.title) {
        flags.push(SpamFlag::KeywordStuffed);
    }

    let reported = r.seeders.unwrap_or(0);
    let spoofed = match r.live {
        Some(l) => reported >= 1_000 && reported > l.seeders.saturating_add(1).saturating_mul(50),
        None => reported >= SPOOF_SEEDERS,
    };
    if spoofed {
        flags.push(SpamFlag::SpoofedSeeders);
    }

    if r.best_seeders().is_none() {
        flags.push(SpamFlag::NoSeedData);
    }
    flags
}

fn is_keyword_stuffed(title: &str) -> bool {
    let toks: Vec<String> = title
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 2)
        .map(str::to_lowercase)
        .collect();
    if toks.len() > 35 {
        return true;
    }
    let mut counts = std::collections::HashMap::new();
    for t in &toks {
        *counts.entry(t.as_str()).or_insert(0u32) += 1;
    }
    if counts.values().any(|&c| c >= 4) {
        return true;
    }
    let resolutions = ["2160p", "1080p", "720p", "480p", "4k"].iter().filter(|r| counts.contains_key(*r)).count();
    let codecs = ["x264", "x265", "hevc", "h264", "h265", "av1"].iter().filter(|c| counts.contains_key(*c)).count();
    resolutions >= 3 || codecs >= 3
}

/// Score one result against the query and attach its flags.
pub fn evaluate(r: &mut TorrentResult, q: &SearchQuery, now: i64) {
    let title = norm_text(&r.title);
    let files = files_norm(r);
    let needles: Vec<&String> = q.terms.iter().chain(q.phrases.iter()).collect();

    let mut covered = 0.0;
    let mut file_match = false;
    for n in &needles {
        if contains_tokens(&title, n) {
            covered += 1.0;
        } else if files.iter().any(|f| contains_tokens(f, n)) {
            covered += 0.5;
            file_match = true;
        }
    }
    if !q.file_patterns.is_empty() && !r.files.is_empty() {
        file_match = true;
    }
    let coverage = if needles.is_empty() { 1.0 } else { covered / needles.len() as f64 };

    let position = needles
        .iter()
        .filter_map(|n| {
            let needle = norm_text(n);
            title.find(&needle).map(|byte| title[..byte].matches(' ').count())
        })
        .min()
        .map_or(0.0, |idx| 1.0 / (1.0 + idx as f64 * 0.3));

    let health = match r.best_seeders() {
        Some(s) => {
            let s = f64::from(s.min(SPOOF_SEEDERS));
            let l = f64::from(r.best_leechers().unwrap_or(0));
            let base = (1.0 + s).ln() / (1.0 + 5_000f64).ln();
            // A swarm that is mostly leechers is slower than its seed count suggests.
            let ratio = if s + l > 0.0 { 0.85 + 0.15 * (s / (s + l)) } else { 1.0 };
            (base * ratio).min(1.0)
        }
        None => 0.15,
    };

    let recency = match r.uploaded_at {
        Some(t) if t > 0 => {
            let days = ((now - t).max(0) as f64) / 86_400.0;
            (-days / 365.0).exp()
        }
        _ => 0.3,
    };

    let consensus = (r.sources.len().clamp(1, 4) - 1) as f64 / 3.0;

    let trust = r.sources.iter().map(|s| s.source_type.trust()).fold(0.0, f64::max)
        + if r.verified_uploader { 0.1 } else { 0.0 };

    let size = match size_window(r) {
        Some((lo, hi)) if r.size > 0 => {
            if (lo..=hi).contains(&r.size) {
                1.0
            } else {
                0.4
            }
        }
        _ => 0.7,
    };

    let flags = spam_flags(r);
    let mut score = W_COVERAGE * coverage
        + W_POSITION * position
        + W_HEALTH * health
        + W_RECENCY * recency
        + W_CONSENSUS * consensus
        + W_TRUST * trust.min(1.0)
        + W_SIZE * size;
    score -= flags.iter().map(|f| penalty(*f)).sum::<f64>();

    r.score = (score.max(0.0) * 10.0).round() / 10.0;
    r.flags = flags;
    r.file_match = file_match;
}

/// Best first; ties broken by seeders, then title for a stable order.
pub fn rank(results: &mut [TorrentResult]) {
    results.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| b.best_seeders().unwrap_or(0).cmp(&a.best_seeders().unwrap_or(0)))
            .then_with(|| a.title.cmp(&b.title))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::merge::Merger;
    use crate::models::{IndexerHit, LivePeers, SourceType, TorrentFile};

    const NOW: i64 = 1_790_000_000;

    fn build(hits: Vec<(&str, SourceType, IndexerHit)>) -> Vec<TorrentResult> {
        let mut m = Merger::new();
        for (id, st, h) in hits {
            m.add(id, id, st, h);
        }
        m.into_results()
    }

    fn hit(hash_byte: u8, title: &str, size: u64, seeders: Option<u32>) -> IndexerHit {
        IndexerHit {
            info_hash: format!("{:02x}", hash_byte).repeat(20),
            title: title.into(),
            size,
            seeders,
            leechers: seeders.map(|s| s / 4),
            uploaded_at: Some(NOW - 30 * 86_400),
            ..Default::default()
        }
    }

    #[test]
    fn healthy_exact_match_outranks_spam_and_partial() {
        let q = SearchQuery::parse("dune part two 1080p");
        let mut rs = build(vec![
            ("apibay", SourceType::Curated, hit(1, "Dune Part Two 2024 1080p BluRay x264-GROUP", 12 * GB, Some(800))),
            ("btdig", SourceType::Dht, hit(1, "Dune Part Two 2024 1080p BluRay x264-GROUP", 12 * GB, None)),
            ("apibay", SourceType::Curated, hit(2, "Dune Part Two 2024 1080p HDCAM", 30 * MB, Some(15_000))),
            ("bt4g", SourceType::Dht, hit(3, "Dune 1984 720p", 2 * GB, None)),
        ]);
        for r in &mut rs {
            evaluate(r, &q, NOW);
        }
        rank(&mut rs);
        assert_eq!(rs[0].info_hash, "01".repeat(20));
        assert_eq!(rs[0].sources.len(), 2);
        assert!(rs[0].flags.is_empty(), "{:?}", rs[0].flags);
        let spam = rs.iter().find(|r| r.info_hash == "02".repeat(20)).unwrap();
        assert!(spam.flags.contains(&SpamFlag::TinyForCategory));
        assert!(spam.flags.contains(&SpamFlag::SpoofedSeeders));
        assert_eq!(rs.last().unwrap().info_hash, "02".repeat(20));
    }

    #[test]
    fn executable_inside_video_is_flagged() {
        let mut h = hit(4, "New Movie 2025 1080p WEB-DL", 2 * GB, Some(50));
        h.files = vec![
            TorrentFile { path: "New Movie/movie.mkv".into(), size: 2 * GB },
            TorrentFile { path: "New Movie/Codec Installer.EXE".into(), size: MB },
        ];
        let mut rs = build(vec![("x", SourceType::Curated, h)]);
        evaluate(&mut rs[0], &SearchQuery::parse("new movie"), NOW);
        assert!(rs[0].flags.contains(&SpamFlag::ExecutableInMedia));
    }

    #[test]
    fn dead_swarm_and_no_seed_data() {
        let mut rs = build(vec![
            ("a", SourceType::Curated, hit(5, "Linux ISO", GB, Some(0))),
            ("b", SourceType::Dht, hit(6, "Linux ISO", GB, None)),
        ]);
        for r in &mut rs {
            evaluate(r, &SearchQuery::parse("linux"), NOW);
        }
        assert!(rs[0].flags.contains(&SpamFlag::DeadSwarm));
        assert!(rs[1].flags.contains(&SpamFlag::NoSeedData));
    }

    #[test]
    fn live_scrape_unmasks_spoofed_counts() {
        let mut rs = build(vec![("a", SourceType::Curated, hit(7, "Ubuntu 24.04", 6 * GB, Some(5_000)))]);
        let r = &mut rs[0];
        evaluate(r, &SearchQuery::parse("ubuntu"), NOW);
        assert!(!r.flags.contains(&SpamFlag::SpoofedSeeders));
        r.live = Some(LivePeers { seeders: 3, leechers: 1, completed: 0, scraped_at: NOW, trackers_ok: 2 });
        evaluate(r, &SearchQuery::parse("ubuntu"), NOW);
        assert!(r.flags.contains(&SpamFlag::SpoofedSeeders));
        assert_eq!(r.best_seeders(), Some(3));
    }

    #[test]
    fn keyword_stuffing() {
        assert!(is_keyword_stuffed("Movie 720p 1080p 2160p 4K x264 x265 HEVC best quality"));
        assert!(is_keyword_stuffed("free free free free movie"));
        assert!(!is_keyword_stuffed("Dune.Part.Two.2024.2160p.UHD.BluRay.REMUX.DV.HDR10.HEVC.TrueHD.7.1.Atmos-FraMeSToR"));
    }

    #[test]
    fn local_filters_apply() {
        let mut a = hit(8, "Show S01E01 1080p WEB-DL x265", 900 * MB, Some(40));
        a.files = vec![TorrentFile { path: "Show/Show.S01E01.mkv".into(), size: 900 * MB }];
        let rs = build(vec![
            ("nyaa", SourceType::Curated, a),
            ("btdig", SourceType::Dht, hit(9, "Show S01E01 720p HDTV x264 CAM", 300 * MB, Some(2))),
        ]);
        let (good, bad) = (&rs[0], &rs[1]);
        let q = |s: &str| SearchQuery::parse(s);
        assert!(passes_filters(good, &q("show res:1080p codec:hevc src:web-dl cat:tv seeds:>=10 size:<2gb")));
        assert!(!passes_filters(bad, &q("show res:1080p")));
        assert!(!passes_filters(bad, &q("show -cam")));
        assert!(passes_filters(good, &q("show file:*.mkv")));
        assert!(!passes_filters(good, &q("show file:*.srt")));
        assert!(passes_filters(good, &q("show site:nyaa source:curated")));
        assert!(!passes_filters(good, &q("show source:dht")));
        assert!(!passes_filters(good, &q("show cat:movies")));
        assert!(!passes_filters(good, &q("show \"s01e02\"")));
        assert!(!passes_filters(bad, &q("show seeds:>5")));
    }

    #[test]
    fn file_match_marks_inner_hits() {
        let mut h = hit(10, "Complete Discography", 3 * GB, Some(10));
        h.files = vec![TorrentFile { path: "Band/1999 - Album/03 - Hidden Track.flac".into(), size: 30 * MB }];
        let mut rs = build(vec![("a", SourceType::Curated, h)]);
        evaluate(&mut rs[0], &SearchQuery::parse("hidden track"), NOW);
        assert!(rs[0].file_match);
    }

    #[test]
    fn glob_patterns() {
        assert!(pattern_match("*.mkv", "dir/file.MKV"));
        assert!(pattern_match("s01e0?", "show.s01e05.mkv"));
        assert!(!pattern_match("*.srt", "file.mkv"));
        assert!(pattern_match("sample", "Sample/x.mkv"));
    }
}
