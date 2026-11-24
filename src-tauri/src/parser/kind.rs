//! What a torrent's content is (video, game, music…), worked out from its file list and
//! name, so the UI can show a fitting icon. The extension holding the most bytes also
//! picks the Windows shell icon (the user's default app for that type).

use once_cell::sync::Lazy;
use regex::Regex;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ContentKind {
    Video,
    Audio,
    Game,
    Software,
    Book,
    Image,
    Archive,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Content {
    pub kind: ContentKind,
    /// Lowercase extension (no dot) holding the most bytes; picks the shell icon.
    pub main_ext: Option<String>,
}

const VIDEO: &[&str] = &["mkv", "mp4", "m4v", "avi", "mov", "wmv", "webm", "ts", "m2ts", "mpg", "mpeg", "flv", "vob", "ogv", "3gp", "rmvb"];
const AUDIO: &[&str] = &["mp3", "flac", "m4a", "aac", "ogg", "opus", "wav", "wma", "alac", "ape", "dsf", "dff", "wv", "mka"];
/// Console ROMs and disc images that are (almost) only ever games.
const GAME: &[&str] = &[
    "cue", "chd", "gdi", "cdi", "cso", "pbp", "nsp", "nsz", "xci", "3ds", "cia", "nds", "gba", "gbc", "gb", "sfc", "smc", "nes", "n64", "z64",
    "v64", "wbfs", "rvz", "wua", "wud", "xex", "vpk", "md", "gen", "sms", "gg", "pce", "a26", "lnx", "ws", "wsc", "ngp",
];
const SOFTWARE: &[&str] = &["exe", "msi", "msix", "appx", "dmg", "pkg", "deb", "rpm", "apk", "appimage"];
const BOOK: &[&str] = &["epub", "pdf", "mobi", "azw3", "azw", "djvu", "cbz", "cbr", "fb2"];
const IMAGE: &[&str] = &["jpg", "jpeg", "png", "gif", "webp", "tif", "tiff", "bmp", "raw", "heic"];
const ARCHIVE: &[&str] = &["zip", "rar", "7z", "tar", "gz", "xz", "bz2", "zst"];
/// Generic disc images: a game when the name looks like one, otherwise software.
const DISC: &[&str] = &["iso", "bin", "img", "mdf", "nrg"];

/// Release names that mark a game: No-Intro/Redump region tags, scene/repack groups.
static GAME_NAME: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)\((?:europe|usa|japan|world|asia|korea|germany|france|spain|italy|en(?:,[a-z]{2})+)\b|\b(?:fitgirl|dodi|elamigos|codex|skidrow|plaza|razor1911|reloaded|empress|tenoke|rune|flt|gog|kaos|xatab)\b|\b(?:ps[1-5x]|psp|xbox(?:360)?|gamecube|ngc|wii(?:u)?|switch|nintendo|sega|dreamcast|saturn|playstation)\b",
    )
    .unwrap()
});

fn ext_of(path: &str) -> Option<String> {
    let file = path.rsplit(['/', '\\']).next()?;
    let (stem, ext) = file.rsplit_once('.')?;
    (!stem.is_empty() && !ext.is_empty() && ext.len() <= 9 && ext.chars().all(|c| c.is_ascii_alphanumeric())).then(|| ext.to_ascii_lowercase())
}

fn kind_of_ext(ext: &str) -> Option<ContentKind> {
    let has = |list: &[&str]| list.contains(&ext);
    Some(if has(VIDEO) {
        ContentKind::Video
    } else if has(AUDIO) {
        ContentKind::Audio
    } else if has(GAME) {
        ContentKind::Game
    } else if has(SOFTWARE) {
        ContentKind::Software
    } else if has(BOOK) {
        ContentKind::Book
    } else if has(IMAGE) {
        ContentKind::Image
    } else if has(ARCHIVE) {
        ContentKind::Archive
    } else {
        return None;
    })
}

/// Classify a torrent from its name and `(path, size)` file list (empty before metadata).
pub fn classify<'a>(name: &str, files: impl IntoIterator<Item = (&'a str, i64)>) -> Content {
    // Bytes per extension, so a season of episodes beats one big sample or extras file.
    let mut by_ext: Vec<(String, i64)> = Vec::new();
    for (path, size) in files {
        if let Some(e) = ext_of(path) {
            match by_ext.iter_mut().find(|(x, _)| *x == e) {
                Some((_, n)) => *n += size,
                None => by_ext.push((e, size)),
            }
        }
    }
    by_ext.sort_by(|a, b| b.1.cmp(&a.1));
    // Before metadata, a single-file torrent's name is its file name.
    let main_ext = by_ext.first().map(|(e, _)| e.clone()).or_else(|| ext_of(name).filter(|e| kind_of_ext(e).is_some()));
    let gamey = GAME_NAME.is_match(name);
    let has = |list: &[&str]| by_ext.iter().any(|(e, _)| list.contains(&e.as_str()));

    let kind = if has(GAME) {
        ContentKind::Game
    } else if gamey && (has(DISC) || has(SOFTWARE) || has(ARCHIVE) || by_ext.is_empty()) {
        // Redump sets, PC repacks (setup.exe + .bin parts), zipped ROMs.
        ContentKind::Game
    } else {
        by_ext
            .iter()
            .find_map(|(e, _)| kind_of_ext(e).or_else(|| DISC.contains(&e.as_str()).then_some(ContentKind::Software)))
            .or_else(|| main_ext.as_deref().and_then(kind_of_ext))
            .unwrap_or(ContentKind::Other)
    };
    Content { kind, main_ext }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn video_release() {
        let c = classify(
            "War Dogs (2016) (1080p BluRay x265 HEVC 10bit AAC 5.1 Tigole)",
            [("War Dogs (2016)/War Dogs (2016).mkv", 4_700_000_000), ("War Dogs (2016)/info.nfo", 2_000)],
        );
        assert_eq!(c, Content { kind: ContentKind::Video, main_ext: Some("mkv".into()) });
    }

    #[test]
    fn redump_game_with_bin_cue() {
        let name = "Starsky & Hutch (Europe) (En,Fr,De,Es,It,Pl,Ru) (Rerelease)";
        let c = classify(name, [(&*format!("{name}/{name}.bin"), 762_000_000), (&*format!("{name}/{name}.cue"), 125)]);
        assert_eq!(c.kind, ContentKind::Game);
        assert_eq!(c.main_ext.as_deref(), Some("bin"));
    }

    #[test]
    fn rom_and_pc_repack() {
        assert_eq!(classify("Some Game", [("Some Game.nsp", 5)]).kind, ContentKind::Game);
        let repack = classify("Elden Ring [FitGirl Repack]", [("setup.exe", 5_000_000), ("fg-01.bin", 4_000_000_000)]);
        assert_eq!(repack.kind, ContentKind::Game);
    }

    #[test]
    fn iso_without_game_hints_is_software() {
        assert_eq!(classify("ubuntu-24.04-desktop-amd64", [("ubuntu-24.04-desktop-amd64.iso", 6_000_000_000)]).kind, ContentKind::Software);
    }

    #[test]
    fn biggest_extension_total_wins() {
        let files = [("01.flac", 30), ("02.flac", 30), ("03.flac", 30), ("cover.jpg", 50)];
        assert_eq!(classify("Album", files).kind, ContentKind::Audio);
        assert_eq!(classify("Album", files).main_ext.as_deref(), Some("flac"), "not the single biggest file");
    }

    #[test]
    fn before_metadata_uses_the_name() {
        assert_eq!(classify("Movie.2020.1080p.mkv", []), Content { kind: ContentKind::Video, main_ext: Some("mkv".into()) });
        assert_eq!(classify("Movie 2020 1080p", []), Content { kind: ContentKind::Other, main_ext: None });
        assert_eq!(classify("Movie.2020.1080p.BluRay", []).main_ext, None, "not a real extension");
        assert_eq!(classify("Crash Bandicoot (USA)", []).kind, ContentKind::Game);
    }
}
