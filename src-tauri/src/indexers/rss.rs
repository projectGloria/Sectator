//! Minimal streaming RSS reader (quick-xml) for Nyaa, BT4G and Torznab feeds.
//! Collects each `<item>`'s direct child text by qualified name, plus
//! `torznab:attr name=… value=…` pairs and the `<enclosure>`.

use std::collections::HashMap;

use quick_xml::events::Event;
use quick_xml::Reader;

use super::IndexerError;

#[derive(Debug, Default, Clone)]
pub struct RssItem {
    /// Child element text keyed by qualified name (`title`, `nyaa:seeders`, …).
    pub fields: HashMap<String, String>,
    /// `<torznab:attr>` / `<newznab:attr>` pairs in document order (names repeat, e.g. `category`).
    pub attrs: Vec<(String, String)>,
    pub enclosure_url: Option<String>,
    pub enclosure_len: Option<u64>,
}

impl RssItem {
    pub fn field(&self, name: &str) -> Option<&str> {
        self.fields.get(name).map(String::as_str).filter(|s| !s.is_empty())
    }

    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    pub fn attrs_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        self.attrs.iter().filter(move |(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

fn local(qname: &str) -> &str {
    qname.rsplit(':').next().unwrap_or(qname)
}

fn resolve_ref(name: &str) -> Option<String> {
    if let Some(num) = name.strip_prefix('#') {
        let code = match num.strip_prefix(['x', 'X']) {
            Some(h) => u32::from_str_radix(h, 16).ok()?,
            None => num.parse().ok()?,
        };
        return char::from_u32(code).map(String::from);
    }
    Some(
        match name {
            "amp" => "&",
            "lt" => "<",
            "gt" => ">",
            "quot" => "\"",
            "apos" => "'",
            _ => return None,
        }
        .to_string(),
    )
}

#[allow(deprecated)] // unescape_value: entity-decoded, which is what we want
fn attrs_of(e: &quick_xml::events::BytesStart<'_>) -> Vec<(String, String)> {
    e.attributes()
        .flatten()
        .map(|a| {
            let k = a.key.local_name().as_ref().to_string();
            let v = a.unescape_value().map(|v| v.into_owned()).unwrap_or_default();
            (k, v)
        })
        .collect()
}

/// Parse every `<item>` of an RSS document. A Torznab `<error code=… description=…/>`
/// root becomes an error.
pub fn parse_items(xml: &str) -> Result<Vec<RssItem>, IndexerError> {
    let mut reader = Reader::from_str(xml);
    let mut items = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    let mut item: Option<(usize, RssItem)> = None;
    let mut text = String::new();
    let mut saw_root = false;

    let handle_leaf = |name: &str, e: &quick_xml::events::BytesStart<'_>, it: &mut RssItem| match local(name) {
        "attr" => {
            let a = attrs_of(e);
            let get = |k: &str| a.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
            if let (Some(n), Some(v)) = (get("name"), get("value")) {
                it.attrs.push((n, v));
            }
        }
        "enclosure" => {
            let a = attrs_of(e);
            for (k, v) in a {
                match k.as_str() {
                    "url" => it.enclosure_url = Some(v),
                    "length" => it.enclosure_len = v.parse().ok(),
                    _ => {}
                }
            }
        }
        _ => {}
    };

    loop {
        let ev = reader.read_event().map_err(|e| IndexerError::Parse(format!("xml: {e}")))?;
        match ev {
            Event::Start(e) => {
                let name = e.name().as_ref().to_string();
                if !saw_root {
                    saw_root = true;
                    check_error_root(&name, &e)?;
                }
                if name == "item" && item.is_none() {
                    item = Some((stack.len() + 1, RssItem::default()));
                } else if let Some((_, it)) = item.as_mut() {
                    handle_leaf(&name, &e, it);
                }
                stack.push(name);
                text.clear();
            }
            Event::Empty(e) => {
                let name = e.name().as_ref().to_string();
                if !saw_root {
                    saw_root = true;
                    check_error_root(&name, &e)?;
                }
                if let Some((_, it)) = item.as_mut() {
                    handle_leaf(&name, &e, it);
                }
            }
            Event::Text(t) => text.push_str(&t.xml10_content()),
            Event::CData(c) => text.push_str(&c.into_inner()),
            Event::GeneralRef(r) => {
                let name = r.into_inner();
                match resolve_ref(&name) {
                    Some(s) => text.push_str(&s),
                    None => {
                        text.push('&');
                        text.push_str(&name);
                        text.push(';');
                    }
                }
            }
            Event::End(_) => {
                let depth = stack.len();
                let name = stack.pop().unwrap_or_default();
                if let Some((item_depth, it)) = item.as_mut() {
                    if depth == *item_depth {
                        if let Some((_, done)) = item.take() {
                            items.push(done);
                        }
                    } else if depth == *item_depth + 1 {
                        it.fields.insert(name, text.trim().to_string());
                    }
                }
                text.clear();
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !saw_root {
        return Err(IndexerError::Parse("empty document".into()));
    }
    Ok(items)
}

fn check_error_root(name: &str, e: &quick_xml::events::BytesStart<'_>) -> Result<(), IndexerError> {
    match local(name) {
        "rss" | "feed" => Ok(()),
        "error" => {
            let a = attrs_of(e);
            let get = |k: &str| a.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone()).unwrap_or_default();
            Err(IndexerError::Config(format!("torznab error {}: {}", get("code"), get("description"))))
        }
        // An HTML block/captcha page instead of a feed.
        other => Err(IndexerError::Parse(format!("expected rss, got <{other}>"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_items_with_namespaces_cdata_and_entities() {
        let xml = r#"<?xml version="1.0"?><rss version="2.0" xmlns:torznab="http://torznab.com/schemas/2015/feed"><channel>
            <title>feed &amp; co</title>
            <item><title>A &amp; B &#34;x&#34;</title><description><![CDATA[<b>raw</b>]]></description>
              <enclosure url="http://h/dl?id=1&amp;k=2" length="123" type="application/x-bittorrent"/>
              <torznab:attr name="seeders" value="5"/><torznab:attr name="category" value="2000"/><torznab:attr name="category" value="2040"/>
            </item>
            <item><title>second</title></item>
        </channel></rss>"#;
        let items = parse_items(xml).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].field("title"), Some("A & B \"x\""));
        assert_eq!(items[0].field("description"), Some("<b>raw</b>"));
        assert_eq!(items[0].enclosure_url.as_deref(), Some("http://h/dl?id=1&k=2"));
        assert_eq!(items[0].enclosure_len, Some(123));
        assert_eq!(items[0].attr("seeders"), Some("5"));
        assert_eq!(items[0].attrs_named("category").collect::<Vec<_>>(), vec!["2000", "2040"]);
        assert_eq!(items[1].field("title"), Some("second"));
    }

    #[test]
    fn torznab_error_and_html_pages_are_errors() {
        let err = parse_items(r#"<?xml version="1.0"?><error code="100" description="Invalid API Key"/>"#);
        assert!(matches!(err, Err(IndexerError::Config(m)) if m.contains("Invalid API Key")));
        assert!(matches!(parse_items("<html><body>captcha</body></html>"), Err(IndexerError::Parse(_))));
        assert!(parse_items("").is_err());
    }
}
