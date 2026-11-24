//! Release-name parser: pulls resolution, source, codec, HDR, audio, episode, group,
//! version, OS/arch and container tags out of scene-style and anime-style titles.

use once_cell::sync::Lazy;
use regex::Regex;

use crate::models::{Category, ParsedMeta, TorrentFile};

/// Wrap a pattern with alphanumeric word boundaries (`regex` has no lookaround,
/// so the boundary characters are consumed by non-capturing groups).
fn re(pat: &str) -> Regex {
    Regex::new(&format!(r"(?i)(?:^|[^a-z0-9])(?:{pat})(?:$|[^a-z0-9])")).expect("valid regex")
}

macro_rules! lazy_re {
    ($name:ident, $pat:expr) => {
        static $name: Lazy<Regex> = Lazy::new(|| re($pat));
    };
}

lazy_re!(RES_P, r"(4320|2160|1440|1080|720|576|480|360)[pi]");
lazy_re!(RES_DIM, r"(?:3840|4096|1920|1280|720|640)x(2160|1080|720|576|480)");
lazy_re!(RES_4K, r"4k|uhd");
lazy_re!(RES_8K, r"8k");

lazy_re!(SRC_REMUX, r"remux|bdremux");
lazy_re!(SRC_WEBDL, r"web[ .\-_]?dl|amzn[ .\-_]?web|webhd");
lazy_re!(SRC_WEBRIP, r"web[ .\-_]?rip");
lazy_re!(SRC_BLURAY, r"blu[ .\-_]?ray|bluray|bdrip|brrip|bd25|bd50|bd");
lazy_re!(SRC_HDTV, r"hdtv|pdtv|dsr");
lazy_re!(SRC_DVD, r"dvd[ .\-_]?rip|dvd5|dvd9|dvd[ .\-_]?r|dvd");
lazy_re!(SRC_HDRIP, r"hd[ .\-_]?rip");
lazy_re!(SRC_CAM, r"cam|cam[ .\-_]?rip|hd[ .\-_]?cam");
lazy_re!(SRC_TS, r"ts|telesync|hd[ .\-_]?ts|pdvd");
lazy_re!(SRC_SCR, r"screener|scr|dvd[ .\-_]?scr");
lazy_re!(SRC_WEB, r"web");

lazy_re!(CODEC_265, r"x[ .]?265|h[ .]?265|hevc");
lazy_re!(CODEC_264, r"x[ .]?264|h[ .]?264|avc");
lazy_re!(CODEC_AV1, r"av1");
lazy_re!(CODEC_XVID, r"xvid|divx");
lazy_re!(CODEC_VP9, r"vp9");

lazy_re!(HDR10P, r"hdr10[ .]?\+|hdr10plus|hdr10p");
lazy_re!(HDR10, r"hdr10");
lazy_re!(HDR, r"hdr");
lazy_re!(DV, r"dv|dovi|dolby[ .]?vision");
lazy_re!(HLG, r"hlg");
lazy_re!(SDR, r"sdr");

lazy_re!(AUD_ATMOS, r"atmos");
lazy_re!(AUD_TRUEHD, r"true[ .\-]?hd");
lazy_re!(AUD_DTSX, r"dts[ .\-:]?x");
lazy_re!(AUD_DTSHD, r"dts[ .\-]?hd(?:[ .\-]?ma)?|dts[ .\-]?ma");
lazy_re!(AUD_DTS, r"dts(?:[0-9][ .][0-9])?");
lazy_re!(AUD_DDP, r"ddp(?:[0-9][ .]?[0-9])?|dd\+(?:[0-9][ .]?[0-9])?|e[ .\-]?ac[ .\-]?3");
lazy_re!(AUD_DD, r"dd(?:[0-9][ .]?[0-9])?|ac3|dolby[ .]?digital");
lazy_re!(AUD_AAC, r"aac(?:[0-9][ .]?[0-9])?");
lazy_re!(AUD_FLAC, r"flac");
lazy_re!(AUD_OPUS, r"opus");
lazy_re!(AUD_MP3, r"mp3|320[ .]?kbps|v0");

static CHANNELS: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?:^|[^0-9])([2-7])[ .]([01])(?:$|[^0-9.]|\.(?:$|[^0-9]))").expect("valid regex"));

lazy_re!(SXXEXX, r"s(\d{1,2})[ .\-]?e(\d{1,4})");
lazy_re!(NXNN, r"(\d{1,2})x(\d{2,3})");
lazy_re!(SEASON_ONLY, r"s(\d{1,2})|season[ .]?(\d{1,2})");
lazy_re!(SEASON_RANGE, r"s\d{1,2}[ .\-]*s?\d{1,2}|seasons?[ .]?\d{1,2}[ .\-]+\d{1,2}");
lazy_re!(COMPLETE, r"complete|integrale|full[ .]?series|batch");
static ANIME_EP: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?:\s|_)-(?:\s|_)(\d{1,4})(?:v\d)?(?:\s|_|$|\[|\(|\.)").expect("valid regex")
});


lazy_re!(VERSION_V, r"v(\d+(?:\.\d+){1,3})");
lazy_re!(VERSION_BARE, r"(\d+\.\d+\.\d+(?:\.\d+)?)");
lazy_re!(VERSION_WORD, r"version[ .]?(\d+(?:\.\d+)+)");

lazy_re!(OS_WIN, r"windows|win(?:10|11|7|8|64|32|x64)?");
lazy_re!(OS_MAC, r"macos|mac[ .]?os(?:[ .]?x)?|osx");
lazy_re!(OS_LINUX, r"linux|ubuntu|debian|fedora|arch[ .]?linux|appimage|deb|rpm");
lazy_re!(OS_ANDROID, r"android|apk");
lazy_re!(OS_IOS, r"ios|ipa");

lazy_re!(ARCH_X64, r"x64|x86[ _\-]64|amd64|64[ .\-]?bit|win64");
lazy_re!(ARCH_ARM, r"arm64|aarch64|apple[ .]silicon");
lazy_re!(ARCH_X86, r"x86|32[ .\-]?bit|i[36]86|win32");

lazy_re!(
    FORMAT,
    r"(mkv|mp4|avi|m2ts|iso|epub|pdf|mobi|azw3|cbz|cbr|flac|mp3|m4a|m4b|ogg|wav|exe|msi|dmg|pkg|apk|zip|rar|7z)"
);

static ANIME_GROUP: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\s*\[([^\]]{1,40})\]").expect("valid regex"));
static SCENE_GROUP: Lazy<Regex> = Lazy::new(|| Regex::new(r"-([A-Za-z0-9][A-Za-z0-9_@]{1,24})$").expect("valid regex"));
static TRAILING_TAGS: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)(\s*(\[[^\]]*\]|\([^)]*\)|\.(mkv|mp4|avi|torrent|rar|zip|iso)))+\s*$").expect("valid regex")
});

/// Tokens that look like a `-GROUP` suffix but are actually release tags.
const NOT_GROUPS: &[&str] = &[
    "dl", "rip", "web", "x264", "x265", "h264", "h265", "hevc", "avc", "hd", "ma", "ac3", "aac", "dts",
    "1080p", "720p", "2160p", "480p", "bluray", "remux", "hdr", "dv", "atmos", "proper", "repack",
    "x64", "x86", "amd64", "arm64", "win64", "win32", "desktop", "portable", "multi", "subs",
];

/// Canonical resolution name for user input or a captured tag.
pub fn canon_resolution(s: &str) -> Option<&'static str> {
    Some(match s.trim().to_ascii_lowercase().as_str() {
        "4320p" | "8k" => "4320p",
        "2160p" | "2160i" | "4k" | "uhd" => "2160p",
        "1440p" | "2k" => "1440p",
        "1080p" | "1080i" | "1080" | "fhd" => "1080p",
        "720p" | "720" | "hd" => "720p",
        "576p" | "576i" | "576" => "576p",
        "480p" | "480i" | "480" | "sd" => "480p",
        "360p" => "360p",
        _ => return None,
    })
}

pub fn canon_codec(s: &str) -> Option<&'static str> {
    Some(match s.trim().to_ascii_lowercase().replace(['.', ' '], "").as_str() {
        "x265" | "h265" | "hevc" => "x265",
        "x264" | "h264" | "avc" => "x264",
        "av1" => "AV1",
        "xvid" | "divx" => "XviD",
        "vp9" => "VP9",
        _ => return None,
    })
}

pub fn canon_source(s: &str) -> Option<&'static str> {
    Some(match s.trim().to_ascii_lowercase().replace([' ', '_', '.'], "-").as_str() {
        "remux" | "bdremux" => "Remux",
        "bluray" | "blu-ray" | "bdrip" | "brrip" | "bd" => "BluRay",
        "web-dl" | "webdl" | "web" => "WEB-DL",
        "webrip" | "web-rip" => "WEBRip",
        "hdtv" => "HDTV",
        "dvd" | "dvdrip" | "dvd-rip" => "DVD",
        "hdrip" | "hd-rip" => "HDRip",
        "cam" | "camrip" | "hdcam" => "CAM",
        "ts" | "telesync" | "hdts" => "TS",
        "scr" | "screener" | "dvdscr" => "SCR",
        _ => return None,
    })
}

/// Parse a release name into structured tags.
pub fn parse(title: &str) -> ParsedMeta {
    let mut m = ParsedMeta::default();
    let t = title;

    m.resolution = if let Some(c) = RES_P.captures(t) {
        canon_resolution(&format!("{}p", &c[1])).map(str::to_string)
    } else if let Some(c) = RES_DIM.captures(t) {
        canon_resolution(&format!("{}p", &c[1])).map(str::to_string)
    } else if RES_8K.is_match(t) {
        Some("4320p".into())
    } else if RES_4K.is_match(t) {
        Some("2160p".into())
    } else {
        None
    };

    m.source = [
        (&SRC_REMUX, "Remux"),
        (&SRC_WEBDL, "WEB-DL"),
        (&SRC_WEBRIP, "WEBRip"),
        (&SRC_BLURAY, "BluRay"),
        (&SRC_HDTV, "HDTV"),
        (&SRC_HDRIP, "HDRip"),
        (&SRC_SCR, "SCR"),
        (&SRC_DVD, "DVD"),
        (&SRC_CAM, "CAM"),
        (&SRC_TS, "TS"),
        (&SRC_WEB, "WEB-DL"),
    ]
    .iter()
    .find(|(r, _)| r.is_match(t))
    .map(|(_, s)| s.to_string());

    m.codec = [(&CODEC_265, "x265"), (&CODEC_264, "x264"), (&CODEC_AV1, "AV1"), (&CODEC_XVID, "XviD"), (&CODEC_VP9, "VP9")]
        .iter()
        .find(|(r, _)| r.is_match(t))
        .map(|(_, s)| s.to_string());

    if HDR10P.is_match(t) {
        m.dynamic_range.push("HDR10+".into());
    } else if HDR10.is_match(t) {
        m.dynamic_range.push("HDR10".into());
    } else if HDR.is_match(t) {
        m.dynamic_range.push("HDR".into());
    }
    if DV.is_match(t) {
        m.dynamic_range.push("DV".into());
    }
    if HLG.is_match(t) {
        m.dynamic_range.push("HLG".into());
    }
    if m.dynamic_range.is_empty() && SDR.is_match(t) {
        m.dynamic_range.push("SDR".into());
    }

    let mut has_audio = false;
    let mut add_audio = |m: &mut ParsedMeta, name: &str| {
        m.audio.push(name.to_string());
        has_audio = true;
    };
    if AUD_TRUEHD.is_match(t) {
        add_audio(&mut m, "TrueHD");
    }
    if AUD_DTSX.is_match(t) {
        add_audio(&mut m, "DTS:X");
    } else if AUD_DTSHD.is_match(t) {
        add_audio(&mut m, "DTS-HD MA");
    } else if AUD_DTS.is_match(t) {
        add_audio(&mut m, "DTS");
    }
    if AUD_DDP.is_match(t) {
        add_audio(&mut m, "DD+");
    } else if AUD_DD.is_match(t) {
        add_audio(&mut m, "DD");
    }
    if AUD_AAC.is_match(t) {
        add_audio(&mut m, "AAC");
    }
    if AUD_FLAC.is_match(t) {
        add_audio(&mut m, "FLAC");
    }
    if AUD_OPUS.is_match(t) {
        add_audio(&mut m, "Opus");
    }
    if AUD_MP3.is_match(t) {
        add_audio(&mut m, "MP3");
    }
    if AUD_ATMOS.is_match(t) {
        add_audio(&mut m, "Atmos");
    }
    if has_audio {
        if let Some(c) = CHANNELS.captures(t) {
            m.audio.push(format!("{}.{}", &c[1], &c[2]));
        }
    }

    if let Some(c) = SXXEXX.captures(t) {
        m.season = c[1].parse().ok();
        m.episode = c[2].parse().ok();
    } else if let Some(c) = NXNN.captures(t) {
        m.season = c[1].parse().ok();
        m.episode = c[2].parse().ok();
    } else if let Some(c) = SEASON_ONLY.captures(t) {
        m.season = c.get(1).or(c.get(2)).and_then(|g| g.as_str().parse().ok());
    }
    if m.episode.is_none() {
        if let Some(c) = ANIME_EP.captures(t) {
            let n: u32 = c[1].parse().unwrap_or(0);
            // A 4-digit "episode" in the 1900–2099 range is a year, not an episode.
            if n > 0 && !(1900..2100).contains(&n) {
                m.episode = Some(n);
            }
        }
    }
    m.complete = COMPLETE.is_match(t) || SEASON_RANGE.is_match(t) || (m.season.is_some() && m.episode.is_none());

    // The last year-like token wins, so a title that *is* a year ("1917") keeps its release year.
    m.year = t
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| w.len() == 4)
        .filter_map(|w| w.parse::<u32>().ok())
        .filter(|y| (1920..2050).contains(y))
        .last();

    m.version = VERSION_V
        .captures(t)
        .or_else(|| VERSION_WORD.captures(t))
        .or_else(|| VERSION_BARE.captures(t))
        .map(|c| c[1].to_string());

    m.os = [(&OS_WIN, "Windows"), (&OS_MAC, "macOS"), (&OS_ANDROID, "Android"), (&OS_IOS, "iOS"), (&OS_LINUX, "Linux")]
        .iter()
        .find(|(r, _)| r.is_match(t))
        .map(|(_, s)| s.to_string());

    m.arch = if ARCH_X64.is_match(t) {
        Some("x64".into())
    } else if ARCH_ARM.is_match(t) {
        Some("arm64".into())
    } else if ARCH_X86.is_match(t) {
        Some("x86".into())
    } else {
        None
    };

    for c in FORMAT.captures_iter(t) {
        let f = c[1].to_ascii_uppercase();
        if !m.format.contains(&f) {
            m.format.push(f);
        }
    }

    m.group = parse_group(t);
    m
}

/// Release group from a raw title (scene suffix, anime prefix or trailing encoder).
pub fn parse_group(title: &str) -> Option<String> {
    if let Some(c) = ANIME_GROUP.captures(title) {
        let g = c[1].trim();
        // A leading "[1080p]" or "[2024]" is a tag, not a group.
        if canon_resolution(g).is_none() && !g.chars().all(|c| c.is_ascii_digit()) {
            return Some(g.to_string());
        }
    }
    if let Some(g) = paren_encoder(title) {
        return Some(g);
    }
    let stripped = TRAILING_TAGS.replace(title.trim(), "");
    let c = SCENE_GROUP.captures(stripped.trim_end())?;
    let g = &c[1];
    if NOT_GROUPS.contains(&g.to_ascii_lowercase().as_str()) || g.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(g.to_string())
}

/// Whether a site cut the title short: long, with a bracket left open (TPB stops at 64
/// characters, so `… (1080p BluRay x265 HEVC 10bit AAC 5.1` loses its closing encoder).
pub fn looks_truncated(title: &str) -> bool {
    let t = title.trim();
    let open = |a: char, b: char| t.matches(a).count() > t.matches(b).count();
    t.chars().count() >= 60 && (open('(', ')') || open('[', ']'))
}

const VIDEO_EXTS: &[&str] = &["mkv", "mp4", "avi", "m4v", "ts", "wmv", "webm", "mov"];

/// The release group shared by most of a torrent's video files, e.g. `Silence` from
/// `Fargo (2014) - S02E08 - Loplop (1080p x265 Silence).mkv`.
pub fn group_from_files(files: &[TorrentFile]) -> Option<String> {
    let stems: Vec<&str> = files
        .iter()
        .filter_map(|f| {
            let name = f.path.rsplit(['/', '\\']).next()?;
            let (stem, ext) = name.rsplit_once('.')?;
            VIDEO_EXTS.contains(&ext.to_ascii_lowercase().as_str()).then_some(stem)
        })
        .collect();
    let mut counts: Vec<(String, usize)> = Vec::new();
    for g in stems.iter().filter_map(|s| parse_group(s)) {
        match counts.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case(&g)) {
            Some((_, n)) => *n += 1,
            None => counts.push((g, 1)),
        }
    }
    let (group, n) = counts.into_iter().max_by_key(|(_, n)| *n)?;
    (n * 2 > stems.len()).then_some(group)
}

/// Release group from the title, or from the file names when the title was cut short.
pub fn group_for(title: &str, files: &[TorrentFile]) -> Option<String> {
    parse_group(title).or_else(|| looks_truncated(title).then(|| group_from_files(files)).flatten())
}

/// Encoder named at the end of a trailing spec block, as in
/// `Show S01 (1080p BluRay x265 HEVC 10bit AAC 5.1 Tigole) [QxR]`.
fn paren_encoder(title: &str) -> Option<String> {
    let t = BRACKET_SUFFIX.replace(title.trim(), "");
    let c = PAREN_SPEC.captures(t.trim_end())?;
    let words: Vec<&str> = c[1].split_whitespace().collect();
    if words.len() < 3 || !words.iter().any(|w| canon_resolution(w).is_some()) {
        return None;
    }
    let last = *words.last()?;
    let lower = last.to_ascii_lowercase();
    let is_tag = canon_resolution(last).is_some()
        || canon_codec(last).is_some()
        || canon_source(last).is_some()
        || NOT_GROUPS.contains(&lower.as_str())
        || PAREN_NOT_GROUPS.contains(&lower.as_str());
    let shaped = last.len() >= 2 && last.len() <= 24 && last.chars().all(|c| c.is_ascii_alphanumeric()) && last.chars().any(|c| c.is_ascii_alphabetic());
    (shaped && !is_tag).then(|| last.to_string())
}

static PAREN_SPEC: Lazy<Regex> = Lazy::new(|| Regex::new(r"\(([^()]{6,120})\)$").expect("valid regex"));
static BRACKET_SUFFIX: Lazy<Regex> = Lazy::new(|| Regex::new(r"(\s*\[[^\]]*\])+\s*$").expect("valid regex"));
/// Audio / video tags that can end a spec block without being an encoder.
const PAREN_NOT_GROUPS: &[&str] = &[
    "eac3", "ddp", "dd", "opus", "flac", "truehd", "mp3", "10bit", "8bit", "sdr", "dovi", "hybrid", "mono", "stereo", "ita", "eng",
];

/// Best-effort category when the indexer did not report one.
pub fn infer_category(meta: &ParsedMeta) -> Option<Category> {
    if meta.os.is_some() || meta.format.iter().any(|f| matches!(f.as_str(), "EXE" | "MSI" | "DMG" | "PKG" | "APK")) {
        return Some(Category::Software);
    }
    if meta.format.iter().any(|f| matches!(f.as_str(), "EPUB" | "PDF" | "MOBI" | "AZW3" | "CBZ" | "CBR" | "M4B")) {
        return Some(Category::Books);
    }
    if meta.season.is_some() || meta.episode.is_some() {
        return Some(Category::Tv);
    }
    if meta.resolution.is_some() || meta.codec.is_some() || meta.source.is_some() {
        return Some(Category::Movies);
    }
    if meta.audio.iter().any(|a| a == "FLAC" || a == "MP3") {
        return Some(Category::Music);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_recovered_from_files_when_title_is_cut() {
        let cut = "Fargo (2014) Season 2 S02 (1080p BluRay x265 HEVC 10bit AAC 5.1";
        assert!(looks_truncated(cut));
        assert!(!looks_truncated("Fargo (2014) Season 5 S05 (1080p AMZN WEB-DL x265 HEVC 10bit EAC3 5.1 Silence)"));
        assert!(!looks_truncated("Fargo (2014"));
        let f = |p: &str| TorrentFile { path: p.into(), size: 1 };
        // From TPB's file list for that torrent.
        let files = vec![
            f("Fargo (2014) - S02E08 - Loplop (1080p x265 Silence).mkv"),
            f("Fargo (2014) - S02E03 - The Myth of Sisyphus (1080p BluRay x265 Silence).mkv"),
            f("Season 2/Fargo (2014) - S02E04 - Fear and Trembling (1080p BluRay x265 Silence).mkv"),
            f("Fargo (2014) - S02E04 - Fear and Trembling (1080p BluRay x265 Silence).srt"),
        ];
        assert_eq!(parse_group(cut), None);
        assert_eq!(group_from_files(&files).as_deref(), Some("Silence"));
        assert_eq!(group_for(cut, &files).as_deref(), Some("Silence"));
        // A complete title never borrows a group from its files.
        assert_eq!(group_for("Fargo.S04.1080p.AMZN.WEBRip.DDP5.1.x264-TOMMY", &files).as_deref(), Some("TOMMY"));
        assert_eq!(group_for("Some Long Untagged Title Without Any Group Information At All Here", &files), None);
        // Files that disagree give no group.
        let mixed = vec![f("a (1080p x265 Joy).mkv"), f("b (1080p x265 Silence).mkv")];
        assert_eq!(group_from_files(&mixed), None);
    }

    #[test]
    fn scene_movie_remux() {
        let m = parse("Dune.Part.Two.2024.2160p.UHD.BluRay.REMUX.DV.HDR10.HEVC.TrueHD.7.1.Atmos-FraMeSToR");
        assert_eq!(m.resolution.as_deref(), Some("2160p"));
        assert_eq!(m.source.as_deref(), Some("Remux"));
        assert_eq!(m.codec.as_deref(), Some("x265"));
        assert_eq!(m.dynamic_range, vec!["HDR10", "DV"]);
        assert_eq!(m.audio, vec!["TrueHD", "Atmos", "7.1"]);
        assert_eq!(m.year, Some(2024));
        assert_eq!(m.group.as_deref(), Some("FraMeSToR"));
        assert_eq!(m.season, None);
        assert!(!m.complete);
        assert_eq!(infer_category(&m), Some(Category::Movies));
    }

    #[test]
    fn web_dl_episode_with_ddp() {
        let m = parse("The.Bear.S03E05.1080p.WEB-DL.DDP5.1.H.264-NTb[EZTVx.to].mkv");
        assert_eq!(m.season, Some(3));
        assert_eq!(m.episode, Some(5));
        assert_eq!(m.resolution.as_deref(), Some("1080p"));
        assert_eq!(m.source.as_deref(), Some("WEB-DL"));
        assert_eq!(m.codec.as_deref(), Some("x264"));
        assert_eq!(m.audio, vec!["DD+", "5.1"]);
        assert_eq!(m.group.as_deref(), Some("NTb"));
        assert!(m.format.contains(&"MKV".to_string()));
        assert_eq!(infer_category(&m), Some(Category::Tv));
    }

    #[test]
    fn season_pack_is_complete() {
        let m = parse("Breaking Bad S01-S05 Complete 720p BluRay x264");
        assert_eq!(m.season, Some(1));
        assert!(m.complete);
        assert_eq!(m.source.as_deref(), Some("BluRay"));
        let pack = parse("Severance.S02.2160p.ATVP.WEB-DL.DDP5.1.Atmos.DV.H.265-FLUX");
        assert_eq!(pack.season, Some(2));
        assert_eq!(pack.episode, None);
        assert!(pack.complete);
        assert_eq!(pack.codec.as_deref(), Some("x265"));
    }

    #[test]
    fn anime_fansub_style() {
        let m = parse("[SubsPlease] Frieren - 12 (1080p) [A1B2C3D4].mkv");
        assert_eq!(m.group.as_deref(), Some("SubsPlease"));
        assert_eq!(m.episode, Some(12));
        assert_eq!(m.resolution.as_deref(), Some("1080p"));
        let batch = parse("[Erai-raws] Spy x Family - 01 ~ 12 [Batch][1080p]");
        assert!(batch.complete);
    }

    #[test]
    fn software_release() {
        let m = parse("Blender v4.2.1 Win64 x64 Portable");
        assert_eq!(m.version.as_deref(), Some("4.2.1"));
        assert_eq!(m.os.as_deref(), Some("Windows"));
        assert_eq!(m.arch.as_deref(), Some("x64"));
        assert_eq!(m.resolution, None);
        assert_eq!(infer_category(&m), Some(Category::Software));

        let l = parse("ubuntu-24.04.1-desktop-amd64.iso");
        assert_eq!(l.version.as_deref(), Some("24.04.1"));
        assert_eq!(l.os.as_deref(), Some("Linux"));
        assert_eq!(l.arch.as_deref(), Some("x64"));
        assert_eq!(l.format, vec!["ISO"]);
    }

    #[test]
    fn cam_and_ts_sources_are_detected() {
        assert_eq!(parse("Some Movie 2025 HDCAM x264").source.as_deref(), Some("CAM"));
        assert_eq!(parse("Some Movie 2025 HDTS 720p").source.as_deref(), Some("TS"));
        assert_eq!(parse("Some.Movie.2025.1080p.WEBRip.x265").source.as_deref(), Some("WEBRip"));
    }

    #[test]
    fn year_title_keeps_release_year() {
        let m = parse("1917.2019.1080p.BluRay.x264-SPARKS");
        assert_eq!(m.year, Some(2019));
        assert_eq!(m.group.as_deref(), Some("SPARKS"));
    }

    #[test]
    fn music_and_books() {
        let m = parse("Pink Floyd - The Dark Side of the Moon (1973) [FLAC 24-96]");
        assert!(m.audio.contains(&"FLAC".to_string()));
        assert_eq!(m.year, Some(1973));
        assert_eq!(m.episode, None);
        assert_eq!(infer_category(&m), Some(Category::Music));
        let b = parse("Clean Code - Robert Martin (epub, pdf)");
        assert_eq!(b.format, vec!["EPUB", "PDF"]);
        assert_eq!(infer_category(&b), Some(Category::Books));
    }

    #[test]
    fn encoder_at_end_of_spec_block_is_the_group() {
        assert_eq!(parse("Fargo (2014) Season 5 S05 (1080p AMZN WEB-DL x265 HEVC 10bit EAC3 5.1 Silence)").group.as_deref(), Some("Silence"));
        assert_eq!(parse("Fargo (1996) RM4K (1080p BluRay x265 HEVC 10bit AAC 5.1 Tigole) [QxR]").group.as_deref(), Some("Tigole"));
        // Truncated titles and spec blocks ending in a tag carry no encoder.
        assert_eq!(parse("Fargo (2014) Season 3 S03 (1080p AMZN WEB-DL x265 HEVC 10bit AAC").group, None);
        assert_eq!(parse("Movie (2020) (1080p BluRay x265 10bit AAC 5.1)").group, None);
        assert_eq!(parse("Movie (2020) (1080p BluRay x265 HEVC 10bit EAC3)").group, None);
        assert_eq!(parse("Fargo (1996)").group, None);
    }

    #[test]
    fn web_dl_suffix_is_not_a_group() {
        assert_eq!(parse("Movie 2023 1080p WEB-DL").group, None);
        assert_eq!(parse("Movie 2023 1080p BluRay x264").group, None);
    }

    #[test]
    fn canonicalisers() {
        assert_eq!(canon_resolution("4K"), Some("2160p"));
        assert_eq!(canon_codec("H.265"), Some("x265"));
        assert_eq!(canon_source("WEB DL"), Some("WEB-DL"));
        assert_eq!(canon_source("nonsense"), None);
    }
}
