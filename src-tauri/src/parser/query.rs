//! Power-user query DSL: the structured form of what the user typed.
//!
//! Grammar (whitespace separated, order-independent):
//! `term`, `"exact phrase"`, `-term`, `-"phrase"`, `file:<pattern>`, `cat:<category>`,
//! `res:<1080p|4k|..>`, `codec:<hevc|x264|..>`, `src:<remux|web-dl|..>`, `seeds:>=N`,
//! `size:>2gb` / `size:<50gb`, `site:<indexer>`, `source:<curated|dht|torznab>`,
//! `by:<uploader|group|crew>` (aliases `uploader:`, `group:`).
//! Unknown operators or invalid values fall back to plain terms, so nothing typed is lost.

use serde::{Deserialize, Serialize};

use crate::models::{Category, SourceType};
use crate::parser::release;

/// Comparison used by `seeds:` and `size:` operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Cmp {
    Gt,
    Ge,
    Lt,
    Le,
    Eq,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NumFilter {
    pub cmp: Cmp,
    pub value: u64,
}

impl NumFilter {
    pub fn accepts(&self, v: u64) -> bool {
        match self.cmp {
            Cmp::Gt => v > self.value,
            Cmp::Ge => v >= self.value,
            Cmp::Lt => v < self.value,
            Cmp::Le => v <= self.value,
            Cmp::Eq => v == self.value,
        }
    }
}

/// Parsed search: free terms plus every recognised operator.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchQuery {
    /// Plain terms (lowercased), matched as tokens.
    pub terms: Vec<String>,
    /// `"exact phrase"` (lowercased), matched contiguously.
    pub phrases: Vec<String>,
    /// `-term`
    pub excluded: Vec<String>,
    /// `file:<pattern>`
    pub file_patterns: Vec<String>,
    /// `cat:`
    pub category: Option<Category>,
    /// `res:`
    pub resolutions: Vec<String>,
    /// `codec:`
    pub codecs: Vec<String>,
    /// `src:` (release source such as remux / web-dl)
    pub release_sources: Vec<String>,
    /// `seeds:`
    pub seeds: Option<NumFilter>,
    /// `size:` (bytes); may hold a lower and an upper bound.
    pub sizes: Vec<NumFilter>,
    /// `site:`
    pub sites: Vec<String>,
    /// `source:`
    pub source_types: Vec<SourceType>,
    /// `by:` uploader, release group or crew (any of them)
    #[serde(default)]
    pub by: Vec<String>,
}

const KB: f64 = 1024.0;

impl SearchQuery {
    /// Parse the raw search-bar text into a structured query.
    pub fn parse(raw: &str) -> Self {
        let mut q = SearchQuery::default();
        for tok in tokenize(raw) {
            q.apply(tok);
        }
        q
    }

    fn apply(&mut self, tok: Token) {
        let text = tok.text.trim();
        if text.is_empty() {
            return;
        }
        if tok.quoted {
            let text = collapse_ws(&text.to_lowercase());
            if tok.negated {
                push_unique(&mut self.excluded, text);
            } else {
                push_unique(&mut self.phrases, text);
            }
            return;
        }
        if tok.negated {
            push_unique(&mut self.excluded, text.to_lowercase());
            return;
        }
        if let Some((key, value)) = text.split_once(':') {
            if !value.is_empty() && self.apply_operator(&key.to_ascii_lowercase(), value) {
                return;
            }
        }
        push_unique(&mut self.terms, text.to_lowercase());
    }

    /// Returns false when the operator is unknown or its value is invalid,
    /// in which case the token is treated as a plain term.
    fn apply_operator(&mut self, key: &str, value: &str) -> bool {
        let v = value.trim().to_lowercase();
        match key {
            "file" => push_unique(&mut self.file_patterns, v),
            "cat" | "category" => match Category::parse(&v) {
                Some(c) => self.category = Some(c),
                None => return false,
            },
            "res" | "resolution" => match release::canon_resolution(&v) {
                Some(r) => push_unique(&mut self.resolutions, r.to_lowercase()),
                None => return false,
            },
            "codec" => match release::canon_codec(&v) {
                Some(c) => push_unique(&mut self.codecs, c.to_lowercase()),
                None => return false,
            },
            "src" => match release::canon_source(&v) {
                Some(s) => push_unique(&mut self.release_sources, s.to_lowercase()),
                None => return false,
            },
            "seeds" | "seeders" => match parse_num_filter(&v, false) {
                Some(f) => self.seeds = Some(f),
                None => return false,
            },
            "size" => match parse_num_filter(&v, true) {
                Some(f) => {
                    self.sizes.retain(|s| !same_direction(s.cmp, f.cmp));
                    self.sizes.push(f);
                }
                None => return false,
            },
            "site" => push_unique(&mut self.sites, v),
            "by" | "uploader" | "group" => push_unique(&mut self.by, v),
            "source" => match SourceType::parse(&v) {
                Some(s) => {
                    if !self.source_types.contains(&s) {
                        self.source_types.push(s);
                    }
                }
                None => return false,
            },
            _ => return false,
        }
        true
    }

    /// Free text sent to remote indexers (operators are applied locally).
    pub fn indexer_text(&self) -> String {
        let mut parts: Vec<&str> = self.terms.iter().map(String::as_str).collect();
        parts.extend(self.phrases.iter().map(String::as_str));
        parts.join(" ")
    }

    pub fn is_empty(&self) -> bool {
        self.terms.is_empty() && self.phrases.is_empty()
    }

    /// Canonical DSL text. `parse(q.to_dsl()) == q` for any parsed query,
    /// which is what keeps filter chips and the search bar in sync.
    pub fn to_dsl(&self) -> String {
        let mut out: Vec<String> = Vec::new();
        out.extend(self.terms.iter().cloned());
        out.extend(self.phrases.iter().map(|p| format!("\"{p}\"")));
        for e in &self.excluded {
            if e.chars().any(char::is_whitespace) {
                out.push(format!("-\"{e}\""));
            } else {
                out.push(format!("-{e}"));
            }
        }
        out.extend(self.file_patterns.iter().map(|f| quote_op("file", f)));
        if let Some(c) = self.category {
            out.push(format!("cat:{}", c.as_str()));
        }
        out.extend(self.resolutions.iter().map(|r| format!("res:{r}")));
        out.extend(self.codecs.iter().map(|c| format!("codec:{c}")));
        out.extend(self.release_sources.iter().map(|s| format!("src:{s}")));
        if let Some(s) = self.seeds {
            out.push(format!("seeds:{}{}", cmp_str(s.cmp), s.value));
        }
        for s in &self.sizes {
            out.push(format!("size:{}{}", cmp_str(s.cmp), format_size(s.value)));
        }
        out.extend(self.sites.iter().map(|s| quote_op("site", s)));
        out.extend(self.by.iter().map(|b| quote_op("by", b)));
        for s in &self.source_types {
            let name = match s {
                SourceType::Curated => "curated",
                SourceType::Dht => "dht",
                SourceType::Torznab => "torznab",
            };
            out.push(format!("source:{name}"));
        }
        out.join(" ")
    }

    /// Stable key for the result cache: operators that only filter locally are excluded,
    /// so re-filtering a cached search never re-hits the network.
    pub fn cache_key(&self) -> String {
        let mut terms = self.terms.clone();
        terms.sort();
        let mut phrases = self.phrases.clone();
        phrases.sort();
        format!("{}|{}", terms.join(" "), phrases.join("|"))
    }
}

fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn quote_op(key: &str, value: &str) -> String {
    if value.chars().any(char::is_whitespace) {
        format!("{key}:\"{value}\"")
    } else {
        format!("{key}:{value}")
    }
}

fn push_unique(v: &mut Vec<String>, s: String) {
    if !v.contains(&s) {
        v.push(s);
    }
}

fn same_direction(a: Cmp, b: Cmp) -> bool {
    let up = |c| matches!(c, Cmp::Gt | Cmp::Ge);
    let down = |c| matches!(c, Cmp::Lt | Cmp::Le);
    (up(a) && up(b)) || (down(a) && down(b)) || (a == Cmp::Eq || b == Cmp::Eq)
}

fn cmp_str(c: Cmp) -> &'static str {
    match c {
        Cmp::Gt => ">",
        Cmp::Ge => ">=",
        Cmp::Lt => "<",
        Cmp::Le => "<=",
        Cmp::Eq => "=",
    }
}

/// `>=10`, `>2gb`, `<500mb`, `10` (bare = at least). Unitless sizes are MB.
fn parse_num_filter(s: &str, is_size: bool) -> Option<NumFilter> {
    let (cmp, rest) = if let Some(r) = s.strip_prefix(">=") {
        (Cmp::Ge, r)
    } else if let Some(r) = s.strip_prefix("<=") {
        (Cmp::Le, r)
    } else if let Some(r) = s.strip_prefix('>') {
        (Cmp::Gt, r)
    } else if let Some(r) = s.strip_prefix('<') {
        (Cmp::Lt, r)
    } else if let Some(r) = s.strip_prefix('=') {
        (Cmp::Eq, r)
    } else {
        (Cmp::Ge, s)
    };
    let value = if is_size { parse_size(rest)? } else { rest.trim().parse().ok()? };
    Some(NumFilter { cmp, value })
}

/// Parse a human size (`2gb`, `1.5 GiB`, `700m`, `500`) into bytes (binary units).
/// Unitless numbers are megabytes; a bare `b` suffix means bytes.
pub fn parse_size(s: &str) -> Option<u64> {
    let s = s.trim().to_ascii_lowercase();
    let split = s.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(s.len());
    let (num, unit) = s.split_at(split);
    let n: f64 = num.parse().ok()?;
    if !n.is_finite() || n < 0.0 {
        return None;
    }
    let unit = unit.trim();
    let mult = match unit {
        "" => KB * KB,
        "b" | "bytes" => 1.0,
        "k" | "kb" | "kib" => KB,
        "m" | "mb" | "mib" => KB * KB,
        "g" | "gb" | "gib" => KB * KB * KB,
        "t" | "tb" | "tib" => KB * KB * KB * KB,
        _ => return None,
    };
    Some((n * mult).round() as u64)
}

/// Shortest exact unit representation, e.g. `2gb`, `500mb`, `1536mb`.
pub fn format_size(bytes: u64) -> String {
    let units = [("tb", 1u64 << 40), ("gb", 1 << 30), ("mb", 1 << 20), ("kb", 1 << 10)];
    for (name, size) in units {
        if bytes >= size && bytes % size == 0 {
            return format!("{}{name}", bytes / size);
        }
    }
    format!("{bytes}b")
}

struct Token {
    text: String,
    quoted: bool,
    negated: bool,
}

/// Whitespace tokenizer that keeps `"quoted phrases"`, `-"negated phrases"`
/// and `key:"quoted values"` intact. Unterminated quotes run to the end of input.
fn tokenize(raw: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let chars: Vec<char> = raw.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_whitespace() {
            i += 1;
            continue;
        }
        let negated = chars[i] == '-' && i + 1 < chars.len() && !chars[i + 1].is_whitespace();
        let start = if negated { i + 1 } else { i };
        if chars[start] == '"' {
            let mut j = start + 1;
            while j < chars.len() && chars[j] != '"' {
                j += 1;
            }
            out.push(Token { text: chars[start + 1..j].iter().collect(), quoted: true, negated });
            i = j + 1;
            continue;
        }
        let mut j = start;
        let mut text = String::new();
        while j < chars.len() && !chars[j].is_whitespace() {
            if chars[j] == '"' {
                j += 1;
                while j < chars.len() && chars[j] != '"' {
                    text.push(chars[j]);
                    j += 1;
                }
                j += 1;
                continue;
            }
            text.push(chars[j]);
            j += 1;
        }
        out.push(Token { text, quoted: false, negated });
        i = j;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_dsl() {
        let q = SearchQuery::parse(
            r#"dune "part two" -cam -"hindi dub" file:.mkv cat:movies res:4k codec:hevc src:web-dl seeds:>=10 size:>2gb size:<50gb site:nyaa source:dht"#,
        );
        assert_eq!(q.terms, vec!["dune"]);
        assert_eq!(q.phrases, vec!["part two"]);
        assert_eq!(q.excluded, vec!["cam", "hindi dub"]);
        assert_eq!(q.file_patterns, vec![".mkv"]);
        assert_eq!(q.category, Some(Category::Movies));
        assert_eq!(q.resolutions, vec!["2160p"]);
        assert_eq!(q.codecs, vec!["x265"]);
        assert_eq!(q.release_sources, vec!["web-dl"]);
        assert_eq!(q.seeds, Some(NumFilter { cmp: Cmp::Ge, value: 10 }));
        assert_eq!(q.sizes.len(), 2);
        assert!(q.sizes[0].accepts(3 << 30) && !q.sizes[0].accepts(1 << 30));
        assert!(q.sizes[1].accepts(10 << 30) && !q.sizes[1].accepts(60 << 30));
        assert_eq!(q.sites, vec!["nyaa"]);
        assert_eq!(q.source_types, vec![SourceType::Dht]);
        assert_eq!(q.indexer_text(), "dune part two");
    }

    #[test]
    fn unknown_or_invalid_operators_become_terms() {
        let q = SearchQuery::parse("re:zero cat:nonsense seeds:lots");
        assert_eq!(q.terms, vec!["re:zero", "cat:nonsense", "seeds:lots"]);
        assert!(q.category.is_none() && q.seeds.is_none());
    }

    #[test]
    fn dsl_round_trips() {
        for raw in [
            r#"the office "season 3" -cam file:"sub title.srt" cat:tv res:1080p seeds:>5 size:<=1536mb source:torznab source:curated"#,
            "ubuntu -arm size:>=700mb site:apibay codec:av1 src:remux",
            "",
        ] {
            let q = SearchQuery::parse(raw);
            assert_eq!(SearchQuery::parse(&q.to_dsl()), q, "round trip for {raw:?} via {:?}", q.to_dsl());
        }
    }

    #[test]
    fn later_size_bound_in_same_direction_replaces_earlier() {
        let q = SearchQuery::parse("x size:>1gb size:>=2gb");
        assert_eq!(q.sizes, vec![NumFilter { cmp: Cmp::Ge, value: 2 << 30 }]);
    }

    #[test]
    fn sizes_parse_with_units() {
        assert_eq!(parse_size("2gb"), Some(2 << 30));
        assert_eq!(parse_size("1.5GiB"), Some(1610612736));
        assert_eq!(parse_size("500"), Some(500 << 20));
        assert_eq!(parse_size("800kb"), Some(800 << 10));
        assert_eq!(parse_size("12b"), Some(12));
        assert_eq!(parse_size("abc"), None);
        assert_eq!(parse_size("5xb"), None);
        assert_eq!(format_size(1536 << 20), "1536mb");
        assert_eq!(format_size(2 << 30), "2gb");
        assert_eq!(format_size(1000), "1000b");
    }

    #[test]
    fn lone_dash_and_unterminated_quote_are_safe() {
        let q = SearchQuery::parse(r#"a - "unterminated"#);
        assert_eq!(q.terms, vec!["a", "-"]);
        assert_eq!(q.phrases, vec!["unterminated"]);
    }

    #[test]
    fn cache_key_ignores_local_filters() {
        let a = SearchQuery::parse("dune part res:1080p -cam");
        let b = SearchQuery::parse("part dune seeds:>5");
        assert_eq!(a.cache_key(), b.cache_key());
    }

    #[test]
    fn by_filters_locally_and_round_trips() {
        let q = SearchQuery::parse("fargo by:QxR uploader:Cybotage group:hhweb");
        assert_eq!(q.by, vec!["qxr", "cybotage", "hhweb"]);
        assert_eq!(q.indexer_text(), "fargo");
        assert_eq!(q.cache_key(), SearchQuery::parse("fargo").cache_key());
        assert_eq!(SearchQuery::parse(&q.to_dsl()), q);
    }
}
