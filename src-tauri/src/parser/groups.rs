//! `by:` matching: an uploader account, a release group, or a bracketed site tag.
//!
//! Some release crews never put their own name in titles: QxR encodes end in the
//! encoder's name (`… 10bit AAC 5.1 Tigole)`), so `by:qxr` also accepts its encoders.

use once_cell::sync::Lazy;
use regex::Regex;

/// Crews whose releases are tagged by member rather than by crew name.
const CREWS: &[(&str, &[&str])] = &[(
    "qxr",
    &[
        "tigole", "joy", "silence", "ghost", "bandi", "freethefish", "nickarad", "panda", "langbard", "ime",
        "t3nzin", "natty", "garshasp", "r00t", "rcvr", "afm72", "vyndros",
    ],
)];

static BRACKET_TAG: Lazy<Regex> = Lazy::new(|| Regex::new(r"\[([^\]\[]{1,24})\]").expect("valid regex"));

/// The lowercase names a `by:` value accepts: itself plus any crew members.
pub fn aliases(name: &str) -> Vec<String> {
    let name = name.trim().to_lowercase();
    let mut out = vec![name.clone()];
    if let Some((_, members)) = CREWS.iter().find(|(crew, _)| *crew == name) {
        out.extend(members.iter().map(|m| m.to_string()));
    }
    out
}

/// Whether a release was put out by `name` (case-insensitive, whole-name match).
pub fn matches(name: &str, uploader: Option<&str>, group: Option<&str>, title: &str) -> bool {
    let names = aliases(name);
    let hit = |s: &str| names.iter().any(|n| n.eq_ignore_ascii_case(s.trim()));
    uploader.is_some_and(hit) || group.is_some_and(hit) || BRACKET_TAG.captures_iter(title).any(|c| hit(&c[1]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_uploader_group_and_bracket_tag() {
        assert!(matches("cybotage", Some("Cybotage"), None, "Anything"));
        assert!(matches("hhweb", Some("TvTeam"), Some("HHWEB"), "Fargo S04E01 2014 1080p NF WEB-DL DDP5 1 H 264-HHWEB"));
        assert!(matches("tgx", None, Some("NTb"), "Fargo.S05.COMPLETE.1080p.AMZN.WEB-DL.DDP5.1.H.264-NTb[TGx]"));
        assert!(!matches("tgx", Some("GalaxyRG"), Some("GalaxyTV"), "Fargo.S02.COMPLETE.720p.BluRay.x264-GalaxyTV"));
    }

    #[test]
    fn crew_name_accepts_its_encoders_only() {
        assert!(matches("qxr", None, Some("Silence"), "Fargo (2014) Season 5 S05 (1080p AMZN WEB-DL x265 HEVC 10bit EAC3 5.1 Silence)"));
        assert!(matches("QxR", None, None, "Some Show S01 (1080p BluRay x265 HEVC 10bit AAC 5.1 Joy) [QxR]"));
        assert!(!matches("qxr", Some("Cybotage"), Some("TOMMY"), "Fargo.S04.1080p.AMZN.WEBRip.DDP5.1.x264-TOMMY"));
        // A member name only matches as a group or tag, never as a stray title word.
        assert!(!matches("qxr", None, None, "Joy (2015) 1080p BluRay x264-SPARKS"));
        assert_eq!(aliases("tigole"), vec!["tigole"]);
    }
}
