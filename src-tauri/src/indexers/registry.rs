//! Builds indexer instances from user config and ships the default catalogue.

use std::sync::Arc;

use super::{apibay, bt4g, btdig, knaben, nyaa, torznab, Indexer, IndexerConfig, IndexerError, ProxyOverride};

pub const KINDS: [&str; 6] = ["apibay", "nyaa", "knaben", "bt4g", "btdig", "torznab"];

pub fn build(cfg: &IndexerConfig) -> Result<Arc<dyn Indexer>, IndexerError> {
    if cfg.base_urls.iter().all(|u| u.trim().is_empty()) {
        return Err(IndexerError::Config(format!("{}: no base URL", cfg.id)));
    }
    Ok(match cfg.kind.as_str() {
        "apibay" => Arc::new(apibay::ApiBay::new(cfg)),
        "nyaa" => Arc::new(nyaa::Nyaa::new(cfg)),
        "knaben" => Arc::new(knaben::Knaben::new(cfg)),
        "bt4g" => Arc::new(bt4g::Bt4g::new(cfg)),
        "btdig" => Arc::new(btdig::BtDig::new(cfg)),
        "torznab" => Arc::new(torznab::Torznab::new(cfg)),
        other => return Err(IndexerError::Config(format!("unknown indexer kind `{other}`"))),
    })
}

fn cfg(id: &str, name: &str, kind: &str, enabled: bool, urls: &[&str]) -> IndexerConfig {
    IndexerConfig {
        id: id.into(),
        name: name.into(),
        kind: kind.into(),
        enabled,
        base_urls: urls.iter().map(|s| s.to_string()).collect(),
        proxy: ProxyOverride::Inherit,
        timeout_ms: None,
        api_key: None,
    }
}

/// Out-of-the-box indexers. Tor and Torznab entries ship disabled.
pub fn default_configs() -> Vec<IndexerConfig> {
    let mut tor = cfg("btdig-tor", "BTDig (Tor)", "btdig", false, &[btdig::ONION]);
    tor.proxy = ProxyOverride::Custom("socks5h://127.0.0.1:9050".into());
    let mut prowlarr = cfg("prowlarr", "Prowlarr (Torznab)", "torznab", false, &["http://127.0.0.1:9696/1"]);
    prowlarr.proxy = ProxyOverride::Direct;
    vec![
        cfg("apibay", "The Pirate Bay", "apibay", true, &["https://apibay.org"]),
        cfg("nyaa", "Nyaa", "nyaa", true, &["https://nyaa.si", "https://nyaa.land"]),
        cfg("knaben", "Knaben", "knaben", true, &["https://api.knaben.org/v1"]),
        cfg("bt4g", "BT4G", "bt4g", true, &["https://bt4gprx.com"]),
        cfg("btdig", "BTDig", "btdig", true, &["https://btdig.com"]),
        tor,
        prowlarr,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::SourceType;

    #[test]
    fn every_default_builds() {
        let defaults = default_configs();
        let mut ids: Vec<&str> = defaults.iter().map(|c| c.id.as_str()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), defaults.len(), "ids are unique");
        for c in &defaults {
            let ix = build(c).unwrap();
            assert_eq!(ix.meta().id, c.id);
            assert!(KINDS.contains(&c.kind.as_str()));
        }
        let tor = defaults.iter().find(|c| c.id == "btdig-tor").unwrap();
        assert!(!tor.enabled && matches!(&tor.proxy, ProxyOverride::Custom(p) if p.starts_with("socks5h://")));
        assert_eq!(build(&defaults[4]).unwrap().meta().source_type, SourceType::Dht);
    }

    #[test]
    fn rejects_bad_config() {
        let mut c = default_configs().remove(0);
        c.kind = "nope".into();
        assert!(build(&c).is_err());
        c.kind = "apibay".into();
        c.base_urls.clear();
        assert!(build(&c).is_err());
    }
}
