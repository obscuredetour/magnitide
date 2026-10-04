use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::Serialize;

pub type BookId = i64;

#[derive(Debug, Clone, Serialize)]
pub struct Book {
    pub id: BookId,
    pub title: String,
    /// Title used for sorting ("Hobbit, The").
    pub sort: Option<String>,
    /// In display order (calibre orders by link-row id).
    pub authors: Vec<String>,
    pub author_sort: Option<String>,
    pub series: Option<String>,
    pub series_index: f64,
    pub tags: Vec<String>,
    pub publisher: Option<String>,
    /// calibre stores ratings as 0..=10 (half-stars). Divide by 2 for stars.
    pub rating: Option<u8>,
    /// ISO 639-2 codes, e.g. "eng", in calibre's order.
    pub languages: Vec<String>,
    pub identifiers: BTreeMap<String, String>,
    /// HTML.
    pub comments: Option<String>,
    pub timestamp: Option<DateTime<Utc>>,
    pub pubdate: Option<DateTime<Utc>>,
    pub last_modified: Option<DateTime<Utc>>,
    /// Folder relative to the library root, always '/'-separated.
    pub path: String,
    pub uuid: Option<String>,
    pub has_cover: bool,
    pub formats: Vec<Format>,
    /// Keyed by lookup name including the '#', e.g. "#genre".
    pub custom: BTreeMap<String, CustomValue>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Format {
    /// Upper case, e.g. "EPUB".
    pub format: String,
    pub size: u64,
    /// File stem; the file on disk is `<name>.<format lowercased>`.
    pub name: String,
}

impl Book {
    pub fn dir(&self, library_root: &Path) -> PathBuf {
        // calibre always uses '/' in `books.path`, regardless of OS.
        self.path.split('/').fold(library_root.to_path_buf(), |p, c| p.join(c))
    }

    pub fn format_path(&self, library_root: &Path, f: &Format) -> PathBuf {
        self.dir(library_root).join(format!("{}.{}", f.name, f.format.to_lowercase()))
    }

    pub fn cover_path(&self, library_root: &Path) -> Option<PathBuf> {
        self.has_cover.then(|| self.dir(library_root).join("cover.jpg"))
    }

    pub fn stars(&self) -> Option<f32> {
        self.rating.map(|r| r as f32 / 2.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Datatype {
    Text,
    Comments,
    Series,
    Enumeration,
    Rating,
    Int,
    Float,
    Bool,
    Datetime,
    Composite,
    Unknown(String),
}

impl Datatype {
    pub fn parse(s: &str) -> Self {
        match s {
            "text" => Self::Text,
            "comments" => Self::Comments,
            "series" => Self::Series,
            "enumeration" => Self::Enumeration,
            "rating" => Self::Rating,
            "int" => Self::Int,
            "float" => Self::Float,
            "bool" => Self::Bool,
            "datetime" => Self::Datetime,
            "composite" => Self::Composite,
            other => Self::Unknown(other.to_owned()),
        }
    }
}

/// One row of the `custom_columns` table.
#[derive(Debug, Clone, Serialize)]
pub struct CustomColumn {
    pub id: i64,
    /// Lookup name without '#'.
    pub label: String,
    /// Display name.
    pub name: String,
    pub datatype: Datatype,
    pub is_multiple: bool,
    pub normalized: bool,
    pub editable: bool,
    /// Free-form JSON; holds e.g. `composite_template`, `enum_values`, `is_names`.
    pub display: serde_json::Value,
}

impl CustomColumn {
    pub fn lookup_name(&self) -> String {
        format!("#{}", self.label)
    }

    /// `custom_column_N` holds values (normalized) or (book, value) rows.
    pub fn value_table(&self) -> String {
        format!("custom_column_{}", self.id)
    }

    /// `books_custom_column_N_link` maps books to values (normalized only).
    pub fn link_table(&self) -> String {
        format!("books_custom_column_{}_link", self.id)
    }

    pub fn composite_template(&self) -> Option<&str> {
        self.display.get("composite_template")?.as_str()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum CustomValue {
    Text(String),
    TextList(Vec<String>),
    Series {
        name: String,
        index: f64,
    },
    Int(i64),
    Float(f64),
    Bool(bool),
    /// 0..=10, like the built-in rating.
    Rating(u8),
    Datetime(DateTime<Utc>),
}

impl std::fmt::Display for CustomValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Text(s) => f.write_str(s),
            Self::TextList(v) => f.write_str(&v.join(", ")),
            Self::Series { name, index } => write!(f, "{name} [{index}]"),
            Self::Int(i) => write!(f, "{i}"),
            Self::Float(x) => write!(f, "{x}"),
            Self::Bool(b) => f.write_str(if *b { "Yes" } else { "No" }),
            Self::Rating(r) => write!(f, "{}★", *r as f32 / 2.0),
            Self::Datetime(d) => write!(f, "{}", d.format("%Y-%m-%d")),
        }
    }
}
