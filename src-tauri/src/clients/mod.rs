//! Torrent-client integrations: push a magnet straight into qBittorrent or
//! Transmission over their local Web APIs instead of the OS magnet handler.

pub mod download;
pub mod qbittorrent;
pub mod transmission;

use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClientKind {
    #[default]
    Qbittorrent,
    Transmission,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ClientConfig {
    pub id: String,
    pub kind: ClientKind,
    pub name: String,
    /// Web UI base, e.g. `http://127.0.0.1:8080` (qBittorrent) or `http://127.0.0.1:9091` (Transmission).
    pub url: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub save_path: Option<String>,
    /// qBittorrent category / Transmission label.
    pub category: Option<String>,
    pub paused: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PushOutcome {
    Added,
    /// The client already has this torrent (or refused it as a duplicate).
    Duplicate,
}

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("not a magnet link")]
    NotMagnet,
    #[error("invalid client URL: {0}")]
    BadUrl(String),
    #[error("login failed: check the username and password")]
    Auth,
    #[error("client answered HTTP {0}")]
    Status(u16),
    #[error("unexpected reply: {0}")]
    Protocol(String),
    #[error("could not reach the client: {0}")]
    Http(#[from] reqwest::Error),
}

pub type Result<T> = std::result::Result<T, ClientError>;

/// Local clients are always reached directly: never through the indexer proxy or Tor.
fn http_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder().no_proxy().connect_timeout(Duration::from_secs(4)).timeout(Duration::from_secs(10)).build()?)
}

fn base_url(cfg: &ClientConfig) -> Result<String> {
    let u = cfg.url.trim().trim_end_matches('/');
    match url::Url::parse(u) {
        Ok(p) if matches!(p.scheme(), "http" | "https") && p.host().is_some() => Ok(u.to_string()),
        _ => Err(ClientError::BadUrl(cfg.url.clone())),
    }
}

fn non_empty(s: &Option<String>) -> Option<&str> {
    s.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

/// Send a magnet to the configured client.
pub async fn push(cfg: &ClientConfig, magnet: &str) -> Result<PushOutcome> {
    if !magnet.starts_with("magnet:?") || magnet.chars().any(char::is_control) {
        return Err(ClientError::NotMagnet);
    }
    let http = http_client()?;
    match cfg.kind {
        ClientKind::Qbittorrent => qbittorrent::add(&http, cfg, magnet).await,
        ClientKind::Transmission => transmission::add(&http, cfg, magnet).await,
    }
}

/// Check connectivity and credentials; returns the client's version string.
pub async fn test(cfg: &ClientConfig) -> Result<String> {
    let http = http_client()?;
    match cfg.kind {
        ClientKind::Qbittorrent => qbittorrent::version(&http, cfg).await,
        ClientKind::Transmission => transmission::version(&http, cfg).await,
    }
}

/// A minimal raw-TCP HTTP/1.1 server for exercising the real client code paths.
#[cfg(test)]
pub(crate) mod fake {
    use std::sync::{Arc, Mutex};

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[derive(Debug, Clone)]
    pub struct Request {
        pub method: String,
        pub path: String,
        pub headers: Vec<(String, String)>,
        pub body: String,
    }

    impl Request {
        pub fn header(&self, name: &str) -> Option<&str> {
            self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
        }
    }

    /// (status, extra headers, body)
    pub type Reply = (u16, Vec<(String, String)>, String);

    /// Serve until the test ends; returns the base URL and the request log.
    pub async fn serve<F>(handler: F) -> (String, Arc<Mutex<Vec<Request>>>)
    where
        F: Fn(&Request) -> Reply + Send + Sync + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let log = Arc::new(Mutex::new(Vec::new()));
        let (log2, handler) = (log.clone(), Arc::new(handler));
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else { return };
                let (log, handler) = (log2.clone(), handler.clone());
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 4096];
                    let req = loop {
                        let Ok(n) = sock.read(&mut chunk).await else { return };
                        if n == 0 {
                            return;
                        }
                        buf.extend_from_slice(&chunk[..n]);
                        let text = String::from_utf8_lossy(&buf).to_string();
                        let Some(end) = text.find("\r\n\r\n") else { continue };
                        let mut lines = text[..end].split("\r\n");
                        let mut first = lines.next().unwrap_or("").split(' ');
                        let (method, path) = (first.next().unwrap_or("").to_string(), first.next().unwrap_or("").to_string());
                        let headers: Vec<(String, String)> =
                            lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_string(), v.trim().to_string())).collect();
                        let len = headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("content-length")).and_then(|(_, v)| v.parse::<usize>().ok()).unwrap_or(0);
                        if buf.len() < end + 4 + len {
                            continue;
                        }
                        let body = String::from_utf8_lossy(&buf[end + 4..end + 4 + len]).to_string();
                        break Request { method, path, headers, body };
                    };
                    let (status, extra, body) = handler(&req);
                    log.lock().unwrap().push(req);
                    let mut out = format!("HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n", body.len());
                    for (k, v) in extra {
                        out.push_str(&format!("{k}: {v}\r\n"));
                    }
                    out.push_str("\r\n");
                    out.push_str(&body);
                    let _ = sock.write_all(out.as_bytes()).await;
                    let _ = sock.shutdown().await;
                });
            }
        });
        (format!("http://{addr}"), log)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn rejects_non_magnets_and_bad_urls() {
        let cfg = ClientConfig { url: "http://127.0.0.1:1".into(), ..Default::default() };
        assert!(matches!(push(&cfg, "https://evil.example/x.exe").await, Err(ClientError::NotMagnet)));
        let bad = ClientConfig { url: "file:///c:/x".into(), ..Default::default() };
        assert!(matches!(test(&bad).await, Err(ClientError::BadUrl(_))));
    }
}
