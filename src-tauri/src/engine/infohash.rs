//! InfoHash normalisation and magnet parsing/building.
//!
//! Every hash is reduced to a 40-char lowercase hex BTIH before dedup:
//! - 40 hex chars (any case) → lowercased
//! - 32 Base32 chars (RFC 4648, the older magnet form) → decoded to hex
//! - BitTorrent v2 `urn:btmh:1220<64 hex>` (SHA-256 multihash) → truncated to the first
//!   20 bytes, which is exactly the key v2/hybrid swarms announce under on trackers and DHT (BEP 52).

use url::form_urlencoded;

/// Normalise any supported infohash spelling (with or without `urn:btih:` / `urn:btmh:`).
pub fn normalize(raw: &str) -> Option<String> {
    let s = raw.trim();
    let lower = s.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("urn:btmh:") {
        return from_multihash(rest);
    }
    let s = if lower.starts_with("urn:btih:") { &s[9..] } else { s };
    match s.len() {
        40 if s.bytes().all(|b| b.is_ascii_hexdigit()) => Some(s.to_ascii_lowercase()),
        32 => base32_decode(s).map(|b| hex(&b)),
        // A bare v2 multihash or raw SHA-256 hex.
        68 if s.to_ascii_lowercase().starts_with("1220") => from_multihash(&s.to_ascii_lowercase()),
        64 if s.bytes().all(|b| b.is_ascii_hexdigit()) => Some(s[..40].to_ascii_lowercase()),
        _ => None,
    }
}

/// `1220` = SHA-256 code + 32-byte length; keep the first 20 bytes (40 hex chars).
fn from_multihash(mh: &str) -> Option<String> {
    let digest = mh.strip_prefix("1220")?;
    if digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit()) {
        Some(digest[..40].to_ascii_lowercase())
    } else {
        None
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// RFC 4648 Base32 (no padding) → 20 bytes.
fn base32_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(20);
    let mut buf: u64 = 0;
    let mut bits = 0u32;
    for c in s.bytes() {
        let v = match c.to_ascii_uppercase() {
            b @ b'A'..=b'Z' => b - b'A',
            b @ b'2'..=b'7' => b - b'2' + 26,
            _ => return None,
        };
        buf = (buf << 5) | u64::from(v);
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    (out.len() == 20).then_some(out)
}

/// What a magnet link carries.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Magnet {
    pub info_hash: String,
    pub name: Option<String>,
    pub trackers: Vec<String>,
    pub size: Option<u64>,
}

/// Parse a magnet URI. A v1 `btih` wins over a v2 `btmh` when both are present (hybrid).
pub fn parse_magnet(uri: &str) -> Option<Magnet> {
    let query = uri.trim().strip_prefix("magnet:?").or_else(|| uri.trim().strip_prefix("magnet:"))?;
    let mut m = Magnet::default();
    let mut v2: Option<String> = None;
    for (k, v) in form_urlencoded::parse(query.as_bytes()) {
        let key = k.split('.').next().unwrap_or("");
        match key {
            "xt" => {
                let lower = v.to_ascii_lowercase();
                if lower.starts_with("urn:btih:") {
                    if let Some(h) = normalize(&v) {
                        m.info_hash = h;
                    }
                } else if lower.starts_with("urn:btmh:") {
                    v2 = normalize(&v);
                }
            }
            "dn" => m.name = Some(v.into_owned()),
            "tr" => {
                let t = v.trim().to_string();
                if !t.is_empty() && !m.trackers.contains(&t) {
                    m.trackers.push(t);
                }
            }
            "xl" => m.size = v.parse().ok(),
            _ => {}
        }
    }
    if m.info_hash.is_empty() {
        m.info_hash = v2?;
    }
    Some(m)
}

/// Build a magnet URI with `dn` and every tracker (percent-encoded).
pub fn build_magnet(info_hash: &str, name: &str, trackers: &[String]) -> String {
    let mut s = format!("magnet:?xt=urn:btih:{info_hash}");
    if !name.is_empty() {
        s.push_str("&dn=");
        s.extend(form_urlencoded::byte_serialize(name.as_bytes()));
    }
    for t in trackers {
        s.push_str("&tr=");
        s.extend(form_urlencoded::byte_serialize(t.as_bytes()));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEX: &str = "dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c";

    #[test]
    fn hex_is_lowercased() {
        assert_eq!(normalize("DD8255ECDC7CA55FB0BBF81323D87062DB1F6D1C").as_deref(), Some(HEX));
        assert_eq!(normalize("urn:btih:DD8255ECDC7CA55FB0BBF81323D87062DB1F6D1C").as_deref(), Some(HEX));
    }

    #[test]
    fn base32_decodes_to_the_same_hash() {
        // base32 of the 20 bytes above
        assert_eq!(normalize("3WBFL3G4PSSV7MF37AJSHWDQMLNR63I4").as_deref(), Some(HEX));
        assert_eq!(normalize("3wbfl3g4pssv7mf37ajshwdqmlnr63i4").as_deref(), Some(HEX));
        assert_eq!(normalize("1WBFL3G4PSSV7MF37AJSHWDQMLNR63I4"), None);
    }

    #[test]
    fn v2_multihash_truncates_to_btih_key() {
        let digest = "caf1e1c30e81cb361b9ee167c4aa64228a7fa4fa9f6105232b28ad099f3a302e";
        assert_eq!(normalize(&format!("urn:btmh:1220{digest}")).as_deref(), Some(&digest[..40]));
        assert_eq!(normalize(digest).as_deref(), Some(&digest[..40]));
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(normalize("not-a-hash"), None);
        assert_eq!(normalize("zz8255ecdc7ca55fb0bbf81323d87062db1f6d1c"), None);
        assert_eq!(normalize(""), None);
    }

    #[test]
    fn magnet_round_trip() {
        let uri = "magnet:?xt=urn:btih:3WBFL3G4PSSV7MF37AJSHWDQMLNR63I4&dn=Big+Buck+Bunny&tr=udp%3A%2F%2Ftracker.opentrackr.org%3A1337%2Fannounce&tr=udp%3A%2F%2Ftracker.opentrackr.org%3A1337%2Fannounce&xl=276445467";
        let m = parse_magnet(uri).unwrap();
        assert_eq!(m.info_hash, HEX);
        assert_eq!(m.name.as_deref(), Some("Big Buck Bunny"));
        assert_eq!(m.trackers, vec!["udp://tracker.opentrackr.org:1337/announce"]);
        assert_eq!(m.size, Some(276445467));
        let rebuilt = build_magnet(&m.info_hash, "Big Buck Bunny", &m.trackers);
        let again = parse_magnet(&rebuilt).unwrap();
        assert_eq!(again.info_hash, HEX);
        assert_eq!(again.trackers, m.trackers);
        assert_eq!(again.name, m.name);
    }

    #[test]
    fn hybrid_magnet_prefers_v1() {
        let digest = "caf1e1c30e81cb361b9ee167c4aa64228a7fa4fa9f6105232b28ad099f3a302e";
        let hybrid = format!("magnet:?xt=urn:btmh:1220{digest}&xt=urn:btih:{HEX}");
        assert_eq!(parse_magnet(&hybrid).unwrap().info_hash, HEX);
        let v2only = format!("magnet:?xt=urn:btmh:1220{digest}");
        assert_eq!(parse_magnet(&v2only).unwrap().info_hash, &digest[..40]);
        assert!(parse_magnet("magnet:?dn=nothing").is_none());
    }
}
