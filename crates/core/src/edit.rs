//! Editing a book's metadata and cover.
//!
//! Titles and first authors name the book's folder and files on disk
//! (`Author/Title (id)/Title - Author.epub`), so an edit that changes them
//! renames those too. The database update and the renames succeed or fail
//! together: renames run first, and if any step fails the completed ones are
//! reversed and the transaction rolls back, leaving everything as it was.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use rusqlite::{Connection, Transaction, params};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use magnitide_catalog::SeriesEntry;

use crate::custom::{FieldValue, write_custom};
use crate::library::{BookId, LOCAL_USER, Library, author_id, now, publisher_id, series_id, tag_id};
use crate::{html, images, names, paths};

/// Every editable field of a book, as the edit form shows and returns it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct BookEdit {
    pub title: String,
    /// Empty means "derive from the title" ("The Hobbit" → "Hobbit, The").
    pub title_sort: String,
    /// In display order; empty means "Unknown".
    pub authors: Vec<String>,
    /// Sort names to set, by author name ("Le Guin, Ursula K."); an empty
    /// value goes back to the automatic one. An author's sort name is shared
    /// by all their books. Authors not listed keep theirs.
    pub author_sorts: BTreeMap<String, String>,
    pub series: Option<String>,
    pub series_index: f64,
    /// Set for an omnibus: the range is `series_index`–`series_end`.
    pub series_end: Option<f64>,
    /// Further series, after the main one.
    pub more_series: Vec<SeriesEntry>,
    /// Books an omnibus contains, in order.
    pub collects: Vec<BookId>,
    pub publisher: Option<String>,
    /// ISO date, "YYYY-MM-DD".
    pub published: Option<String>,
    /// 1–10 (half stars); null for no rating. Personal, so per user.
    pub rating: Option<u8>,
    pub tags: Vec<String>,
    /// ISO 639-2 codes ("eng").
    pub languages: Vec<String>,
    pub identifiers: BTreeMap<String, String>,
    /// HTML; sanitized when saved.
    pub description_html: Option<String>,
    /// Custom fields by key ("pages"): null clears one. Fields left out are
    /// unchanged; the editor gets every field.
    #[serde(default)]
    pub custom: BTreeMap<String, Option<FieldValue>>,
}

/// What a successful edit did on disk.
#[derive(Debug, Default, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct EditOutcome {
    /// The book's folder moved (title or first author changed).
    pub moved: bool,
    /// Book files renamed to match the new title or author.
    pub renamed_files: usize,
}

impl Library {
    /// A book's current values, ready for the edit form.
    pub fn book_edit(&self, id: BookId) -> Result<BookEdit> {
        let b = self.book(id)?;
        Ok(BookEdit {
            title: b.title,
            title_sort: b.title_sort,
            authors: b.authors,
            author_sorts: BTreeMap::new(),
            series_index: b.series.as_ref().map_or(1.0, |s| s.1),
            series: b.series.map(|s| s.0),
            series_end: b.series_end,
            more_series: b.more_series,
            collects: b.collects,
            publisher: b.publisher,
            published: b.published.map(|d| d.to_string()),
            rating: b.rating,
            tags: b.tags,
            languages: b.languages,
            identifiers: b.identifiers,
            description_html: b.description_html,
            custom: self.custom_edit_values(id)?,
        })
    }

    /// Save an edit. Renames the book's folder and files if the title or first
    /// author changed. All-or-nothing across the database and the disk.
    pub fn update_book(&mut self, id: BookId, edit: &BookEdit) -> Result<EditOutcome> {
        let clean = Clean::from(edit)?;
        let custom = self.clean_custom(&edit.custom)?;
        let root = self.root.clone();
        let tx = self.conn.transaction()?;

        let old_path: String =
            tx.query_row("SELECT path FROM books WHERE id = ?", [id], |r| r.get(0)).map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Error::NoSuchBook(id),
                e => e.into(),
            })?;

        debug_assert!(!old_path.is_empty());
        write_metadata(&tx, id, &clean)?;
        write_custom(&tx, id, &custom)?;
        // Folder and file names follow the (new) title and first author.
        sync_names(&tx, &root, id, &clean.title, &clean.authors[0])?.commit(tx, &root)
    }

    /// Replace a book's cover with an image (any common format; stored as JPEG).
    pub fn set_cover(&mut self, id: BookId, image: &[u8]) -> Result<()> {
        let jpeg =
            images::as_jpeg(image).map_err(|e| Error::InvalidBook(format!("that image couldn't be read: {e}")))?;
        let dir = dir_for(&self.root, &self.book(id)?.path);
        fs::create_dir_all(&dir)?;
        // Write beside, then rename over: a crash never leaves half a cover.
        let tmp = dir.join(".cover.jpg.tmp");
        fs::write(&tmp, jpeg)?;
        fs::rename(&tmp, dir.join("cover.jpg"))?;
        self.conn
            .execute("UPDATE books SET cover_file = 'cover.jpg', modified_at = ? WHERE id = ?", params![now(), id])?;
        Ok(())
    }

    /// Remove a book's cover.
    pub fn remove_cover(&mut self, id: BookId) -> Result<()> {
        let dir = dir_for(&self.root, &self.book(id)?.path);
        match fs::remove_file(dir.join("cover.jpg")) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
            _ => {}
        }
        self.conn.execute("UPDATE books SET cover_file = NULL, modified_at = ? WHERE id = ?", params![now(), id])?;
        Ok(())
    }

    /// Remove one format from a book; the book and its other formats stay.
    /// `discard` disposes of the file (the app moves it to the Trash); it
    /// isn't called when the file is already missing. If it fails, nothing
    /// changes.
    pub fn remove_format(
        &mut self,
        id: BookId,
        format: &str,
        discard: impl FnOnce(&Path) -> std::io::Result<()>,
    ) -> Result<()> {
        let dir = dir_for(&self.root, &self.book(id)?.path);
        let tx = self.conn.transaction()?;
        let filename: String = tx
            .query_row("SELECT filename FROM files WHERE book_id = ? AND format = ?", params![id, format], |r| r.get(0))
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Error::InvalidBook(format!("this book has no {format} file")),
                e => e.into(),
            })?;
        tx.execute("DELETE FROM files WHERE book_id = ? AND format = ?", params![id, format])?;
        tx.execute("UPDATE books SET modified_at = ? WHERE id = ?", params![now(), id])?;
        let file = dir.join(&filename);
        if file.exists() {
            discard(&file)?;
        }
        tx.commit()?;
        Ok(())
    }
}

/// How an author sorts, for the edit form.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct AuthorSort {
    pub name: String,
    /// What they sort as now: their saved sort name, or the automatic one
    /// for a new author.
    pub sort: String,
    /// The automatic sort name ("Le Guin, Ursula K." → "Guin, Ursula K. Le").
    pub automatic: String,
}

impl Library {
    /// How each named author sorts (known authors by their saved sort name).
    pub fn author_sorts(&self, names_: &[String]) -> Result<Vec<AuthorSort>> {
        let mut stmt = self.conn.prepare_cached("SELECT sort_name FROM authors WHERE name = ?")?;
        names_
            .iter()
            .map(|n| n.trim())
            .filter(|n| !n.is_empty())
            .map(|name| {
                let automatic = names::author_sort(name);
                let saved: Option<String> =
                    rusqlite::OptionalExtension::optional(stmt.query_row([name], |r| r.get(0)))?;
                Ok(AuthorSort { name: name.to_owned(), sort: saved.unwrap_or_else(|| automatic.clone()), automatic })
            })
            .collect()
    }
}

/// A [`BookEdit`] validated and normalized.
struct Clean {
    title: String,
    title_sort: String,
    authors: Vec<String>,
    /// Name → sort name to save (already resolved from "automatic").
    author_sorts: BTreeMap<String, String>,
    series: Option<(String, f64)>,
    series_end: Option<f64>,
    more_series: Vec<SeriesEntry>,
    collects: Vec<BookId>,
    publisher: Option<String>,
    published: Option<NaiveDate>,
    rating: Option<u8>,
    tags: Vec<String>,
    languages: Vec<String>,
    identifiers: BTreeMap<String, String>,
    description: Option<String>,
}

impl Clean {
    fn from(e: &BookEdit) -> Result<Self> {
        let trimmed = |s: &Option<String>| s.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned);
        let list = |v: &[String]| {
            let mut out: Vec<String> = Vec::new();
            for s in v.iter().map(|s| s.trim()).filter(|s| !s.is_empty()) {
                if !out.iter().any(|o| o.eq_ignore_ascii_case(s)) {
                    out.push(s.to_owned());
                }
            }
            out
        };

        let title = e.title.trim().to_owned();
        if title.is_empty() {
            return Err(Error::InvalidBook("a book needs a title".into()));
        }
        if let Some(r) = e.rating
            && !(1..=10).contains(&r)
        {
            return Err(Error::InvalidBook(format!("rating {r} is outside 1–10")));
        }
        if !e.series_index.is_finite() || e.series_index < 0.0 {
            return Err(Error::InvalidBook("series number must be zero or more".into()));
        }
        let published = match trimmed(&e.published) {
            Some(d) => Some(
                d.parse::<NaiveDate>()
                    .map_err(|_| Error::InvalidBook(format!("“{d}” isn't a date (expected YYYY-MM-DD)")))?,
            ),
            None => None,
        };
        let mut authors = list(&e.authors);
        if authors.is_empty() {
            authors.push("Unknown".into());
        }
        let range_ok = |start: f64, end: Option<f64>| match end {
            Some(end) if !end.is_finite() || end <= start => {
                Err(Error::InvalidBook(format!("an omnibus range must end after it starts ({start}–{end})")))
            }
            _ => Ok(()),
        };
        let series = trimmed(&e.series);
        let series_end = series.as_ref().and(e.series_end);
        range_ok(e.series_index, series_end)?;
        let mut more_series: Vec<SeriesEntry> = Vec::new();
        for s in &e.more_series {
            let name = s.name.trim();
            let taken = |n: &str| n.eq_ignore_ascii_case(name);
            if name.is_empty() || series.as_deref().is_some_and(taken) || more_series.iter().any(|m| taken(&m.name)) {
                continue;
            }
            if !s.index.is_finite() || s.index < 0.0 {
                return Err(Error::InvalidBook(format!("the number in “{name}” must be zero or more")));
            }
            range_ok(s.index, s.end)?;
            more_series.push(SeriesEntry { name: name.to_owned(), index: s.index, end: s.end });
        }
        let mut collects: Vec<BookId> = Vec::new();
        for id in &e.collects {
            if !collects.contains(id) {
                collects.push(*id);
            }
        }
        let title_sort = match e.title_sort.trim() {
            "" => names::title_sort(&title),
            s => s.to_owned(),
        };
        let author_sorts = e
            .author_sorts
            .iter()
            .map(|(name, sort)| {
                let name = name.trim().to_owned();
                let sort = match sort.trim() {
                    "" => names::author_sort(&name),
                    s => s.to_owned(),
                };
                (name, sort)
            })
            .filter(|(name, _)| !name.is_empty())
            .collect();
        Ok(Self {
            title_sort,
            title,
            author_sorts,
            authors,
            series: series.map(|s| (s, e.series_index)),
            series_end,
            more_series,
            collects,
            publisher: trimmed(&e.publisher),
            published,
            rating: e.rating,
            tags: list(&e.tags),
            languages: list(&e.languages).into_iter().map(|l| l.to_ascii_lowercase()).collect(),
            identifiers: e
                .identifiers
                .iter()
                .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_owned()))
                .filter(|(k, v)| !k.is_empty() && !v.is_empty())
                .collect(),
            description: e.description_html.as_deref().map(html::sanitize).filter(|d| !d.trim().is_empty()),
        })
    }
}

fn write_metadata(tx: &Transaction<'_>, id: BookId, c: &Clean) -> Result<()> {
    let series = c.series.as_ref().map(|(name, _)| series_id(tx, name)).transpose()?;
    let publisher = c.publisher.as_deref().map(|p| publisher_id(tx, p)).transpose()?;
    tx.execute(
        "UPDATE books SET title = ?, title_sort = ?, series_id = ?, series_index = ?, series_end = ?, publisher_id = ?,
                          published = ?, description_html = ?, modified_at = ?
         WHERE id = ?",
        params![
            c.title,
            c.title_sort,
            series,
            c.series.as_ref().map(|(_, i)| *i),
            c.series_end,
            publisher,
            c.published.map(|d| d.to_string()),
            c.description,
            now(),
            id
        ],
    )?;

    tx.execute("DELETE FROM book_authors WHERE book_id = ?", [id])?;
    for (pos, name) in (0_i64..).zip(&c.authors) {
        let sort = c.author_sorts.get(name).map(String::as_str);
        let author = author_id(tx, name, sort)?;
        if let Some(sort) = sort {
            // An existing author keeps their row; their sort name changes everywhere.
            tx.execute("UPDATE authors SET sort_name = ? WHERE id = ?", params![sort, author])?;
        }
        tx.execute(
            "INSERT INTO book_authors (book_id, author_id, position) VALUES (?, ?, ?)",
            params![id, author, pos],
        )?;
    }
    tx.execute("DELETE FROM book_more_series WHERE book_id = ?", [id])?;
    for (pos, s) in (0_i64..).zip(&c.more_series) {
        let sid = series_id(tx, &s.name)?;
        tx.execute(
            "INSERT INTO book_more_series (book_id, series_id, position, number, number_end) VALUES (?, ?, ?, ?, ?)",
            params![id, sid, pos, s.index, s.end],
        )?;
    }
    tx.execute("DELETE FROM book_collects WHERE omnibus_id = ?", [id])?;
    for (pos, book) in (0_i64..).zip(c.collects.iter().filter(|b| **b != id)) {
        let exists: bool = tx.query_row("SELECT count(*) FROM books WHERE id = ?", [book], |r| r.get::<_, i64>(0))? > 0;
        if !exists {
            return Err(Error::InvalidBook(format!("book {book}, which this one collects, isn't in the library")));
        }
        tx.execute(
            "INSERT INTO book_collects (omnibus_id, book_id, position) VALUES (?, ?, ?)",
            params![id, book, pos],
        )?;
    }
    tx.execute("DELETE FROM book_tags WHERE book_id = ?", [id])?;
    for tag in &c.tags {
        let tag = tag_id(tx, tag)?;
        tx.execute("INSERT OR IGNORE INTO book_tags (book_id, tag_id) VALUES (?, ?)", params![id, tag])?;
    }
    tx.execute("DELETE FROM book_languages WHERE book_id = ?", [id])?;
    for (pos, lang) in (0_i64..).zip(&c.languages) {
        tx.execute("INSERT INTO book_languages (book_id, lang, position) VALUES (?, ?, ?)", params![id, lang, pos])?;
    }
    tx.execute("DELETE FROM identifiers WHERE book_id = ?", [id])?;
    for (scheme, value) in &c.identifiers {
        tx.execute("INSERT INTO identifiers (book_id, scheme, value) VALUES (?, ?, ?)", params![id, scheme, value])?;
    }
    tx.execute(
        "INSERT INTO user_books (user_id, book_id, rating) VALUES (?, ?, ?)
         ON CONFLICT (user_id, book_id) DO UPDATE SET rating = excluded.rating",
        params![LOCAL_USER, id, c.rating],
    )?;

    // Names no book uses any more.
    crate::library::prune_unused_names(tx)?;
    Ok(())
}

/// Book `id`'s folder and file names after [`sync_names`]: renamed in `tx` and
/// on disk, waiting for the caller to commit (or drop, which rolls back the
/// transaction; the renames are undone too, see [`Synced::commit`]).
pub(crate) struct Synced {
    renames: Renames,
    outcome: EditOutcome,
    /// The folder the book moved out of, to remove if now empty.
    left: Option<PathBuf>,
    /// Files the database lists that aren't on disk.
    pub missing: Vec<String>,
}

impl Synced {
    /// Commit `tx`. If that fails, put the renamed files back.
    pub(crate) fn commit(self, tx: Transaction<'_>, root: &Path) -> Result<EditOutcome> {
        if let Err(e) = tx.commit() {
            self.renames.undo();
            return Err(e.into());
        }
        if let Some(old_parent) = self.left.as_deref().and_then(Path::parent).filter(|p| *p != root) {
            let _ = fs::remove_dir(old_parent); // only succeeds if now empty
        }
        Ok(self.outcome)
    }
}

/// A book file's name now, and the name its metadata calls for.
pub(crate) struct FileName {
    pub format: String,
    pub current: String,
    pub wanted: String,
}

/// The folder (relative path) and file names book `id` should have for
/// `title` and `first_author`.
pub(crate) fn expected_names(
    tx: &Connection,
    id: BookId,
    title: &str,
    first_author: &str,
) -> Result<(String, Vec<FileName>)> {
    let mut stmt = tx.prepare_cached("SELECT format, filename FROM files WHERE book_id = ?")?;
    let files = stmt
        .query_map([id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .map(|row| {
            row.map(|(format, current)| {
                let wanted = paths::book_file(title, first_author, &format);
                FileName { format, current, wanted }
            })
        })
        .collect::<rusqlite::Result<_>>()?;
    Ok((paths::book_dir(first_author, title, id), files))
}

/// Rename book `id`'s folder and files to match `title` and `first_author`,
/// in `tx` and on disk. Disk renames are undone if any of them fails.
pub(crate) fn sync_names(
    tx: &Transaction<'_>,
    root: &Path,
    id: BookId,
    title: &str,
    first_author: &str,
) -> Result<Synced> {
    let old_path: String = tx.query_row("SELECT path FROM books WHERE id = ?", [id], |r| r.get(0))?;
    let (new_path, files) = expected_names(tx, id, title, first_author)?;
    let old_dir = dir_for(root, &old_path);
    let new_dir = dir_for(root, &new_path);

    let mut plan = Vec::new();
    let mut missing = Vec::new();
    for FileName { format, current, wanted } in &files {
        if !old_dir.join(current).is_file() {
            missing.push(format!("{old_path}/{current}"));
        }
        if current != wanted {
            plan.push((old_dir.join(current), old_dir.join(wanted)));
            tx.execute("UPDATE files SET filename = ? WHERE book_id = ? AND format = ?", params![wanted, id, format])?;
        }
    }
    let moved = old_path != new_path;
    if moved {
        tx.execute("UPDATE books SET path = ? WHERE id = ?", params![new_path, id])?;
    }
    let outcome = EditOutcome { moved, renamed_files: plan.len() };

    // Only touch the disk if the book's folder is there (files may be missing).
    let mut renames = Renames::default();
    if old_dir.is_dir() {
        let disk = (|| -> Result<()> {
            for (from, to) in &plan {
                if from.exists() {
                    renames.rename(from, to)?;
                }
            }
            if moved {
                if let Some(parent) = new_dir.parent() {
                    fs::create_dir_all(parent)?;
                }
                renames.rename(&old_dir, &new_dir)?;
            }
            Ok(())
        })();
        if let Err(e) = disk {
            renames.undo();
            return Err(e);
        }
    }
    Ok(Synced { renames, outcome, left: moved.then_some(old_dir), missing })
}

pub(crate) fn dir_for(root: &Path, rel: &str) -> PathBuf {
    rel.split('/').fold(root.to_path_buf(), |p, c| p.join(c))
}

/// Renames performed so far, so a failure can put everything back.
#[derive(Default)]
struct Renames(Vec<(PathBuf, PathBuf)>);

impl Renames {
    fn rename(&mut self, from: &Path, to: &Path) -> Result<()> {
        if to.exists() {
            return Err(Error::InvalidBook(format!("can't rename to {}: something is already there", to.display())));
        }
        fs::rename(from, to)?;
        self.0.push((from.to_path_buf(), to.to_path_buf()));
        Ok(())
    }

    fn undo(self) {
        for (from, to) in self.0.into_iter().rev() {
            let _ = fs::rename(&to, &from);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::import::add_files;

    /// A library with one book added from "Old Title - Old Author.epub".
    fn library_with_book() -> (tempfile::TempDir, Library, BookId) {
        let tmp = tempfile::tempdir().unwrap();
        let mut lib = Library::create(tmp.path().join("lib"), "Test").unwrap();
        let src = tmp.path().join("Old Title - Old Author.epub");
        fs::write(&src, "epub bytes").unwrap();
        let id = add_files(&mut lib, &[src], |_| {}).unwrap().added[0];
        (tmp, lib, id)
    }

    fn file_path(lib: &Library, id: BookId) -> PathBuf {
        let cat = lib.load_catalog().unwrap();
        let b = cat.get(id).unwrap();
        b.format_path(lib.root(), &b.formats[0])
    }

    #[test]
    fn omnibus_ranges_more_series_and_collects() {
        let tmp = tempfile::tempdir().unwrap();
        let mut lib = Library::create(tmp.path().join("lib"), "Test").unwrap();
        let mut add = |name: &str| {
            let src = tmp.path().join(format!("{name} - Philip Pullman.epub"));
            fs::write(&src, name).unwrap();
            crate::import::add_files(&mut lib, &[src], |_| {}).unwrap().added[0]
        };
        let (compass, knife, omnibus) = (add("The Golden Compass"), add("The Subtle Knife"), add("His Dark Materials"));
        for (id, n) in [(compass, 1.0), (knife, 2.0)] {
            let mut e = lib.book_edit(id).unwrap();
            e.series = Some("His Dark Materials".into());
            e.series_index = n;
            lib.update_book(id, &e).unwrap();
        }

        let mut e = lib.book_edit(omnibus).unwrap();
        e.series = Some("His Dark Materials".into());
        e.series_index = 1.0;
        e.series_end = Some(3.0);
        e.more_series = vec![
            SeriesEntry { name: "Omnibus Editions".into(), index: 12.0, end: None },
            SeriesEntry { name: "his dark materials".into(), index: 9.0, end: None }, // same as the main one: dropped
        ];
        e.collects = vec![compass, knife, compass, omnibus];
        lib.update_book(omnibus, &e).unwrap();

        let back = lib.book_edit(omnibus).unwrap();
        assert_eq!(back.series_end, Some(3.0));
        assert_eq!(back.more_series, [SeriesEntry { name: "Omnibus Editions".into(), index: 12.0, end: None }]);
        assert_eq!(back.collects, [compass, knife], "deduplicated, never itself");

        let cat = lib.load_catalog().unwrap();
        assert_eq!(cat.get(compass).unwrap().collected_in, [omnibus]);
        assert_eq!(cat.get(omnibus).unwrap().collects, [compass, knife]);
        // Sorted by series: the omnibus sits just before book 1.
        let mut books: Vec<_> = cat.books.iter().collect();
        magnitide_catalog::sort_books(&mut books, magnitide_catalog::SortKey::Series, magnitide_catalog::SortDir::Asc);
        assert_eq!(books.iter().map(|b| b.id).collect::<Vec<_>>(), [omnibus, compass, knife]);
        // Searchable by its other series.
        let q = magnitide_catalog::Query::parse("series:omnibus").unwrap();
        assert_eq!(cat.books.iter().filter(|b| q.matches(b)).count(), 1);

        // A backwards range is refused.
        let mut bad = lib.book_edit(omnibus).unwrap();
        bad.series_end = Some(1.0);
        assert!(lib.update_book(omnibus, &bad).is_err());
        // No series, no range; and its extra series' name is still in use.
        let mut e = lib.book_edit(omnibus).unwrap();
        e.series = None;
        lib.update_book(omnibus, &e).unwrap();
        let back = lib.book_edit(omnibus).unwrap();
        assert_eq!((back.series, back.series_end, back.more_series.len()), (None, None, 1));
    }

    #[test]
    fn author_sort_names_can_be_overridden_and_reset() {
        let tmp = tempfile::tempdir().unwrap();
        let mut lib = Library::create(tmp.path().join("lib"), "Test").unwrap();
        let src = tmp.path().join("The Dispossessed - Ursula K. Le Guin.epub");
        fs::write(&src, "x").unwrap();
        let id = crate::import::add_files(&mut lib, &[src], |_| {}).unwrap().added[0];
        let names_ = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();

        let sorts = lib.author_sorts(&names_(&["Ursula K. Le Guin", "New Person", " "])).unwrap();
        assert_eq!(sorts.len(), 2);
        assert_eq!((sorts[0].sort.as_str(), sorts[0].automatic.as_str()), ("Guin, Ursula K. Le", "Guin, Ursula K. Le"));
        assert_eq!(sorts[1].sort, "Person, New");

        let mut edit = lib.book_edit(id).unwrap();
        edit.author_sorts.insert("Ursula K. Le Guin".into(), " Le Guin, Ursula K. ".into());
        lib.update_book(id, &edit).unwrap();
        assert_eq!(lib.author_sorts(&names_(&["Ursula K. Le Guin"])).unwrap()[0].sort, "Le Guin, Ursula K.");
        let cat = lib.load_catalog().unwrap();
        assert_eq!(cat.get(id).unwrap().author_sort.as_deref(), Some("Le Guin, Ursula K."));

        // A later edit that doesn't mention it keeps it; an empty value resets it.
        let edit = lib.book_edit(id).unwrap();
        assert!(edit.author_sorts.is_empty());
        lib.update_book(id, &edit).unwrap();
        assert_eq!(lib.author_sorts(&names_(&["Ursula K. Le Guin"])).unwrap()[0].sort, "Le Guin, Ursula K.");
        let mut edit = lib.book_edit(id).unwrap();
        edit.author_sorts.insert("Ursula K. Le Guin".into(), String::new());
        lib.update_book(id, &edit).unwrap();
        assert_eq!(lib.author_sorts(&names_(&["Ursula K. Le Guin"])).unwrap()[0].sort, "Guin, Ursula K. Le");
    }

    #[test]
    fn edits_round_trip_and_unused_names_are_removed() {
        let (_tmp, mut lib, id) = library_with_book();
        let mut e = lib.book_edit(id).unwrap();
        e.title = "The Dispossessed".into();
        e.title_sort = String::new();
        e.authors = vec!["Ursula K. Le Guin".into(), "  ".into(), "ursula k. le guin".into()];
        e.series = Some("Hainish Cycle".into());
        e.series_index = 5.0;
        e.rating = Some(10);
        e.tags = vec!["SF".into(), "Classics".into(), "sf".into()];
        e.published = Some("1974-05-01".into());
        e.identifiers = BTreeMap::from([("ISBN".into(), " 9780061054884 ".into()), ("empty".into(), "".into())]);
        e.description_html = Some("<p>An <i>ambiguous</i> utopia<script>x()</script></p>".into());
        lib.update_book(id, &e).unwrap();

        let b = lib.book(id).unwrap();
        assert_eq!(b.title_sort, "Dispossessed, The", "derived when left empty");
        assert_eq!(b.authors, ["Ursula K. Le Guin"], "blank and repeated names dropped");
        assert_eq!(b.series, Some(("Hainish Cycle".into(), 5.0)));
        assert_eq!(b.rating, Some(10));
        assert_eq!(b.tags, ["Classics", "SF"]);
        assert_eq!(b.published, NaiveDate::from_ymd_opt(1974, 5, 1));
        assert_eq!(b.identifiers, BTreeMap::from([("isbn".into(), "9780061054884".into())]));
        assert_eq!(b.description_html.as_deref(), Some("<p>An <i>ambiguous</i> utopia</p>"));

        let old_author: i64 =
            lib.conn.query_row("SELECT count(*) FROM authors WHERE name = 'Old Author'", [], |r| r.get(0)).unwrap();
        assert_eq!(old_author, 0, "unused author removed");
    }

    #[test]
    fn renames_folder_and_files_to_match() {
        let (_tmp, mut lib, id) = library_with_book();
        let old_dir = lib.root().join("Old Author");
        assert!(file_path(&lib, id).ends_with("Old Author/Old Title (1)/Old Title - Old Author.epub"));

        let mut e = lib.book_edit(id).unwrap();
        e.title = "New Title".into();
        e.authors = vec!["New Author".into()];
        let outcome = lib.update_book(id, &e).unwrap();

        assert_eq!(outcome, EditOutcome { moved: true, renamed_files: 1 });
        let path = file_path(&lib, id);
        assert!(path.ends_with("New Author/New Title (1)/New Title - New Author.epub"), "{}", path.display());
        assert_eq!(fs::read_to_string(&path).unwrap(), "epub bytes");
        assert!(!old_dir.exists(), "empty old author folder removed");
    }

    #[test]
    fn a_failed_rename_leaves_everything_as_it_was() {
        let (_tmp, mut lib, id) = library_with_book();
        let before = lib.book(id).unwrap();
        // Block the new folder name with a file.
        fs::create_dir_all(lib.root().join("New Author")).unwrap();
        fs::write(lib.root().join("New Author/New Title (1)"), "in the way").unwrap();

        let mut e = lib.book_edit(id).unwrap();
        e.title = "New Title".into();
        e.authors = vec!["New Author".into()];
        assert!(lib.update_book(id, &e).is_err());

        let after = lib.book(id).unwrap();
        assert_eq!((after.title, after.path, after.authors), (before.title, before.path, before.authors));
        let path = file_path(&lib, id);
        assert!(
            path.ends_with("Old Author/Old Title (1)/Old Title - Old Author.epub") && path.is_file(),
            "files renamed back"
        );
    }

    #[test]
    fn validates() {
        let (_tmp, mut lib, id) = library_with_book();
        let base = lib.book_edit(id).unwrap();
        let bad = |f: &dyn Fn(&mut BookEdit)| {
            let mut e = base.clone();
            f(&mut e);
            e
        };
        for e in [
            bad(&|e| e.title = "  ".into()),
            bad(&|e| e.rating = Some(11)),
            bad(&|e| e.published = Some("May 1974".into())),
            bad(&|e| e.series_index = -1.0),
        ] {
            assert!(matches!(lib.update_book(id, &e), Err(Error::InvalidBook(_))), "{e:?}");
        }
        assert!(matches!(lib.update_book(999, &base), Err(Error::NoSuchBook(999))));
    }

    #[test]
    fn sets_and_removes_covers() {
        let (_tmp, mut lib, id) = library_with_book();
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbImage::new(3, 4).write_to(&mut png, image::ImageFormat::Png).unwrap();
        lib.set_cover(id, png.get_ref()).unwrap();

        let cat = lib.load_catalog().unwrap();
        let cover = cat.get(id).unwrap().cover_path(lib.root()).unwrap();
        assert!(fs::read(&cover).unwrap().starts_with(&[0xFF, 0xD8, 0xFF]), "stored as JPEG");
        assert!(matches!(lib.set_cover(id, b"not an image"), Err(Error::InvalidBook(_))));

        lib.remove_cover(id).unwrap();
        assert!(!cover.exists());
        assert!(!lib.load_catalog().unwrap().get(id).unwrap().has_cover);
    }

    #[test]
    fn removes_a_format() {
        let (tmp, mut lib, id) = library_with_book();
        let kepub = tmp.path().join("Old Title - Old Author.kepub");
        fs::write(&kepub, "kepub bytes").unwrap();
        add_files(&mut lib, &[kepub], |_| {}).unwrap();
        let formats = |lib: &Library| -> Vec<String> {
            lib.load_catalog().unwrap().get(id).unwrap().formats.iter().map(|f| f.format.clone()).collect()
        };
        assert_eq!(formats(&lib), ["EPUB", "KEPUB"]);
        let epub = file_path(&lib, id);

        // A failed discard changes nothing.
        let refused = lib.remove_format(id, "EPUB", |_| Err(std::io::Error::other("no trash")));
        assert!(refused.is_err());
        assert!(epub.exists());
        assert_eq!(formats(&lib), ["EPUB", "KEPUB"]);

        let mut discarded = None;
        lib.remove_format(id, "EPUB", |p| {
            discarded = Some(p.to_owned());
            fs::remove_file(p)
        })
        .unwrap();
        assert_eq!(discarded.as_deref(), Some(epub.as_path()));
        assert!(!epub.exists());
        assert_eq!(formats(&lib), ["KEPUB"]);
        assert!(matches!(lib.remove_format(id, "EPUB", |_| Ok(())), Err(Error::InvalidBook(_))));

        // A file already gone from disk: only the record goes.
        fs::remove_file(file_path(&lib, id)).unwrap();
        lib.remove_format(id, "KEPUB", |_| panic!("nothing to discard")).unwrap();
        assert!(formats(&lib).is_empty());
        assert!(lib.book(id).is_ok(), "the book stays");
    }
}
