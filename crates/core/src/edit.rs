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
use rusqlite::{Transaction, params};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::library::{BookId, LOCAL_USER, Library, author_id, now, publisher_id, series_id, tag_id};
use crate::{html, images, names, paths};

/// Every editable field of a book, as the edit form shows and returns it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct BookEdit {
    pub title: String,
    /// Empty means "derive from the title" ("The Hobbit" → "Hobbit, The").
    pub title_sort: String,
    /// In display order; empty means "Unknown".
    pub authors: Vec<String>,
    pub series: Option<String>,
    pub series_index: f64,
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
            series_index: b.series.as_ref().map_or(1.0, |s| s.1),
            series: b.series.map(|s| s.0),
            publisher: b.publisher,
            published: b.published.map(|d| d.to_string()),
            rating: b.rating,
            tags: b.tags,
            languages: b.languages,
            identifiers: b.identifiers,
            description_html: b.description_html,
        })
    }

    /// Save an edit. Renames the book's folder and files if the title or first
    /// author changed. All-or-nothing across the database and the disk.
    pub fn update_book(&mut self, id: BookId, edit: &BookEdit) -> Result<EditOutcome> {
        let clean = Clean::from(edit)?;
        let root = self.root.clone();
        let tx = self.conn.transaction()?;

        let old_path: String =
            tx.query_row("SELECT path FROM books WHERE id = ?", [id], |r| r.get(0)).map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Error::NoSuchBook(id),
                e => e.into(),
            })?;

        write_metadata(&tx, id, &clean)?;

        // Folder and file names follow the (new) title and first author.
        let new_path = paths::book_dir(&clean.authors[0], &clean.title, id);
        let file_rows: Vec<(String, String)> = {
            let mut stmt = tx.prepare("SELECT format, filename FROM files WHERE book_id = ?")?;
            stmt.query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?
        };
        let old_dir = dir_for(&root, &old_path);
        let new_dir = dir_for(&root, &new_path);

        let mut plan = Vec::new();
        for (format, filename) in &file_rows {
            let wanted = paths::book_file(&clean.title, &clean.authors[0], format);
            if *filename != wanted {
                plan.push((old_dir.join(filename), old_dir.join(&wanted)));
                tx.execute(
                    "UPDATE files SET filename = ? WHERE book_id = ? AND format = ?",
                    params![wanted, id, format],
                )?;
            }
        }
        let renamed_files = plan.len();
        let moved = old_path != new_path;
        if moved {
            tx.execute("UPDATE books SET path = ? WHERE id = ?", params![new_path, id])?;
        }

        // Only touch the disk if the book's folder is there (files may be missing).
        let mut done = Renames::default();
        if old_dir.is_dir() {
            let disk = (|| -> Result<()> {
                for (from, to) in &plan {
                    if from.exists() {
                        done.rename(from, to)?;
                    }
                }
                if moved {
                    if let Some(parent) = new_dir.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    done.rename(&old_dir, &new_dir)?;
                }
                Ok(())
            })();
            if let Err(e) = disk {
                done.undo();
                return Err(e);
            }
        }

        if let Err(e) = tx.commit() {
            done.undo();
            return Err(e.into());
        }
        // Tidy an author folder the book has left, if it's now empty.
        if moved && let Some(old_parent) = old_dir.parent().filter(|p| *p != root) {
            let _ = fs::remove_dir(old_parent);
        }
        Ok(EditOutcome { moved, renamed_files })
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
}

/// A [`BookEdit`] validated and normalized.
struct Clean {
    title: String,
    title_sort: String,
    authors: Vec<String>,
    series: Option<(String, f64)>,
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
        let title_sort = match e.title_sort.trim() {
            "" => names::title_sort(&title),
            s => s.to_owned(),
        };
        Ok(Self {
            title_sort,
            title,
            authors,
            series: trimmed(&e.series).map(|s| (s, e.series_index)),
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
        "UPDATE books SET title = ?, title_sort = ?, series_id = ?, series_index = ?, publisher_id = ?,
                          published = ?, description_html = ?, modified_at = ?
         WHERE id = ?",
        params![
            c.title,
            c.title_sort,
            series,
            c.series.as_ref().map(|(_, i)| *i),
            publisher,
            c.published.map(|d| d.to_string()),
            c.description,
            now(),
            id
        ],
    )?;

    tx.execute("DELETE FROM book_authors WHERE book_id = ?", [id])?;
    for (pos, name) in (0_i64..).zip(&c.authors) {
        let author = author_id(tx, name, None)?;
        tx.execute(
            "INSERT INTO book_authors (book_id, author_id, position) VALUES (?, ?, ?)",
            params![id, author, pos],
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
    tx.execute_batch(
        "DELETE FROM authors WHERE id NOT IN (SELECT author_id FROM book_authors);
         DELETE FROM tags WHERE id NOT IN (SELECT tag_id FROM book_tags);
         DELETE FROM series WHERE id NOT IN (SELECT series_id FROM books WHERE series_id IS NOT NULL);
         DELETE FROM publishers WHERE id NOT IN (SELECT publisher_id FROM books WHERE publisher_id IS NOT NULL);",
    )?;
    Ok(())
}

fn dir_for(root: &Path, rel: &str) -> PathBuf {
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
}
