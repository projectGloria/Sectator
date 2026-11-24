//! qBittorrent Web API v2: cookie login, `torrents/add`, `app/version`.

use reqwest::header::{COOKIE, REFERER, SET_COOKIE};

use super::{base_url, non_empty, ClientConfig, ClientError, PushOutcome, Result};

/// Log in when credentials are set; returns the `SID=…` cookie to send back.
/// Without credentials we rely on qBittorrent's "bypass auth for localhost".
async fn login(http: &reqwest::Client, base: &str, cfg: &ClientConfig) -> Result<Option<String>> {
    let Some(user) = non_empty(&cfg.username) else { return Ok(None) };
    let form = [("username", user), ("password", cfg.password.as_deref().unwrap_or(""))];
    let resp = http.post(format!("{base}/api/v2/auth/login")).header(REFERER, base).form(&form).send().await?;
    if resp.status().as_u16() == 403 {
        return Err(ClientError::Auth); // IP banned after too many failures
    }
    if !resp.status().is_success() {
        return Err(ClientError::Status(resp.status().as_u16()));
    }
    let cookie = resp
        .headers()
        .get_all(SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .filter_map(|v| v.split(';').next())
        .map(str::trim)
        .filter(|kv| kv.contains('='))
        .collect::<Vec<_>>()
        .join("; ");
    let body = resp.text().await?;
    if body.trim() != "Ok." || cookie.is_empty() {
        return Err(ClientError::Auth);
    }
    Ok(Some(cookie))
}

fn authed(rb: reqwest::RequestBuilder, base: &str, cookie: &Option<String>) -> reqwest::RequestBuilder {
    let rb = rb.header(REFERER, base);
    match cookie {
        Some(c) => rb.header(COOKIE, c),
        None => rb,
    }
}

pub async fn add(http: &reqwest::Client, cfg: &ClientConfig, magnet: &str) -> Result<PushOutcome> {
    let base = base_url(cfg)?;
    let cookie = login(http, &base, cfg).await?;
    let mut form: Vec<(&str, &str)> = vec![("urls", magnet)];
    if let Some(p) = non_empty(&cfg.save_path) {
        form.push(("savepath", p));
    }
    if let Some(c) = non_empty(&cfg.category) {
        form.push(("category", c));
    }
    if cfg.paused {
        // `paused` before v5, `stopped` from v5 on; each version ignores the other.
        form.push(("paused", "true"));
        form.push(("stopped", "true"));
    }
    let resp = authed(http.post(format!("{base}/api/v2/torrents/add")), &base, &cookie).form(&form).send().await?;
    match resp.status().as_u16() {
        200 => {}
        403 => return Err(ClientError::Auth),
        409 => return Ok(PushOutcome::Duplicate),
        s => return Err(ClientError::Status(s)),
    }
    match resp.text().await?.trim() {
        "Ok." => Ok(PushOutcome::Added),
        // qBittorrent answers "Fails." when nothing was added; for a well-formed
        // magnet that means it is already in the list.
        "Fails." => Ok(PushOutcome::Duplicate),
        other => Err(ClientError::Protocol(other.chars().take(120).collect())),
    }
}

pub async fn version(http: &reqwest::Client, cfg: &ClientConfig) -> Result<String> {
    let base = base_url(cfg)?;
    let cookie = login(http, &base, cfg).await?;
    let resp = authed(http.get(format!("{base}/api/v2/app/version")), &base, &cookie).send().await?;
    match resp.status().as_u16() {
        200 => Ok(format!("qBittorrent {}", resp.text().await?.trim())),
        403 => Err(ClientError::Auth),
        s => Err(ClientError::Status(s)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clients::fake::{serve, Request};
    use crate::clients::{push, test, ClientKind};

    const MAGNET: &str = "magnet:?xt=urn:btih:3b245504cf5f11bbdbe1201cea6a6bf45aee1bc0&dn=ubuntu";

    fn qb(req: &Request) -> (u16, Vec<(String, String)>, String) {
        let authed = req.header("cookie") == Some("SID=abc123");
        match (req.method.as_str(), req.path.as_str()) {
            ("POST", "/api/v2/auth/login") if req.body == "username=admin&password=s%26cret" => {
                (200, vec![("Set-Cookie".into(), "SID=abc123; HttpOnly; SameSite=Strict; path=/".into())], "Ok.".into())
            }
            ("POST", "/api/v2/auth/login") => (200, vec![], "Fails.".into()),
            (_, _) if !authed => (403, vec![], "Forbidden".into()),
            ("GET", "/api/v2/app/version") => (200, vec![], "v5.1.2".into()),
            ("POST", "/api/v2/torrents/add") if req.body.contains("dn%3Dubuntu") && !req.body.contains("category") => (200, vec![], "Ok.".into()),
            ("POST", "/api/v2/torrents/add") => (200, vec![], "Fails.".into()),
            _ => (404, vec![], String::new()),
        }
    }

    fn cfg(url: &str, pass: &str) -> ClientConfig {
        ClientConfig { kind: ClientKind::Qbittorrent, url: format!("{url}/"), username: Some("admin".into()), password: Some(pass.into()), ..Default::default() }
    }

    #[tokio::test]
    async fn login_add_and_version() {
        let (url, log) = serve(qb).await;
        assert_eq!(test(&cfg(&url, "s&cret")).await.unwrap(), "qBittorrent v5.1.2");
        assert_eq!(push(&cfg(&url, "s&cret"), MAGNET).await.unwrap(), PushOutcome::Added);
        let mut dup = cfg(&url, "s&cret");
        dup.category = Some("linux".into());
        assert_eq!(push(&dup, MAGNET).await.unwrap(), PushOutcome::Duplicate);
        assert!(matches!(push(&cfg(&url, "wrong"), MAGNET).await, Err(ClientError::Auth)));

        let log = log.lock().unwrap();
        let add = log.iter().find(|r| r.path == "/api/v2/torrents/add").unwrap();
        assert_eq!(add.header("referer"), Some(url.as_str()));
        assert!(add.header("content-type").unwrap().starts_with("application/x-www-form-urlencoded"));
    }

    #[tokio::test]
    async fn no_credentials_skips_login() {
        let (url, log) = serve(|r: &Request| if r.path == "/api/v2/torrents/add" { (200, vec![], "Ok.".into()) } else { (500, vec![], String::new()) }).await;
        let c = ClientConfig { kind: ClientKind::Qbittorrent, url, paused: true, ..Default::default() };
        assert_eq!(push(&c, MAGNET).await.unwrap(), PushOutcome::Added);
        let log = log.lock().unwrap();
        assert_eq!(log.len(), 1);
        assert!(log[0].body.contains("paused=true") && log[0].body.contains("stopped=true"));
    }
}
