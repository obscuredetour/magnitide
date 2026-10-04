//! Editing several books at once: only the fields asked for change, and each
//! book is saved like a single edit (so folder and file renames, and their
//! all-or-nothing rules, are the same).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::library::{BookId, Library};

/// A field across the selected books: the value they share, or `mixed`.
/// Neither (value null, not mixed) means none of them has one.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct Shared<T> {
    pub value: Option<T>,
    pub mixed: bool,
}

impl<T: PartialEq + Clone> Shared<T> {
    fn of(values: impl IntoIterator<Item = Option<T>>) -> Self {
        let mut iter = values.into_iter();
        let Some(first) = iter.next() else { return Self { value: None, mixed: false } };
        if iter.all(|v| v == first) { Self { value: first, mixed: false } } else { Self { value: None, mixed: true } }
    }
}

/// A tag on some of the selected books.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct TagCount {
    pub name: String,
    /// How many of the selected books have it.
    pub books: usize,
}

/// What the selected books have in common, for the bulk edit form.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct BulkValues {
    pub books: usize,
    pub authors: Shared<Vec<String>>,
    pub publisher: Shared<String>,
    pub published: Shared<String>,
    pub rating: Shared<u8>,
    pub languages: Shared<Vec<String>>,
    pub series: Shared<String>,
    /// Every tag any of them has, most common first.
    pub tags: Vec<TagCount>,
}

/// Changes for every selected book. Null (None) leaves a field as each
/// book has it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct BulkEdit {
    pub authors: Option<Vec<String>>,
    /// "" clears it.
    pub publisher: Option<String>,
    /// ISO date; "" clears it.
    pub published: Option<String>,
    /// 1–10; 0 clears it.
    pub rating: Option<u8>,
    pub languages: Option<Vec<String>>,
    /// "" takes the books out of their series.
    pub series: Option<String>,
    /// Number the books from this, in the order given (1, 2, 3…). Null keeps
    /// each book's number.
    pub series_start: Option<f64>,
    pub add_tags: Vec<String>,
    /// Matched without regard to case.
    pub remove_tags: Vec<String>,
}

#[derive(Debug, Default, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct BulkReport {
    pub edited: usize,
    /// Books whose folder moved (author changed).
    pub moved: usize,
    /// Books that couldn't be saved, with the reason; left as they were.
    pub failed: Vec<String>,
}

impl Library {
    /// What `ids` have in common.
    pub fn bulk_values(&self, ids: &[BookId]) -> Result<BulkValues> {
        let books = ids.iter().map(|id| self.book_edit(*id)).collect::<Result<Vec<_>>>()?;
        let nonempty = |s: &Option<String>| s.clone().filter(|s| !s.trim().is_empty());
        let mut tags: BTreeMap<String, (String, usize)> = BTreeMap::new();
        for b in &books {
            for t in &b.tags {
                tags.entry(t.to_lowercase()).or_insert_with(|| (t.clone(), 0)).1 += 1;
            }
        }
        let mut tags: Vec<TagCount> = tags.into_values().map(|(name, books)| TagCount { name, books }).collect();
        tags.sort_by(|a, b| b.books.cmp(&a.books).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
        Ok(BulkValues {
            books: books.len(),
            authors: Shared::of(books.iter().map(|b| Some(b.authors.clone()))),
            publisher: Shared::of(books.iter().map(|b| nonempty(&b.publisher))),
            published: Shared::of(books.iter().map(|b| nonempty(&b.published))),
            rating: Shared::of(books.iter().map(|b| b.rating)),
            languages: Shared::of(books.iter().map(|b| Some(b.languages.clone()).filter(|l| !l.is_empty()))),
            series: Shared::of(books.iter().map(|b| nonempty(&b.series))),
            tags,
        })
    }

    /// Apply `change` to each of `ids`, in order. A book that fails is
    /// reported and left as it was; the others are still saved.
    pub fn bulk_edit(
        &mut self,
        ids: &[BookId],
        change: &BulkEdit,
        mut progress: impl FnMut(usize, usize),
    ) -> Result<BulkReport> {
        let mut report = BulkReport::default();
        for (i, id) in ids.iter().enumerate() {
            progress(i, ids.len());
            let mut e = match self.book_edit(*id) {
                Ok(e) => e,
                Err(err) => {
                    report.failed.push(format!("Book {id}: {err}"));
                    continue;
                }
            };
            let title = e.title.clone();
            if let Some(a) = &change.authors {
                e.authors = a.clone();
            }
            if let Some(p) = &change.publisher {
                e.publisher = Some(p.clone()).filter(|p| !p.trim().is_empty());
            }
            if let Some(d) = &change.published {
                e.published = Some(d.clone()).filter(|d| !d.trim().is_empty());
            }
            if let Some(r) = change.rating {
                e.rating = (r > 0).then_some(r);
            }
            if let Some(l) = &change.languages {
                e.languages = l.clone();
            }
            if let Some(s) = &change.series {
                e.series = Some(s.clone()).filter(|s| !s.trim().is_empty());
            }
            if let Some(start) = change.series_start {
                e.series_index = start + i as f64;
            }
            e.tags.retain(|t| !change.remove_tags.iter().any(|r| r.trim().eq_ignore_ascii_case(t.trim())));
            for t in &change.add_tags {
                if !t.trim().is_empty() && !e.tags.iter().any(|x| x.trim().eq_ignore_ascii_case(t.trim())) {
                    e.tags.push(t.trim().to_owned());
                }
            }
            match self.update_book(*id, &e) {
                Ok(outcome) => {
                    report.edited += 1;
                    report.moved += usize::from(outcome.moved);
                }
                Err(err) => report.failed.push(format!("“{title}”: {err}")),
            }
        }
        progress(ids.len(), ids.len());
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::import::add_files;

    fn library() -> (tempfile::TempDir, Library, Vec<BookId>) {
        let tmp = tempfile::tempdir().unwrap();
        let mut lib = Library::create(tmp.path().join("lib"), "Test").unwrap();
        let mut ids = Vec::new();
        for name in ["The Hunger Games", "Catching Fire", "Mockingjay"] {
            let src = tmp.path().join(format!("{name} - Suzanne Collins.epub"));
            fs::write(&src, name).unwrap();
            ids.push(add_files(&mut lib, &[src], |_| {}).unwrap().added[0]);
        }
        (tmp, lib, ids)
    }

    #[test]
    fn shows_what_books_share() {
        let (_tmp, mut lib, ids) = library();
        let mut e = lib.book_edit(ids[0]).unwrap();
        e.tags = vec!["Dystopia".into(), "YA".into()];
        e.publisher = Some("Scholastic".into());
        lib.update_book(ids[0], &e).unwrap();
        let mut e = lib.book_edit(ids[1]).unwrap();
        e.tags = vec!["dystopia".into()];
        lib.update_book(ids[1], &e).unwrap();

        let v = lib.bulk_values(&ids).unwrap();
        assert_eq!(v.books, 3);
        assert_eq!(v.authors, Shared { value: Some(vec!["Suzanne Collins".into()]), mixed: false });
        assert_eq!(v.publisher, Shared { value: None, mixed: true });
        assert_eq!(v.series, Shared { value: None, mixed: false }, "none of them has one");
        assert_eq!(v.tags, [TagCount { name: "Dystopia".into(), books: 2 }, TagCount { name: "YA".into(), books: 1 }]);
    }

    #[test]
    fn changes_only_what_is_asked() {
        let (_tmp, mut lib, ids) = library();
        let mut e = lib.book_edit(ids[1]).unwrap();
        e.tags = vec!["Keep".into(), "Old".into()];
        e.publisher = Some("Scholastic".into());
        lib.update_book(ids[1], &e).unwrap();

        let change = BulkEdit {
            series: Some("The Hunger Games".into()),
            series_start: Some(1.0),
            add_tags: vec!["Dystopia".into(), "keep".into()],
            remove_tags: vec!["OLD".into()],
            rating: Some(8),
            ..Default::default()
        };
        let report = lib.bulk_edit(&ids, &change, |_, _| {}).unwrap();
        assert_eq!((report.edited, report.moved), (3, 0), "{report:?}");

        let cat = lib.load_catalog().unwrap();
        for (n, id) in ids.iter().enumerate() {
            let b = cat.get(*id).unwrap();
            assert_eq!(b.series.as_deref(), Some("The Hunger Games"));
            assert_eq!(b.series_index, n as f64 + 1.0);
            assert_eq!(b.rating, Some(8));
        }
        let fire = cat.get(ids[1]).unwrap();
        assert_eq!(fire.tags, ["Dystopia", "Keep"]);
        assert_eq!(fire.publisher.as_deref(), Some("Scholastic"), "untouched");

        // Clearing, and a new author moves the folders.
        let change = BulkEdit {
            authors: Some(vec!["S. Collins".into()]),
            series: Some(String::new()),
            rating: Some(0),
            ..Default::default()
        };
        let report = lib.bulk_edit(&ids, &change, |_, _| {}).unwrap();
        assert_eq!((report.edited, report.moved), (3, 3));
        let cat = lib.load_catalog().unwrap();
        let b = cat.get(ids[0]).unwrap();
        assert_eq!((b.series.as_deref(), b.rating, b.authors[0].as_str()), (None, None, "S. Collins"));
        assert!(b.path.starts_with("S. Collins/"), "{}", b.path);
    }
}
