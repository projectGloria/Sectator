//! HTTP tracker scrape (BEP-48 convention): `/announce` → `/scrape?info_hash=…`,
//! with a minimal bencode decoder for the response.

use std::collections::HashMap;

use super::{ScrapeError, Swarm};

/// Announce URL → scrape URL, if the tracker follows the `/announce` convention.
pub fn scrape_url(announce: &str) -> Option<String> {
    let (head, query) = match announce.split_once('?') {
        Some((h, q)) => (h, Some(q)),
        None => (announce, None),
    };
    if !(head.starts_with("http://") || head.starts_with("https://")) {
        return None;
    }
    let slash = head.rfind('/')?;
    let last = &head[slash + 1..];
    let rest = last.strip_prefix("announce")?;
    let mut out = format!("{}scrape{}", &head[..=slash], rest);
    if let Some(q) = query {
        out.push('?');
        out.push_str(q);
    }
    Some(out)
}

/// Percent-encode raw infohash bytes.
pub fn encode_hash(h: &[u8; 20]) -> String {
    h.iter()
        .map(|b| match b {
            b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z' | b'-' | b'.' | b'_' | b'~' => (*b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

pub fn request_url(scrape: &str, hashes: &[[u8; 20]]) -> String {
    let mut url = scrape.to_string();
    let mut sep = if url.contains('?') { '&' } else { '?' };
    for h in hashes {
        url.push(sep);
        url.push_str("info_hash=");
        url.push_str(&encode_hash(h));
        sep = '&';
    }
    url
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Bencode {
    Int(i64),
    Bytes(Vec<u8>),
    List(Vec<Bencode>),
    Dict(Vec<(Vec<u8>, Bencode)>),
}

impl Bencode {
    pub fn get(&self, key: &[u8]) -> Option<&Bencode> {
        match self {
            Bencode::Dict(d) => d.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn int(&self) -> Option<i64> {
        match self {
            Bencode::Int(i) => Some(*i),
            _ => None,
        }
    }
}

pub fn decode(input: &[u8]) -> Result<Bencode, ScrapeError> {
    let (v, rest) = decode_at(input, 0)?;
    if !rest.is_empty() {
        return Err(ScrapeError::Protocol("trailing bytes after bencode".into()));
    }
    Ok(v)
}

fn bad(msg: &str) -> ScrapeError {
    ScrapeError::Protocol(format!("bencode: {msg}"))
}

fn decode_at(b: &[u8], depth: usize) -> Result<(Bencode, &[u8]), ScrapeError> {
    if depth > 64 {
        return Err(bad("nesting too deep"));
    }
    match b.first().ok_or_else(|| bad("unexpected end"))? {
        b'i' => {
            let end = b.iter().position(|&c| c == b'e').ok_or_else(|| bad("unterminated int"))?;
            let n = std::str::from_utf8(&b[1..end]).ok().and_then(|s| s.parse().ok()).ok_or_else(|| bad("bad int"))?;
            Ok((Bencode::Int(n), &b[end + 1..]))
        }
        b'l' => {
            let mut rest = &b[1..];
            let mut items = Vec::new();
            while rest.first() != Some(&b'e') {
                let (v, r) = decode_at(rest, depth + 1)?;
                items.push(v);
                rest = r;
            }
            Ok((Bencode::List(items), &rest[1..]))
        }
        b'd' => {
            let mut rest = &b[1..];
            let mut items = Vec::new();
            while rest.first() != Some(&b'e') {
                let (k, r) = decode_at(rest, depth + 1)?;
                let Bencode::Bytes(k) = k else { return Err(bad("dict key must be a string")) };
                let (v, r) = decode_at(r, depth + 1)?;
                items.push((k, v));
                rest = r;
            }
            Ok((Bencode::Dict(items), &rest[1..]))
        }
        b'0'..=b'9' => {
            let colon = b.iter().position(|&c| c == b':').ok_or_else(|| bad("missing ':'"))?;
            let len: usize = std::str::from_utf8(&b[..colon]).ok().and_then(|s| s.parse().ok()).ok_or_else(|| bad("bad length"))?;
            let start = colon + 1;
            let end = start.checked_add(len).filter(|&e| e <= b.len()).ok_or_else(|| bad("string overruns input"))?;
            Ok((Bencode::Bytes(b[start..end].to_vec()), &b[end..]))
        }
        _ => Err(bad("unexpected byte")),
    }
}

/// Parse a scrape response into swarm counts keyed by raw 20-byte infohash.
pub fn parse_response(body: &[u8]) -> Result<HashMap<[u8; 20], Swarm>, ScrapeError> {
    let root = decode(body)?;
    if let Some(Bencode::Bytes(reason)) = root.get(b"failure reason") {
        return Err(ScrapeError::Tracker(String::from_utf8_lossy(reason).into_owned()));
    }
    let Some(Bencode::Dict(files)) = root.get(b"files") else {
        return Err(ScrapeError::Protocol("no `files` dict".into()));
    };
    let n = |v: &Bencode, k: &[u8]| v.get(k).and_then(Bencode::int).unwrap_or(0).clamp(0, u32::MAX as i64) as u32;
    Ok(files
        .iter()
        .filter_map(|(k, v)| {
            let key: [u8; 20] = k.as_slice().try_into().ok()?;
            Some((key, Swarm { seeders: n(v, b"complete"), leechers: n(v, b"incomplete"), completed: n(v, b"downloaded") }))
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn announce_to_scrape() {
        assert_eq!(scrape_url("http://t.example/announce").as_deref(), Some("http://t.example/scrape"));
        assert_eq!(scrape_url("https://t.example/x/announce.php?pk=1").as_deref(), Some("https://t.example/x/scrape.php?pk=1"));
        assert_eq!(scrape_url("http://t.example/a"), None);
        assert_eq!(scrape_url("udp://t.example:80/announce"), None);
    }

    #[test]
    fn builds_request() {
        let mut h = [0u8; 20];
        h[0] = 0xAB;
        h[1] = b'z';
        let u = request_url("http://t/scrape?pk=1", &[h]);
        assert!(u.starts_with("http://t/scrape?pk=1&info_hash=%ABz%00"));
    }

    #[test]
    fn decodes_scrape_response() {
        let key = [7u8; 20];
        let mut body = b"d5:filesd20:".to_vec();
        body.extend_from_slice(&key);
        body.extend_from_slice(b"d8:completei12e10:downloadedi340e10:incompletei5eeee");
        let m = parse_response(&body).unwrap();
        assert_eq!(m[&key], Swarm { seeders: 12, leechers: 5, completed: 340 });

        let fail = parse_response(b"d14:failure reason9:forbiddene");
        assert!(matches!(fail, Err(ScrapeError::Tracker(r)) if r == "forbidden"));
        assert!(decode(b"d3:abc").is_err());
        assert!(decode(b"5:ab").is_err());
        assert_eq!(decode(b"li1ei-2ee").unwrap(), Bencode::List(vec![Bencode::Int(1), Bencode::Int(-2)]));
    }
}
