//! calibre metadata-source plugins in Find Metadata.
//!
//! Each enabled source is asked at once (like the built-in ones); its
//! results become [`SourceRecord`]s that join the same ranking. A plugin
//! gives covers as image data rather than URLs, so a record's cover URL is a
//! `calibre-plugin:` address naming the plugin and the book, which
//! [`Plugins::cover`] turns back into a request to that plugin.

use std::collections::BTreeMap;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use magnitide_sources::{Lookup, SourceRecord};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{Installed, PluginKind, Plugins};

/// How long a plugin may take (it's told this; the host waits a little longer).
const TIMEOUT_SECS: u64 = 30;

pub const COVER_SCHEME: &str = "calibre-plugin:";

/// Which plugin to ask for a cover, and about which book.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct CoverRequest {
    plugin: String,
    title: String,
    authors: Vec<String>,
    identifiers: BTreeMap<String, String>,
}

fn cover_url(req: &CoverRequest) -> String {
    format!("{COVER_SCHEME}{}", URL_SAFE_NO_PAD.encode(serde_json::to_vec(req).unwrap_or_default()))
}

fn parse_cover_url(url: &str) -> Option<CoverRequest> {
    let data = URL_SAFE_NO_PAD.decode(url.strip_prefix(COVER_SCHEME)?).ok()?;
    serde_json::from_slice(&data).ok()
}

/// Whether `url` is a plugin cover address (see [`Plugins::cover`]).
pub fn is_plugin_cover(url: &str) -> bool {
    url.starts_with(COVER_SCHEME)
}

/// The host's record (see `record()` in host.py) as a [`SourceRecord`].
fn to_record(plugin: &str, v: &Value) -> SourceRecord {
    let text = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_owned).filter(|s| !s.trim().is_empty());
    let list = |k: &str| -> Vec<String> {
        v.get(k)
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect())
            .unwrap_or_default()
    };
    let identifiers: BTreeMap<String, String> = v
        .get("identifiers")
        .and_then(Value::as_object)
        .map(|m| m.iter().filter_map(|(k, x)| Some((k.to_lowercase(), x.as_str()?.to_owned()))).collect())
        .unwrap_or_default();
    let title = text("title").unwrap_or_default();
    let authors = list("authors");
    let cover_url = v.get("hasCover").and_then(Value::as_bool).unwrap_or(false).then(|| {
        cover_url(&CoverRequest {
            plugin: plugin.to_owned(),
            title: title.clone(),
            authors: authors.clone(),
            identifiers: identifiers.clone(),
        })
    });
    SourceRecord {
        source: None,
        source_name: plugin.to_owned(),
        source_key: text("sourceKey").unwrap_or_else(|| format!("{plugin}:{title}")),
        link: text("link").filter(|l| l.starts_with("https://") || l.starts_with("http://")),
        title,
        subtitle: None,
        authors,
        publisher: text("publisher"),
        published: text("published"),
        languages: list("languages"),
        identifiers,
        tags: list("tags"),
        description_html: text("descriptionHtml"),
        cover_url,
        pages: None,
        series: text("series"),
        series_index: v.get("seriesIndex").and_then(Value::as_f64),
        has_details: false,
    }
}

fn lookup_params(lookup: &Lookup) -> Value {
    let mut identifiers = serde_json::Map::new();
    if let Some(isbn) = lookup.isbn.as_deref().filter(|i| !i.trim().is_empty()) {
        identifiers.insert("isbn".into(), isbn.trim().into());
    }
    json!({
        "title": lookup.title.trim(),
        "authors": lookup.authors.iter().map(|a| a.trim()).filter(|a| !a.is_empty()).collect::<Vec<_>>(),
        "identifiers": identifiers,
        "timeout": TIMEOUT_SECS,
    })
}

impl Plugins {
    /// Installed, enabled metadata sources.
    pub fn metadata_sources(&self) -> Vec<Installed> {
        self.list()
            .into_iter()
            .filter(|p| {
                p.enabled && p.kind == PluginKind::MetadataSource && p.capabilities.iter().any(|c| c == "identify")
            })
            .collect()
    }

    /// Ask every enabled metadata-source plugin at once: each one's name and
    /// its records, or why it failed. Empty when there are none (or no calibre).
    pub fn identify(&self, lookup: &Lookup) -> Vec<(String, Result<Vec<SourceRecord>, String>)> {
        let sources = self.metadata_sources();
        if sources.is_empty() {
            return Vec::new();
        }
        let host = match self.host() {
            Ok(h) => h,
            Err(e) => return sources.into_iter().map(|p| (p.name, Err(e.clone()))).collect(),
        };
        let params = lookup_params(lookup);
        std::thread::scope(|scope| {
            let handles: Vec<_> = sources
                .iter()
                .map(|p| {
                    let (host, mut params) = (host.clone(), params.clone());
                    params["path"] = json!(self.path_of(p));
                    let name = p.name.clone();
                    scope.spawn(move || {
                        let answer =
                            host.call("identify", params, Duration::from_secs(TIMEOUT_SECS + 15)).and_then(|v| {
                                let records: Vec<SourceRecord> = v
                                    .get("records")
                                    .and_then(Value::as_array)
                                    .map(|a| a.iter().map(|r| to_record(&name, r)).collect())
                                    .unwrap_or_default();
                                match v.get("error").and_then(Value::as_str) {
                                    // A source that failed outright; one that found something keeps it.
                                    Some(e) if records.is_empty() => Err(e.to_owned()),
                                    _ => Ok(records),
                                }
                            });
                        (name, answer)
                    })
                })
                .collect();
            handles
                .into_iter()
                .zip(&sources)
                .map(|(h, p)| h.join().unwrap_or_else(|_| (p.name.clone(), Err("crashed".into()))))
                .collect()
        })
    }

    /// The cover a `calibre-plugin:` address names, from its plugin.
    pub fn cover(&self, url: &str) -> Result<Vec<u8>, String> {
        let req = parse_cover_url(url).ok_or("not a plugin cover address")?;
        let plugin = self
            .list()
            .into_iter()
            .find(|p| p.name == req.plugin && p.enabled)
            .ok_or_else(|| format!("the {} plugin isn't installed and on", req.plugin))?;
        let host = self.host()?;
        let out = tempfile_path(&self.dir)?;
        let params = json!({
            "path": self.path_of(&plugin),
            "title": req.title,
            "authors": req.authors,
            "identifiers": req.identifiers,
            "timeout": TIMEOUT_SECS,
            "out": out,
        });
        let answer = host.call("cover", params, Duration::from_secs(TIMEOUT_SECS + 15));
        let bytes = std::fs::read(&out).ok();
        let _ = std::fs::remove_file(&out);
        answer?;
        bytes.filter(|b| !b.is_empty()).ok_or_else(|| format!("{} found no cover", req.plugin))
    }
}

/// A fresh path for the host to write a cover to.
fn tempfile_path(dir: &std::path::Path) -> Result<std::path::PathBuf, String> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let dir = dir.join("tmp");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join(format!("cover-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cover_addresses() {
        let req = CoverRequest {
            plugin: "Goodreads".into(),
            title: "Project Hail Mary".into(),
            authors: vec!["Andy Weir".into()],
            identifiers: [("goodreads".to_owned(), "54493401".to_owned())].into(),
        };
        let url = cover_url(&req);
        assert!(is_plugin_cover(&url) && !url.contains(['/', '+', '=', ' ']), "{url}");
        assert_eq!(parse_cover_url(&url), Some(req));
        assert_eq!(parse_cover_url("calibre-plugin:not base64!"), None);
        assert_eq!(parse_cover_url("https://covers.openlibrary.org/b/id/1-L.jpg"), None);
    }

    #[test]
    fn records_from_the_host() {
        let v = json!({
            "title": "Project Hail Mary", "authors": ["Andy Weir"], "publisher": "Ballantine Books",
            "published": "2021-05-04", "languages": ["eng"], "identifiers": {"ISBN": "9780593135204", "test": "42"},
            "tags": ["Science Fiction"], "descriptionHtml": "<p>Hi</p>", "link": "javascript:alert(1)",
            "sourceKey": "test:42", "hasCover": true, "relevance": 0, "series": "S", "seriesIndex": 2.0
        });
        let r = to_record("Test Source", &v);
        assert_eq!((r.source, r.source_name.as_str(), r.source_key.as_str()), (None, "Test Source", "test:42"));
        assert_eq!(
            r.identifiers.get("isbn").map(String::as_str),
            Some("9780593135204"),
            "identifier types lower-cased"
        );
        assert_eq!(r.link, None, "only web links");
        assert_eq!((r.series.as_deref(), r.series_index), (Some("S"), Some(2.0)));
        let cover = parse_cover_url(r.cover_url.as_deref().unwrap()).unwrap();
        assert_eq!(
            (cover.plugin.as_str(), cover.identifiers.get("test").map(String::as_str)),
            ("Test Source", Some("42"))
        );
        let bare = to_record("X", &json!({"title": "T"}));
        assert_eq!((bare.cover_url, bare.source_key.as_str(), bare.publisher), (None, "X:T", None));
    }

    #[test]
    fn lookup_parameters() {
        let p = lookup_params(&Lookup {
            title: " Dune ".into(),
            authors: vec!["Frank Herbert".into(), " ".into()],
            isbn: Some("978".into()),
        });
        assert_eq!(p["title"], "Dune");
        assert_eq!(p["authors"], json!(["Frank Herbert"]));
        assert_eq!(p["identifiers"]["isbn"], "978");
    }
}
