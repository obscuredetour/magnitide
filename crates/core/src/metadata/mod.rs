//! Reading metadata from book files being added to a library.
//!
//! EPUB (and KEPUB, which is EPUB) is read fully, cover included; so are
//! MOBI, AZW and AZW3. Other formats fall back to the file name; a PDF
//! reader is on the roadmap.

use std::collections::BTreeMap;
use std::path::Path;

use chrono::NaiveDate;

pub mod epub;
pub mod epub_write;
pub mod filename;
pub mod mobi;

/// What a book file says about itself. Everything is optional: files in
/// the wild are often incomplete.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BookMetadata {
    pub title: Option<String>,
    /// Display names in order, with a sort form when the file gives one.
    pub authors: Vec<Author>,
    pub series: Option<(String, f64)>,
    pub publisher: Option<String>,
    pub published: Option<NaiveDate>,
    /// ISO 639-2 codes ("eng").
    pub languages: Vec<String>,
    pub identifiers: BTreeMap<String, String>,
    /// HTML or plain text, not yet sanitized.
    pub description: Option<String>,
    pub tags: Vec<String>,
    /// Image bytes in whatever format the file stores.
    pub cover: Option<Vec<u8>>,
    /// Book uuids the file carries (calibre writes its book uuid), lower
    /// case. Read only; used to recognize a book's copies.
    pub uuids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Author {
    pub name: String,
    pub sort: Option<String>,
}

/// Best-effort metadata for a book file: read from inside the file where
/// supported, with the file name filling any gaps (title, authors).
pub fn read(path: &Path, format: &str) -> BookMetadata {
    read_with(path, format, &crate::hooks::NoHooks)
}

/// Whether Magnitide reads `format` itself.
pub fn builtin_reads(format: &str) -> bool {
    matches!(format, "EPUB" | "KEPUB" | "MOBI" | "AZW" | "AZW3" | "AZW4" | "PRC" | "PDB")
}

/// [`read`], with plugin readers: one that overrides is asked first (ours
/// on failure); one that's a fallback only for formats we can't read.
pub fn read_with(path: &Path, format: &str, plugins: &dyn crate::hooks::MetadataPlugins) -> BookMetadata {
    use crate::hooks::Precedence;

    let from_name = filename::parse(path);
    let builtin = || match format {
        "EPUB" | "KEPUB" => epub::read(path).unwrap_or_default(),
        "MOBI" | "AZW" | "AZW3" | "AZW4" | "PRC" | "PDB" => mobi::read(path).unwrap_or_default(),
        _ => BookMetadata::default(),
    };
    let mut meta = match plugins.reads(format) {
        Some(Precedence::Override) => plugins.read(path, format).unwrap_or_else(|_| builtin()),
        Some(Precedence::Fallback) if !builtin_reads(format) => plugins.read(path, format).unwrap_or_default(),
        _ => builtin(),
    };
    if meta.title.is_none() {
        meta.title = from_name.title;
    }
    if meta.authors.is_empty() {
        meta.authors = from_name.authors;
    }
    meta
}

pub use magnitide_catalog::languages::{language_code, language_tag};

/// "2021", "2021-11", "2021-11-23", "2021-11-23T07:00:00Z" → a date.
pub fn parse_date(s: &str) -> Option<NaiveDate> {
    let s = s.trim();
    let date_part = s.get(..10).filter(|d| d.len() == 10).unwrap_or(s);
    NaiveDate::parse_from_str(date_part, "%Y-%m-%d")
        .ok()
        .or_else(|| NaiveDate::parse_from_str(&format!("{}-01", s.get(..7)?), "%Y-%m-%d").ok())
        .or_else(|| NaiveDate::from_ymd_opt(s.get(..4)?.parse().ok()?, 1, 1))
        // 0101-01-01 and similar placeholders mean "unknown".
        .filter(|d| chrono::Datelike::year(d) > 101)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_partial_dates() {
        assert_eq!(parse_date("2021-11-23T07:00:00+00:00"), NaiveDate::from_ymd_opt(2021, 11, 23));
        assert_eq!(parse_date("2021-11"), NaiveDate::from_ymd_opt(2021, 11, 1));
        assert_eq!(parse_date("1974"), NaiveDate::from_ymd_opt(1974, 1, 1));
        assert_eq!(parse_date("0101-01-01T00:00:00+00:00"), None);
        assert_eq!(parse_date("someday"), None);
    }
}
