//! What changed in a book's metadata between two versions of it, e.g. a
//! book before and after a calibre plugin edited it. Only fields both kinds
//! of library have are compared (not shelves, omnibus links or extra series).

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use chrono::NaiveDate;

use crate::model::{Book, BookId, CustomColumn, CustomValue};

/// Each `Some` field is a new value; `None` means unchanged. Fields that can
/// be cleared hold an inner `Option` (`Some(None)`: cleared).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BookChanges {
    pub title: Option<String>,
    /// Only when the sort changed on its own; a new title brings its own sort.
    pub sort: Option<String>,
    pub authors: Option<Vec<String>>,
    /// Name and number.
    pub series: Option<Option<(String, f64)>>,
    pub tags: Option<Vec<String>>,
    pub publisher: Option<Option<String>>,
    /// 1–10 (half stars).
    pub rating: Option<Option<u8>>,
    pub languages: Option<Vec<String>>,
    pub identifiers: Option<BTreeMap<String, String>>,
    /// HTML.
    pub comments: Option<Option<String>>,
    pub published: Option<Option<NaiveDate>>,
    /// By lookup name ("#genre"); `None` clears the field.
    pub custom: BTreeMap<String, Option<CustomValue>>,
}

impl BookChanges {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// The names of the fields that changed, for reports ("title", "#genre").
    pub fn fields(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut note = |changed: bool, name: &str| {
            if changed {
                out.push(name.to_owned());
            }
        };
        note(self.title.is_some(), "title");
        note(self.sort.is_some(), "title sort");
        note(self.authors.is_some(), "authors");
        note(self.series.is_some(), "series");
        note(self.tags.is_some(), "tags");
        note(self.publisher.is_some(), "publisher");
        note(self.rating.is_some(), "rating");
        note(self.languages.is_some(), "languages");
        note(self.identifiers.is_some(), "identifiers");
        note(self.comments.is_some(), "description");
        note(self.published.is_some(), "published");
        out.extend(self.custom.keys().cloned());
        out
    }
}

/// What changed in a library outside Magnitide (a calibre plugin working on
/// the shadow library), ready to apply.
#[derive(Debug, Default)]
pub struct Changes {
    pub books: Vec<ChangedBook>,
    /// New books (their ids are the other library's, not this one's).
    pub added: Vec<AddedBook>,
    pub removed: Vec<BookId>,
    /// Custom columns created there.
    pub new_columns: Vec<CustomColumn>,
}

impl Changes {
    pub fn is_empty(&self) -> bool {
        self.books.is_empty() && self.added.is_empty() && self.removed.is_empty() && self.new_columns.is_empty()
    }
}

#[derive(Debug)]
pub struct ChangedBook {
    pub id: BookId,
    pub fields: BookChanges,
    pub files: Vec<FileChange>,
    pub cover: Option<CoverChange>,
}

#[derive(Debug, PartialEq)]
pub enum FileChange {
    /// A new format, and the file holding it.
    Added {
        format: String,
        path: PathBuf,
    },
    /// New contents for a format the book has.
    Replaced {
        format: String,
        path: PathBuf,
    },
    Removed {
        format: String,
    },
}

#[derive(Debug, PartialEq)]
pub enum CoverChange {
    Set(PathBuf),
    Removed,
}

#[derive(Debug)]
pub struct AddedBook {
    pub book: Book,
    /// Upper-case format and file.
    pub files: Vec<(String, PathBuf)>,
    pub cover: Option<PathBuf>,
}

/// How `after` differs from `before` (the same book).
pub fn book_changes(before: &Book, after: &Book) -> BookChanges {
    fn changed<T: PartialEq + Clone>(a: &T, b: &T) -> Option<T> {
        (a != b).then(|| b.clone())
    }
    let series = |b: &Book| b.series.clone().map(|s| (s, b.series_index));
    // Tags have no order: calibre keeps them in the order they were added,
    // Magnitide alphabetically.
    let tags = |b: &Book| b.tags.iter().cloned().collect::<BTreeSet<_>>();
    let rating = |b: &Book| b.rating.filter(|&r| r > 0);
    let empty_is_none = |s: &Option<String>| s.clone().filter(|s| !s.trim().is_empty());

    let title = changed(&before.title, &after.title);
    let sort =
        if title.is_none() && after.sort.is_some() { changed(&before.sort, &after.sort).flatten() } else { None };
    let mut custom = BTreeMap::new();
    for key in before.custom.keys().chain(after.custom.keys()) {
        let (a, b) = (before.custom.get(key), after.custom.get(key));
        if a != b {
            custom.insert(key.clone(), b.cloned());
        }
    }
    BookChanges {
        title,
        sort,
        authors: changed(&before.authors, &after.authors),
        series: changed(&series(before), &series(after)),
        tags: (tags(before) != tags(after)).then(|| tags(after).into_iter().collect()),
        publisher: changed(&before.publisher, &after.publisher),
        rating: changed(&rating(before), &rating(after)),
        languages: changed(&before.languages, &after.languages),
        identifiers: changed(&before.identifiers, &after.identifiers),
        comments: changed(&empty_is_none(&before.comments), &empty_is_none(&after.comments)),
        // Magnitide keeps the date only; calibre a timestamp.
        published: changed(&before.pubdate.map(|d| d.date_naive()), &after.pubdate.map(|d| d.date_naive())),
        custom,
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::*;

    fn book() -> Book {
        Book {
            id: 1,
            title: "The Dispossessed".into(),
            sort: Some("Dispossessed, The".into()),
            authors: vec!["Ursula K. Le Guin".into()],
            author_sort: None,
            series: Some("Hainish Cycle".into()),
            series_index: 5.0,
            series_end: None,
            more_series: vec![],
            collects: vec![],
            collected_in: vec![],
            tags: vec!["Classics".into(), "Science Fiction".into()],
            shelves: vec![],
            publisher: None,
            rating: Some(8),
            languages: vec!["eng".into()],
            identifiers: BTreeMap::new(),
            comments: None,
            timestamp: None,
            pubdate: Some(Utc.with_ymd_and_hms(1974, 5, 1, 0, 0, 0).unwrap()),
            last_modified: None,
            path: String::new(),
            uuid: None,
            has_cover: false,
            formats: vec![],
            custom: BTreeMap::from([("#read".into(), CustomValue::Bool(false))]),
        }
    }

    #[test]
    fn unchanged_is_empty() {
        let a = book();
        let mut b = a.clone();
        // Not changes: tag order, a pubdate's time of day, an empty description.
        b.tags.reverse();
        b.pubdate = Some(Utc.with_ymd_and_hms(1974, 5, 1, 12, 30, 0).unwrap());
        b.comments = Some("  ".into());
        b.last_modified = Some(Utc::now());
        assert!(book_changes(&a, &b).is_empty(), "{:?}", book_changes(&a, &b));
    }

    #[test]
    fn reports_each_field() {
        let a = book();
        let mut b = a.clone();
        b.title = "A Different Title".into();
        b.sort = Some("Different Title, A".into());
        b.series_index = 6.0;
        b.tags.push("Utopia".into());
        b.publisher = Some("Harper & Row".into());
        b.rating = Some(0);
        b.custom.insert("#read".into(), CustomValue::Bool(true));
        b.custom.insert("#pages".into(), CustomValue::Int(387));
        let c = book_changes(&a, &b);
        assert_eq!(c.title.as_deref(), Some("A Different Title"));
        assert_eq!(c.sort, None, "a new title brings its own sort");
        assert_eq!(c.series, Some(Some(("Hainish Cycle".into(), 6.0))));
        assert_eq!(c.tags.unwrap(), ["Classics", "Science Fiction", "Utopia"]);
        assert_eq!(c.publisher, Some(Some("Harper & Row".into())));
        assert_eq!(c.rating, Some(None), "calibre's 0 is no rating");
        assert_eq!(c.custom.len(), 2);
        assert_eq!(c.authors, None);

        let mut d = a.clone();
        d.custom.clear();
        d.sort = Some("Dispossessed".into());
        let c = book_changes(&a, &d);
        assert_eq!(c.custom, BTreeMap::from([("#read".into(), None)]));
        assert_eq!(c.sort.as_deref(), Some("Dispossessed"), "a sort changed by itself is kept");
        assert_eq!(c.fields(), ["title sort", "#read"]);
    }
}
