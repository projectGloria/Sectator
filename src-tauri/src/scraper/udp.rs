//! BEP-15 UDP tracker scrape: connect (action 0) then scrape (action 2).
//! Encoding/decoding are pure functions; `scrape` drives one tracker over a socket.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use tokio::net::UdpSocket;
use tokio::time::timeout;

use super::{ScrapeError, Swarm};

pub const PROTOCOL_ID: u64 = 0x0417_2710_1980;
pub const ACTION_CONNECT: u32 = 0;
pub const ACTION_SCRAPE: u32 = 2;
pub const ACTION_ERROR: u32 = 3;
/// A scrape packet must fit in a single datagram; trackers accept ~74 hashes.
pub const MAX_HASHES: usize = 74;

static TX_SEED: AtomicU32 = AtomicU32::new(0);

/// Unpredictable-enough transaction id without pulling in a RNG crate.
pub fn transaction_id() -> u32 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let mut x = nanos ^ TX_SEED.fetch_add(0x9E37_79B9, Ordering::Relaxed);
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    x
}

pub fn encode_connect(tx: u32) -> [u8; 16] {
    let mut b = [0u8; 16];
    b[..8].copy_from_slice(&PROTOCOL_ID.to_be_bytes());
    b[8..12].copy_from_slice(&ACTION_CONNECT.to_be_bytes());
    b[12..].copy_from_slice(&tx.to_be_bytes());
    b
}

fn be32(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn check_header(b: &[u8], tx: u32, action: u32) -> Result<(), ScrapeError> {
    if b.len() < 8 {
        return Err(ScrapeError::Protocol("short packet".into()));
    }
    if be32(b, 4) != tx {
        return Err(ScrapeError::Protocol("transaction id mismatch".into()));
    }
    match be32(b, 0) {
        a if a == action => Ok(()),
        ACTION_ERROR => Err(ScrapeError::Tracker(String::from_utf8_lossy(&b[8..]).into_owned())),
        a => Err(ScrapeError::Protocol(format!("unexpected action {a}"))),
    }
}

pub fn decode_connect(b: &[u8], tx: u32) -> Result<u64, ScrapeError> {
    check_header(b, tx, ACTION_CONNECT)?;
    if b.len() < 16 {
        return Err(ScrapeError::Protocol("short connect response".into()));
    }
    Ok(u64::from_be_bytes(b[8..16].try_into().unwrap()))
}

/// `hashes` are 40-char hex infohashes; invalid ones are rejected.
pub fn encode_scrape(conn: u64, tx: u32, hashes: &[String]) -> Result<Vec<u8>, ScrapeError> {
    if hashes.len() > MAX_HASHES {
        return Err(ScrapeError::Protocol(format!("at most {MAX_HASHES} hashes per scrape")));
    }
    let mut b = Vec::with_capacity(16 + 20 * hashes.len());
    b.extend_from_slice(&conn.to_be_bytes());
    b.extend_from_slice(&ACTION_SCRAPE.to_be_bytes());
    b.extend_from_slice(&tx.to_be_bytes());
    for h in hashes {
        b.extend_from_slice(&hex20(h).ok_or_else(|| ScrapeError::Protocol(format!("bad infohash {h}")))?);
    }
    Ok(b)
}

pub fn hex20(h: &str) -> Option<[u8; 20]> {
    if h.len() != 40 {
        return None;
    }
    let mut out = [0u8; 20];
    for (i, chunk) in h.as_bytes().chunks(2).enumerate() {
        out[i] = u8::from_str_radix(std::str::from_utf8(chunk).ok()?, 16).ok()?;
    }
    Some(out)
}

/// Response order matches request order: seeders, completed, leechers per hash.
pub fn decode_scrape(b: &[u8], tx: u32, count: usize) -> Result<Vec<Swarm>, ScrapeError> {
    check_header(b, tx, ACTION_SCRAPE)?;
    let body = &b[8..];
    let n = (body.len() / 12).min(count);
    if n == 0 && count > 0 {
        return Err(ScrapeError::Protocol("empty scrape response".into()));
    }
    Ok((0..n)
        .map(|i| Swarm { seeders: be32(body, i * 12), completed: be32(body, i * 12 + 4), leechers: be32(body, i * 12 + 8) })
        .collect())
}

/// `udp://host:port/announce` → `host:port`.
pub fn host_port(tracker: &str) -> Option<String> {
    let u = url::Url::parse(tracker).ok()?;
    (u.scheme() == "udp").then(|| Some(format!("{}:{}", u.host_str()?, u.port()?)))?
}

async fn roundtrip(sock: &UdpSocket, pkt: &[u8], wait: Duration, buf: &mut [u8]) -> Result<usize, ScrapeError> {
    // Two attempts: UDP packets get dropped.
    for _ in 0..2 {
        sock.send(pkt).await.map_err(|e| ScrapeError::Io(e.to_string()))?;
        if let Ok(r) = timeout(wait, sock.recv(buf)).await {
            return r.map_err(|e| ScrapeError::Io(e.to_string()));
        }
    }
    Err(ScrapeError::Timeout)
}

/// Scrape up to `MAX_HASHES` hashes from one UDP tracker.
pub async fn scrape(tracker: &str, hashes: &[String], wait: Duration) -> Result<Vec<Swarm>, ScrapeError> {
    let target = host_port(tracker).ok_or_else(|| ScrapeError::Protocol(format!("not a udp tracker: {tracker}")))?;
    let addr: SocketAddr = tokio::net::lookup_host(&target)
        .await
        .map_err(|e| ScrapeError::Io(e.to_string()))?
        .next()
        .ok_or_else(|| ScrapeError::Io(format!("cannot resolve {target}")))?;
    let bind = if addr.is_ipv6() { "[::]:0" } else { "0.0.0.0:0" };
    let sock = UdpSocket::bind(bind).await.map_err(|e| ScrapeError::Io(e.to_string()))?;
    sock.connect(addr).await.map_err(|e| ScrapeError::Io(e.to_string()))?;
    let mut buf = vec![0u8; 16 + 12 * MAX_HASHES];

    let tx = transaction_id();
    let n = roundtrip(&sock, &encode_connect(tx), wait, &mut buf).await?;
    let conn = decode_connect(&buf[..n], tx)?;

    let tx = transaction_id();
    let pkt = encode_scrape(conn, tx, hashes)?;
    let n = roundtrip(&sock, &pkt, wait, &mut buf).await?;
    decode_scrape(&buf[..n], tx, hashes.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    const H1: &str = "2c603fb78071e4933ca5c95bd1a77060053ff988";
    const H2: &str = "444d6b7665bf616065d4c3793642bc41c17e767e";

    #[test]
    fn packet_round_trip() {
        let c = encode_connect(7);
        assert_eq!(&c[..8], &[0, 0, 4, 0x17, 0x27, 0x10, 0x19, 0x80]);
        assert_eq!(be32(&c, 12), 7);

        let mut resp = Vec::new();
        resp.extend_from_slice(&0u32.to_be_bytes());
        resp.extend_from_slice(&7u32.to_be_bytes());
        resp.extend_from_slice(&0xDEAD_BEEF_u64.to_be_bytes());
        assert_eq!(decode_connect(&resp, 7).unwrap(), 0xDEAD_BEEF);
        assert!(decode_connect(&resp, 8).is_err());

        let s = encode_scrape(5, 9, &[H1.into(), H2.into()]).unwrap();
        assert_eq!(s.len(), 16 + 40);
        assert_eq!(&s[16..36], &hex20(H1).unwrap());
        assert!(encode_scrape(5, 9, &["zz".into()]).is_err());
        assert!(encode_scrape(5, 9, &vec![H1.to_string(); MAX_HASHES + 1]).is_err());

        let mut err = Vec::new();
        err.extend_from_slice(&3u32.to_be_bytes());
        err.extend_from_slice(&9u32.to_be_bytes());
        err.extend_from_slice(b"torrent not registered");
        assert!(matches!(decode_scrape(&err, 9, 1), Err(ScrapeError::Tracker(m)) if m.contains("not registered")));
    }

    #[test]
    fn host_port_only_for_udp() {
        assert_eq!(host_port("udp://tracker.opentrackr.org:1337/announce").as_deref(), Some("tracker.opentrackr.org:1337"));
        assert_eq!(host_port("http://x:80/announce"), None);
    }

    /// A real BEP-15 exchange against an in-process tracker on loopback.
    #[tokio::test]
    async fn scrapes_a_local_tracker() {
        let server = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let port = server.local_addr().unwrap().port();
        let srv = tokio::spawn(async move {
            let mut buf = [0u8; 2048];
            let conn_id: u64 = 0x1122_3344_5566_7788;
            // connect
            let (n, peer) = server.recv_from(&mut buf).await.unwrap();
            assert_eq!(n, 16);
            assert_eq!(u64::from_be_bytes(buf[..8].try_into().unwrap()), PROTOCOL_ID);
            let tx = be32(&buf, 12);
            let mut r = Vec::new();
            r.extend_from_slice(&ACTION_CONNECT.to_be_bytes());
            r.extend_from_slice(&tx.to_be_bytes());
            r.extend_from_slice(&conn_id.to_be_bytes());
            server.send_to(&r, peer).await.unwrap();
            // scrape
            let (n, peer) = server.recv_from(&mut buf).await.unwrap();
            assert_eq!(u64::from_be_bytes(buf[..8].try_into().unwrap()), conn_id);
            assert_eq!(be32(&buf, 8), ACTION_SCRAPE);
            let tx = be32(&buf, 12);
            let hashes = (n - 16) / 20;
            let mut r = Vec::new();
            r.extend_from_slice(&ACTION_SCRAPE.to_be_bytes());
            r.extend_from_slice(&tx.to_be_bytes());
            for i in 0..hashes as u32 {
                for v in [100 + i, 50 + i, 10 + i] {
                    r.extend_from_slice(&v.to_be_bytes());
                }
            }
            server.send_to(&r, peer).await.unwrap();
        });
        let got = scrape(&format!("udp://127.0.0.1:{port}/announce"), &[H1.into(), H2.into()], Duration::from_secs(2))
            .await
            .unwrap();
        srv.await.unwrap();
        assert_eq!(got, vec![Swarm { seeders: 100, completed: 50, leechers: 10 }, Swarm { seeders: 101, completed: 51, leechers: 11 }]);
    }
}
