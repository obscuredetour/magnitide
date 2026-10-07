//! calibre's `MetadataReaderPlugin`s and `MetadataWriterPlugin`s, as
//! Magnitide's metadata readers and writers for the formats they handle.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use magnitide_core::hooks::{MetadataPlugins, Precedence};
use magnitide_core::metadata::{Author, BookMetadata};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{Installed, PluginKind, Plugins};

const TIMEOUT: Duration = Duration::from_secs(60);

/// A book's metadata as the host sends and takes it (`book_metadata()` in
/// magnitide_host/hooks.py).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginMetadata {
    pub title: Option<String>,
    pub title_sort: Option<String>,
    #[serde(default)]
    pub authors: Vec<PluginAuthor>,
    pub series: Option<String>,
    pub series_index: Option<f64>,
    pub publisher: Option<String>,
    /// ISO date.
    pub published: Option<String>,
    #[serde(default)]
    pub languages: Vec<String>,
    #[serde(default)]
    pub identifiers: BTreeMap<String, String>,
    pub description: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub rating: Option<i64>,
    pub uuid: Option<String>,
    /// Where the host wrote the cover (when reading).
    pub cover: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginAuthor {
    pub name: String,
    pub sort: Option<String>,
}

impl From<PluginMetadata> for BookMetadata {
    fn from(m: PluginMetadata) -> Self {
        let cover = m.cover.as_deref().and_then(|p| std::fs::read(p).ok());
        BookMetadata {
            title: m.title,
            authors: m.authors.into_iter().map(|a| Author { name: a.name, sort: a.sort }).collect(),
            series: m.series.map(|s| (s, m.series_index.unwrap_or(1.0))),
            publisher: m.publisher,
            published: m.published.as_deref().and_then(magnitide_core::metadata::parse_date),
            languages: m.languages,
            identifiers: m.identifiers,
            description: m.description,
            tags: m.tags,
            cover,
            uuids: m.uuid.into_iter().map(|u| u.to_lowercase()).collect(),
        }
    }
}

impl From<&BookMetadata> for PluginMetadata {
    fn from(m: &BookMetadata) -> Self {
        PluginMetadata {
            title: m.title.clone(),
            title_sort: None,
            authors: m.authors.iter().map(|a| PluginAuthor { name: a.name.clone(), sort: a.sort.clone() }).collect(),
            series: m.series.as_ref().map(|(s, _)| s.clone()),
            series_index: m.series.as_ref().map(|(_, i)| *i),
            publisher: m.publisher.clone(),
            published: m.published.map(|d| d.to_string()),
            languages: m.languages.clone(),
            identifiers: m.identifiers.clone(),
            description: m.description.clone(),
            tags: m.tags.clone(),
            rating: None,
            uuid: m.uuids.first().cloned(),
            cover: None,
        }
    }
}

impl Plugins {
    fn handlers(&self, kind: PluginKind, format: &str) -> Vec<Installed> {
        let ext = format.to_lowercase();
        let mut ans: Vec<Installed> = self
            .list()
            .into_iter()
            .filter(|p| p.enabled && p.kind == kind)
            .filter(|p| {
                p.hooks.as_ref().is_some_and(|h| h.file_types.iter().any(|f| f == "*" || f.eq_ignore_ascii_case(&ext)))
            })
            .collect();
        ans.sort_by_key(|p| std::cmp::Reverse(p.hooks.as_ref().map_or(1, |h| h.priority)));
        ans
    }

    /// Enabled reader plugins for `format` (upper case), highest priority first.
    pub fn metadata_readers(&self, format: &str) -> Vec<Installed> {
        self.handlers(PluginKind::MetadataReader, format)
    }

    pub fn metadata_writers(&self, format: &str) -> Vec<Installed> {
        self.handlers(PluginKind::MetadataWriter, format)
    }

    /// Whether any enabled plugin reads or writes a format (no host needed).
    pub fn has_metadata_plugins(&self) -> bool {
        self.list()
            .iter()
            .any(|p| p.enabled && matches!(p.kind, PluginKind::MetadataReader | PluginKind::MetadataWriter))
    }

    fn scratch_file(&self, suffix: &str) -> Result<PathBuf, String> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let dir = self.dir.join("tmp");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        Ok(dir.join(format!("{suffix}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst))))
    }
}

impl MetadataPlugins for Plugins {
    fn reads(&self, format: &str) -> Option<Precedence> {
        let readers = self.metadata_readers(format);
        let best = readers.iter().map(|p| p.hooks.as_ref().map_or(1, |h| h.priority)).max()?;
        // calibre's own readers have priority 1; a plugin above that overrides them.
        Some(if best > 1 { Precedence::Override } else { Precedence::Fallback })
    }

    fn read(&self, path: &Path, format: &str) -> Result<BookMetadata, String> {
        let readers = self.metadata_readers(format);
        if readers.is_empty() {
            return Err(format!("no plugin reads {format}"));
        }
        let host = self.host()?;
        let out = self.scratch_file("cover")?;
        let paths: Vec<PathBuf> = readers.iter().map(|p| self.path_of(p)).collect();
        let params = json!({ "path": path, "ext": format.to_lowercase(), "out": out, "plugins": paths });
        let answer = host.call("metadata.read", params, TIMEOUT);
        let result = answer.and_then(|v| {
            serde_json::from_value::<PluginMetadata>(v.get("metadata").cloned().unwrap_or(Value::Null))
                .map_err(|e| e.to_string())
        });
        let meta = result.map(BookMetadata::from);
        let _ = std::fs::remove_file(&out);
        if let Err(e) = &meta {
            for p in &readers {
                self.note_error(&p.name, e);
            }
        }
        meta
    }

    fn writes(&self, format: &str) -> bool {
        !self.metadata_writers(format).is_empty()
    }

    fn write(&self, path: &Path, format: &str, meta: &BookMetadata) -> Result<(), String> {
        let writers = self.metadata_writers(format);
        if writers.is_empty() {
            return Err(format!("no plugin writes {format}"));
        }
        let host = self.host()?;
        let cover = match &meta.cover {
            Some(bytes) => {
                let p = self.scratch_file("cover")?;
                std::fs::write(&p, bytes).map_err(|e| e.to_string())?;
                Some(p)
            }
            None => None,
        };
        let paths: Vec<PathBuf> = writers.iter().map(|p| self.path_of(p)).collect();
        let params = json!({
            "path": path, "ext": format.to_lowercase(), "metadata": PluginMetadata::from(meta), "cover": cover, "plugins": paths,
        });
        let answer = host.call("metadata.write", params, TIMEOUT).map(|_| ());
        if let Some(c) = cover {
            let _ = std::fs::remove_file(c);
        }
        if let Err(e) = &answer {
            for p in &writers {
                self.note_error(&p.name, e);
            }
        }
        answer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_metadata_round_trips() {
        let v = json!({
            "title": "T", "authors": [{"name": "A", "sort": "A, Sort"}], "series": "S", "seriesIndex": 2.5,
            "published": "2021-05-04", "languages": ["eng"], "identifiers": {"isbn": "9780593135204"},
            "description": "<p>d</p>", "tags": ["x"], "rating": 8, "uuid": "ABC"
        });
        let m: PluginMetadata = serde_json::from_value(v).unwrap();
        let b = BookMetadata::from(m);
        assert_eq!(b.title.as_deref(), Some("T"));
        assert_eq!(b.authors[0].sort.as_deref(), Some("A, Sort"));
        assert_eq!(b.series.as_ref().map(|(s, i)| (s.as_str(), *i)), Some(("S", 2.5)));
        assert_eq!(b.published.map(|d| d.to_string()).as_deref(), Some("2021-05-04"));
        assert_eq!(b.uuids, ["abc"]);
        let back = PluginMetadata::from(&b);
        assert_eq!(back.series_index, Some(2.5));
        assert_eq!(back.published.as_deref(), Some("2021-05-04"));
    }
}
