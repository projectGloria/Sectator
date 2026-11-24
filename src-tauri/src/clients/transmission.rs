//! Transmission RPC: CSRF session-id handshake, `torrent-add`, `session-get`.

use serde_json::{json, Value};

use super::{base_url, non_empty, ClientConfig, ClientError, PushOutcome, Result};

const SESSION_HEADER: &str = "X-Transmission-Session-Id";

/// Accept either the Web UI root or the full RPC endpoint.
fn rpc_url(cfg: &ClientConfig) -> Result<String> {
    let base = base_url(cfg)?;
    Ok(if base.ends_with("/rpc") { base } else { format!("{base}/transmission/rpc") })
}

/// POST one RPC call, repeating once with the session id a 409 hands back.
async fn call(http: &reqwest::Client, cfg: &ClientConfig, body: &Value) -> Result<Value> {
    let url = rpc_url(cfg)?;
    let mut session: Option<String> = None;
    for _ in 0..2 {
        let mut rb = http.post(&url).json(body);
        if let Some(user) = non_empty(&cfg.username) {
            rb = rb.basic_auth(user, cfg.password.as_deref());
        }
        if let Some(s) = &session {
            rb = rb.header(SESSION_HEADER, s);
        }
        let resp = rb.send().await?;
        match resp.status().as_u16() {
            409 => {
                session = resp.headers().get(SESSION_HEADER).and_then(|v| v.to_str().ok()).map(String::from);
                if session.is_none() {
                    return Err(ClientError::Protocol("409 without a session id".into()));
                }
            }
            401 | 403 => return Err(ClientError::Auth),
            200 => {
                let v: Value = resp.json().await?;
                return match v.get("result").and_then(Value::as_str) {
                    Some("success") => Ok(v.get("arguments").cloned().unwrap_or(Value::Null)),
                    Some(other) => Err(ClientError::Protocol(other.chars().take(120).collect())),
                    None => Err(ClientError::Protocol("missing result".into())),
                };
            }
            s => return Err(ClientError::Status(s)),
        }
    }
    Err(ClientError::Protocol("session id was not accepted".into()))
}

pub async fn add(http: &reqwest::Client, cfg: &ClientConfig, magnet: &str) -> Result<PushOutcome> {
    let mut args = json!({ "filename": magnet, "paused": cfg.paused });
    if let Some(p) = non_empty(&cfg.save_path) {
        args["download-dir"] = json!(p);
    }
    if let Some(c) = non_empty(&cfg.category) {
        args["labels"] = json!([c]);
    }
    let out = call(http, cfg, &json!({ "method": "torrent-add", "arguments": args })).await?;
    if out.get("torrent-duplicate").is_some() {
        Ok(PushOutcome::Duplicate)
    } else if out.get("torrent-added").is_some() {
        Ok(PushOutcome::Added)
    } else {
        Err(ClientError::Protocol("torrent-add returned neither added nor duplicate".into()))
    }
}

pub async fn version(http: &reqwest::Client, cfg: &ClientConfig) -> Result<String> {
    let out = call(http, cfg, &json!({ "method": "session-get", "arguments": { "fields": ["version"] } })).await?;
    let v = out.get("version").and_then(Value::as_str).ok_or_else(|| ClientError::Protocol("no version".into()))?;
    Ok(format!("Transmission {v}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clients::fake::{serve, Request};
    use crate::clients::{push, test, ClientKind};

    const MAGNET: &str = "magnet:?xt=urn:btih:3b245504cf5f11bbdbe1201cea6a6bf45aee1bc0&dn=ubuntu";

    fn tr(req: &Request) -> (u16, Vec<(String, String)>, String) {
        if req.path != "/transmission/rpc" {
            return (404, vec![], String::new());
        }
        // "user:pw" in base64
        if req.header("authorization") != Some("Basic dXNlcjpwdw==") {
            return (401, vec![], String::new());
        }
        if req.header(SESSION_HEADER) != Some("tok42") {
            return (409, vec![(SESSION_HEADER.into(), "tok42".into())], String::new());
        }
        let body: Value = serde_json::from_str(&req.body).unwrap();
        let reply = match body["method"].as_str().unwrap() {
            "session-get" => json!({ "result": "success", "arguments": { "version": "4.0.6 (38c164933e)" } }),
            "torrent-add" if body["arguments"]["labels"][0] == "linux" => {
                json!({ "result": "success", "arguments": { "torrent-duplicate": { "id": 1 } } })
            }
            "torrent-add" => json!({ "result": "success", "arguments": { "torrent-added": { "id": 2, "name": "ubuntu" } } }),
            _ => json!({ "result": "method name not recognized" }),
        };
        (200, vec![], reply.to_string())
    }

    fn cfg(url: &str) -> ClientConfig {
        ClientConfig { kind: ClientKind::Transmission, url: url.into(), username: Some("user".into()), password: Some("pw".into()), ..Default::default() }
    }

    #[tokio::test]
    async fn session_handshake_add_and_version() {
        let (url, log) = serve(tr).await;
        assert_eq!(test(&cfg(&url)).await.unwrap(), "Transmission 4.0.6 (38c164933e)");
        let mut c = cfg(&format!("{url}/transmission/rpc"));
        c.save_path = Some("D:\\Torrents".into());
        assert_eq!(push(&c, MAGNET).await.unwrap(), PushOutcome::Added);
        c.category = Some("linux".into());
        assert_eq!(push(&c, MAGNET).await.unwrap(), PushOutcome::Duplicate);

        let mut bad = cfg(&url);
        bad.password = Some("nope".into());
        assert!(matches!(push(&bad, MAGNET).await, Err(ClientError::Auth)));

        let log = log.lock().unwrap();
        let add: Value = serde_json::from_str(&log.iter().find(|r| r.body.contains("D:")).unwrap().body).unwrap();
        assert_eq!(add["arguments"]["download-dir"], "D:\\Torrents");
        assert_eq!(add["arguments"]["filename"], MAGNET);
    }
}
