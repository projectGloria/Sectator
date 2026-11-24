//! Small text helpers shared by adapters: HTML entity decoding, dates, URL encoding.

use chrono::DateTime;

/// Decode the HTML entities indexers actually emit (named basics plus numeric refs).
pub fn html_unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let end = rest[1..].find(|c: char| c == ';' || c == '&' || c.is_whitespace()).map(|j| j + 1);
        let decoded = match end {
            Some(j) if rest.as_bytes()[j] == b';' => decode_entity(&rest[1..j]).map(|c| (c, j + 1)),
            _ => None,
        };
        match decoded {
            Some((c, len)) => {
                out.push(c);
                rest = &rest[len..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn decode_entity(name: &str) -> Option<char> {
    if let Some(num) = name.strip_prefix('#') {
        let code = match num.strip_prefix(['x', 'X']) {
            Some(hex) => u32::from_str_radix(hex, 16).ok()?,
            None => num.parse().ok()?,
        };
        return char::from_u32(code);
    }
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{a0}',
        _ => return None,
    })
}

/// RFC 2822 (`pubDate`) → unix seconds. Tolerates a missing space after the weekday comma.
pub fn parse_rfc2822(s: &str) -> Option<i64> {
    let s = s.trim();
    let fixed;
    let s = match s.find(',') {
        Some(i) if s[i + 1..].chars().next().is_some_and(|c| !c.is_whitespace()) => {
            fixed = format!("{}, {}", &s[..i], &s[i + 1..]);
            fixed.as_str()
        }
        _ => s,
    };
    DateTime::parse_from_rfc2822(s).ok().map(|d| d.timestamp())
}

pub fn parse_rfc3339(s: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(s.trim()).ok().map(|d| d.timestamp())
}

/// Percent-encode one query-string value.
pub fn enc(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

/// Trim a base URL's trailing slashes.
pub fn base(s: &str) -> &str {
    s.trim_end_matches('/')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unescapes_entities() {
        assert_eq!(html_unescape("Linux &amp; Scripting"), "Linux & Scripting");
        assert_eq!(html_unescape("&#34;q&#34; &#x27;x&#x27; &lt;b&gt;"), "\"q\" 'x' <b>");
        assert_eq!(html_unescape("AT&T & co &bogus; &"), "AT&T & co &bogus; &");
    }

    #[test]
    fn parses_dates() {
        assert_eq!(parse_rfc2822("Fri, 30 Oct 2026 15:32:00 -0000"), Some(1_793_374_320));
        // BT4G omits the space after the comma.
        assert_eq!(parse_rfc2822("Mon,23 Nov 2026 13:08:47 -0000"), parse_rfc2822("Mon, 23 Nov 2026 13:08:47 +0000"));
        assert_eq!(parse_rfc3339("2026-04-25T15:35:00+00:00"), Some(1_777_131_300));
        assert_eq!(parse_rfc2822("garbage"), None);
    }
}
