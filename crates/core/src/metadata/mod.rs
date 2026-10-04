//! Reading metadata from book files being added to a library.
//!
//! EPUB (and KEPUB, which is EPUB) is read fully, cover included. Other
//! formats fall back to the file name for now; MOBI/AZW3 and PDF readers
//! are on the roadmap.

use std::collections::BTreeMap;
use std::path::Path;

use chrono::NaiveDate;

pub mod epub;
pub mod epub_write;
pub mod filename;

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
}

#[derive(Debug, Clone, PartialEq)]
pub struct Author {
    pub name: String,
    pub sort: Option<String>,
}

/// Best-effort metadata for a book file: read from inside the file where
/// supported, with the file name filling any gaps (title, authors).
pub fn read(path: &Path, format: &str) -> BookMetadata {
    let from_name = filename::parse(path);
    let mut meta = match format {
        "EPUB" | "KEPUB" => epub::read(path).unwrap_or_default(),
        _ => BookMetadata::default(),
    };
    if meta.title.is_none() {
        meta.title = from_name.title;
    }
    if meta.authors.is_empty() {
        meta.authors = from_name.authors;
    }
    meta
}

/// ISO 639-1 codes and the ISO 639-2 codes calibre (and Magnitide) store.
const LANGUAGES: &[(&str, &str)] = &[
    ("en", "eng"),
    ("de", "deu"),
    ("fr", "fra"),
    ("es", "spa"),
    ("it", "ita"),
    ("nl", "nld"),
    ("pt", "por"),
    ("ru", "rus"),
    ("ja", "jpn"),
    ("zh", "zho"),
    ("ko", "kor"),
    ("sv", "swe"),
    ("no", "nor"),
    ("nb", "nor"),
    ("da", "dan"),
    ("fi", "fin"),
    ("pl", "pol"),
    ("cs", "ces"),
    ("tr", "tur"),
    ("el", "ell"),
    ("he", "heb"),
    ("ar", "ara"),
    ("hi", "hin"),
    ("uk", "ukr"),
    ("hu", "hun"),
    ("ro", "ron"),
    ("ca", "cat"),
    ("la", "lat"),
];

/// ISO 639-1 ("en", "en-GB") to the ISO 639-2 codes calibre uses ("eng").
/// Already three-letter codes pass through; unknown ones are kept as given.
pub fn language_code(code: &str) -> String {
    let base = code.trim().split(['-', '_']).next().unwrap_or("").to_ascii_lowercase();
    LANGUAGES.iter().find(|(two, _)| *two == base).map_or(base, |(_, three)| (*three).to_owned())
}

/// The reverse, for writing into files: EPUB wants BCP 47 tags, which use the
/// two-letter code where one exists ("eng" → "en").
pub fn language_tag(code: &str) -> String {
    let code = code.trim().to_ascii_lowercase();
    LANGUAGES.iter().find(|(_, three)| *three == code).map_or(code, |(two, _)| (*two).to_owned())
}

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
    fn maps_language_codes() {
        assert_eq!(language_code("en-GB"), "eng");
        assert_eq!(language_code("de"), "deu");
        assert_eq!(language_code("eng"), "eng");
        assert_eq!(language_code("tlh"), "tlh");
        assert_eq!(language_tag("eng"), "en");
        assert_eq!(language_tag("nor"), "no");
        assert_eq!(language_tag("tlh"), "tlh");
    }

    #[test]
    fn parses_partial_dates() {
        assert_eq!(parse_date("2021-11-23T07:00:00+00:00"), NaiveDate::from_ymd_opt(2021, 11, 23));
        assert_eq!(parse_date("2021-11"), NaiveDate::from_ymd_opt(2021, 11, 1));
        assert_eq!(parse_date("1974"), NaiveDate::from_ymd_opt(1974, 1, 1));
        assert_eq!(parse_date("0101-01-01T00:00:00+00:00"), None);
        assert_eq!(parse_date("someday"), None);
    }
}
