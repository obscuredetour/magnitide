use std::cmp::Reverse;
use std::str::FromStr;

use serde::Deserialize;

use crate::model::Book;

/// Book orderings shared by the CLI and the app.
///
/// Text keys compare case-insensitively. calibre uses ICU collation, so
/// accented and non-Latin titles may order slightly differently.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SortKey {
    #[default]
    Title,
    Author,
    /// Most recently added first.
    Added,
    /// Most recently published first.
    Published,
    /// Highest rated first.
    Rating,
    /// By series name, then series index; books without a series last.
    Series,
}

impl FromStr for SortKey {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        serde_json::from_value(serde_json::Value::String(s.to_lowercase()))
            .map_err(|_| format!("unknown sort {s:?}; expected title, author, added, published, rating or series"))
    }
}

pub fn sort_books(books: &mut [&Book], key: SortKey) {
    let text = |s: &Option<String>, fallback: &str| s.as_deref().unwrap_or(fallback).to_lowercase();
    match key {
        SortKey::Title => books.sort_by_cached_key(|b| text(&b.sort, &b.title)),
        SortKey::Author => books.sort_by_cached_key(|b| (text(&b.author_sort, ""), text(&b.sort, &b.title))),
        SortKey::Added => books.sort_by_key(|b| Reverse(b.timestamp)),
        SortKey::Published => books.sort_by_key(|b| Reverse(b.pubdate)),
        SortKey::Rating => books.sort_by_key(|b| Reverse(b.rating)),
        SortKey::Series => books.sort_by_cached_key(|b| {
            (b.series.is_none(), text(&b.series, ""), SeriesIndex(b.series_index), text(&b.sort, &b.title))
        }),
    }
}

/// Total order over series indexes (which are f64 in calibre).
#[derive(PartialEq)]
struct SeriesIndex(f64);

impl Eq for SeriesIndex {}

impl PartialOrd for SeriesIndex {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SeriesIndex {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.total_cmp(&other.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sort_keys() {
        assert_eq!("Series".parse::<SortKey>(), Ok(SortKey::Series));
        assert!("bogus".parse::<SortKey>().is_err());
    }
}
