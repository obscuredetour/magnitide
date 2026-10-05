use std::cmp::Ordering;
use std::str::FromStr;

use serde::Deserialize;

use crate::model::Book;

/// Book orderings shared by the CLI and the app.
///
/// Text keys compare case-insensitively. calibre uses ICU collation, so
/// accented and non-Latin titles may order slightly differently.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "lowercase")]
pub enum SortKey {
    #[default]
    Title,
    Author,
    /// Series name, then series index.
    Series,
    Rating,
    /// Date added to the library.
    Added,
    Published,
    Publisher,
    /// Size of the largest format.
    Size,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "lowercase")]
pub enum SortDir {
    Asc,
    Desc,
}

impl SortKey {
    /// The direction people usually want first: A→Z for text, newest /
    /// highest / largest first for dates, ratings and sizes.
    pub fn natural_dir(self) -> SortDir {
        match self {
            Self::Title | Self::Author | Self::Series | Self::Publisher => SortDir::Asc,
            Self::Rating | Self::Added | Self::Published | Self::Size => SortDir::Desc,
        }
    }
}

impl FromStr for SortKey {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        serde_json::from_value(serde_json::Value::String(s.to_lowercase())).map_err(|_| {
            format!("unknown sort {s:?}; expected title, author, series, rating, added, published, publisher or size")
        })
    }
}

/// A book's value for one sort key. `None` (no rating, no series, …) always
/// sorts last, whichever the direction.
#[derive(PartialEq, PartialOrd)]
enum Value {
    Text(String),
    /// Name, number, then omnibuses (0) before single books (1) at the same number.
    Series(String, f64, u8),
    Number(f64),
}

fn value(b: &Book, key: SortKey) -> Option<Value> {
    let text = |s: &str| (!s.is_empty()).then(|| Value::Text(s.to_lowercase()));
    match key {
        SortKey::Title => text(b.sort.as_deref().unwrap_or(&b.title)),
        SortKey::Author => b.author_sort.as_deref().and_then(text),
        SortKey::Series => {
            b.series.as_ref().map(|s| Value::Series(s.to_lowercase(), b.series_index, u8::from(b.series_end.is_none())))
        }
        SortKey::Rating => b.rating.map(|r| Value::Number(r.into())),
        SortKey::Added => b.timestamp.map(|t| Value::Number(t.timestamp_micros() as f64)),
        SortKey::Published => b.pubdate.map(|t| Value::Number(t.timestamp_micros() as f64)),
        SortKey::Publisher => b.publisher.as_deref().and_then(text),
        SortKey::Size => b.formats.iter().map(|f| f.size).max().map(|s| Value::Number(s as f64)),
    }
}

pub fn sort_books(books: &mut [&Book], key: SortKey, dir: SortDir) {
    let mut keyed: Vec<_> =
        books.iter().map(|b| (value(b, key), b.sort.as_deref().unwrap_or(&b.title).to_lowercase(), *b)).collect();
    keyed.sort_by(|(va, ta, _), (vb, tb, _)| {
        let primary = match (va, vb) {
            (Some(a), Some(b)) => {
                let o = a.partial_cmp(b).unwrap_or(Ordering::Equal);
                if dir == SortDir::Desc { o.reverse() } else { o }
            }
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        };
        // Ties (same author, same rating…) fall back to title order.
        primary.then_with(|| ta.cmp(tb))
    });
    for (slot, (_, _, b)) in books.iter_mut().zip(keyed) {
        *slot = b;
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn book(id: i64, title: &str, rating: Option<u8>, series: Option<(&str, f64)>) -> Book {
        Book {
            id,
            title: title.into(),
            sort: None,
            authors: vec![],
            author_sort: None,
            series: series.map(|(s, _)| s.into()),
            series_index: series.map_or(1.0, |(_, i)| i),
            series_end: None,
            more_series: vec![],
            collects: vec![],
            collected_in: vec![],
            tags: vec![],
            shelves: Vec::new(),
            publisher: None,
            rating,
            languages: vec![],
            identifiers: BTreeMap::new(),
            comments: None,
            timestamp: None,
            pubdate: None,
            last_modified: None,
            path: String::new(),
            uuid: None,
            has_cover: false,
            formats: vec![],
            custom: BTreeMap::new(),
        }
    }

    fn ids(books: &[&Book]) -> Vec<i64> {
        books.iter().map(|b| b.id).collect()
    }

    #[test]
    fn parses_sort_keys() {
        assert_eq!("Series".parse::<SortKey>(), Ok(SortKey::Series));
        assert!("bogus".parse::<SortKey>().is_err());
    }

    #[test]
    fn missing_values_sort_last_in_both_directions() {
        let (a, b, c) = (book(1, "A", Some(4), None), book(2, "B", None, None), book(3, "C", Some(8), None));
        let mut v = vec![&a, &b, &c];
        sort_books(&mut v, SortKey::Rating, SortDir::Desc);
        assert_eq!(ids(&v), [3, 1, 2]);
        sort_books(&mut v, SortKey::Rating, SortDir::Asc);
        assert_eq!(ids(&v), [1, 3, 2]);
    }

    #[test]
    fn series_orders_by_name_then_index_with_title_tiebreak() {
        let b1 = book(1, "Two", None, Some(("Saga", 2.0)));
        let b2 = book(2, "One", None, Some(("Saga", 1.0)));
        let b3 = book(3, "Alone", None, None);
        let b4 = book(4, "Half", None, Some(("Saga", 1.5)));
        let mut v = vec![&b1, &b2, &b3, &b4];
        sort_books(&mut v, SortKey::Series, SortDir::Asc);
        assert_eq!(ids(&v), [2, 4, 1, 3]);
        sort_books(&mut v, SortKey::Series, SortDir::Desc);
        assert_eq!(ids(&v), [1, 4, 2, 3]);
    }
}
