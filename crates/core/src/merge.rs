//! Merging two records of the same book (say, one book added twice in
//! different formats) into one.

use std::fs;
use std::path::PathBuf;

use rusqlite::params;
use serde::Serialize;

use crate::edit::dir_for;
use crate::error::{Error, Result};
use crate::library::{BookId, Library, now};
use crate::paths;

/// What a merge did.
#[derive(Debug, Default, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct MergeOutcome {
    /// Files moved into the kept book.
    pub formats_moved: usize,
    /// The kept book had no cover and took the other's.
    pub cover_taken: bool,
}

impl Library {
    /// Merge book `other` into book `keep`, then remove `other`.
    ///
    /// `other`'s files move into `keep`'s folder, named for `keep`; `keep`
    /// takes `other`'s cover if it has none, any tags and identifier schemes
    /// it lacks, and its shelves. Everything else of `keep` stays. If both have a file
    /// in the same format nothing happens (no file is ever discarded). All or
    /// nothing across the database and the disk.
    pub fn merge_books(&mut self, keep: BookId, other: BookId) -> Result<MergeOutcome> {
        if keep == other {
            return Err(Error::InvalidBook("a book can't be merged into itself".into()));
        }
        let (keep_book, other_book) = (self.book(keep)?, self.book(other)?);
        let files = |id: BookId| -> Result<Vec<(String, String)>> {
            let mut stmt = self.conn.prepare("SELECT format, filename FROM files WHERE book_id = ?")?;
            let rows = stmt.query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        };
        let (keep_files, other_files) = (files(keep)?, files(other)?);
        if let Some((format, _)) = other_files.iter().find(|(f, _)| keep_files.iter().any(|(k, _)| k == f)) {
            return Err(Error::InvalidBook(format!(
                "both books have a {format} file; remove one of them before merging"
            )));
        }

        let root = self.root.clone();
        let (keep_dir, other_dir) = (dir_for(&root, &keep_book.path), dir_for(&root, &other_book.path));
        let first_author = keep_book.authors.first().map_or("Unknown", String::as_str);
        let tx = self.conn.transaction()?;
        let mut moved: Vec<(PathBuf, PathBuf)> = Vec::new();
        let mut outcome = MergeOutcome::default();

        let result = (|| -> Result<()> {
            fs::create_dir_all(&keep_dir)?;
            for (format, filename) in &other_files {
                let wanted = paths::book_file(&keep_book.title, first_author, format);
                let (from, to) = (other_dir.join(filename), keep_dir.join(&wanted));
                if to.exists() {
                    return Err(Error::InvalidBook(format!("{} already exists", to.display())));
                }
                fs::rename(&from, &to)?;
                moved.push((from, to));
                tx.execute(
                    "UPDATE files SET book_id = ?, filename = ? WHERE book_id = ? AND format = ?",
                    params![keep, wanted, other, format],
                )?;
                outcome.formats_moved += 1;
            }
            if keep_book.cover_file.is_none()
                && let Some(cover) = &other_book.cover_file
            {
                let (from, to) = (other_dir.join(cover), keep_dir.join("cover.jpg"));
                if from.is_file() && !to.exists() {
                    fs::rename(&from, &to)?;
                    moved.push((from, to));
                    tx.execute("UPDATE books SET cover_file = 'cover.jpg' WHERE id = ?", [keep])?;
                    outcome.cover_taken = true;
                }
            }
            tx.execute(
                "INSERT OR IGNORE INTO book_tags (book_id, tag_id) SELECT ?1, tag_id FROM book_tags WHERE book_id = ?2",
                params![keep, other],
            )?;
            tx.execute(
                "INSERT INTO identifiers (book_id, scheme, value)
                 SELECT ?1, scheme, value FROM identifiers
                 WHERE book_id = ?2 AND scheme NOT IN (SELECT scheme FROM identifiers WHERE book_id = ?1)",
                params![keep, other],
            )?;
            tx.execute(
                "INSERT OR IGNORE INTO book_shelves (shelf_id, book_id, added_at)
                 SELECT shelf_id, ?1, added_at FROM book_shelves WHERE book_id = ?2",
                params![keep, other],
            )?;
            // Omnibus links and further series follow the kept book.
            tx.execute(
                "UPDATE OR IGNORE book_collects SET book_id = ?1 WHERE book_id = ?2 AND omnibus_id <> ?1",
                params![keep, other],
            )?;
            tx.execute(
                "UPDATE OR IGNORE book_collects SET omnibus_id = ?1 WHERE omnibus_id = ?2 AND book_id <> ?1",
                params![keep, other],
            )?;
            tx.execute(
                "INSERT OR IGNORE INTO book_more_series (book_id, series_id, position, number, number_end)
                 SELECT ?1, series_id, position + 1000, number, number_end FROM book_more_series
                 WHERE book_id = ?2 AND series_id IS NOT (SELECT series_id FROM books WHERE id = ?1)",
                params![keep, other],
            )?;
            tx.execute("UPDATE books SET modified_at = ? WHERE id = ?", params![now(), keep])?;
            // Its authors, tags, files rows and the like go with it (ON DELETE CASCADE).
            tx.execute("DELETE FROM books WHERE id = ?", [other])?;
            crate::library::prune_unused_names(&tx)?;
            Ok(())
        })();

        if let Err(e) = result.and_then(|()| tx.commit().map_err(Error::from)) {
            for (from, to) in moved.iter().rev() {
                let _ = fs::rename(to, from);
            }
            return Err(e);
        }
        // The other book's folder: only a leftover cover is removed, and the
        // folders only if that leaves them empty.
        let _ = fs::remove_file(other_dir.join("cover.jpg"));
        if fs::remove_dir(&other_dir).is_ok()
            && let Some(author_dir) = other_dir.parent().filter(|p| *p != root)
        {
            let _ = fs::remove_dir(author_dir);
        }
        Ok(outcome)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::add_files;

    fn add(lib: &mut Library, dir: &std::path::Path, name: &str) -> BookId {
        let src = dir.join(name);
        fs::write(&src, format!("{name} bytes")).unwrap();
        add_files(lib, &[src], |_| {}).unwrap().added[0]
    }

    #[test]
    fn merges_formats_and_removes_the_other_book() {
        let tmp = tempfile::tempdir().unwrap();
        let mut lib = Library::create(tmp.path().join("lib"), "Test").unwrap();
        let keep = add(&mut lib, tmp.path(), "The Oleander Sword - Tasha Suri.epub");
        // The old bug: the same book again as a separate record.
        let other = add(&mut lib, tmp.path(), "Oleander (copy) - Someone Else.kepub");
        lib.set_cover(other, &{
            let mut buf = std::io::Cursor::new(Vec::new());
            image::RgbImage::new(2, 3).write_to(&mut buf, image::ImageFormat::Png).unwrap();
            buf.into_inner()
        })
        .unwrap();
        let other_dir = dir_for(lib.root(), &lib.book(other).unwrap().path);

        let outcome = lib.merge_books(keep, other).unwrap();
        assert_eq!(outcome, MergeOutcome { formats_moved: 1, cover_taken: true });

        let cat = lib.load_catalog().unwrap();
        assert_eq!(cat.books.len(), 1);
        let b = cat.get(keep).unwrap();
        assert!(b.has_cover);
        for f in &b.formats {
            assert!(b.format_path(lib.root(), f).is_file(), "{f:?}");
        }
        let mut names: Vec<&str> = b.formats.iter().map(|f| f.name.as_str()).collect();
        names.dedup();
        assert_eq!(names, ["The Oleander Sword - Tasha Suri"], "named for the kept book");
        assert!(!other_dir.exists() && !other_dir.parent().unwrap().exists(), "empty folders removed");
        assert!(lib.book(other).is_err());
        let orphan: i64 =
            lib.conn.query_row("SELECT count(*) FROM authors WHERE name = 'Someone Else'", [], |r| r.get(0)).unwrap();
        assert_eq!(orphan, 0);
    }

    #[test]
    fn refuses_when_both_have_a_format() {
        let tmp = tempfile::tempdir().unwrap();
        let mut lib = Library::create(tmp.path().join("lib"), "Test").unwrap();
        let a = add(&mut lib, tmp.path(), "A - X.epub");
        let b = add(&mut lib, tmp.path(), "B - Y.epub");
        assert!(lib.merge_books(a, b).unwrap_err().to_string().contains("both books have a EPUB"));
        assert_eq!(lib.load_catalog().unwrap().books.len(), 2, "nothing removed");
        assert!(lib.merge_books(a, a).is_err());
    }
}
