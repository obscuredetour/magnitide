//! "Tidy file names": bring every book's folder and file names in line with
//! its title and first author, and report files missing from disk.
//!
//! Edits already keep names in sync; this catches libraries whose names
//! drifted (renamed outside Magnitide, copied from elsewhere, or named by an
//! older version with different naming rules). Each book is renamed in its
//! own transaction, all-or-nothing with the disk, like an edit.

use serde::Serialize;

use crate::edit::{FileName, expected_names, sync_names};
use crate::error::Result;
use crate::library::{BookId, Library};

/// What tidying would do, without doing it.
#[derive(Debug, Default, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct TidyPreview {
    /// Books whose folder or file names don't match their metadata.
    pub books: usize,
    /// Book files that would be renamed.
    pub files: usize,
}

#[derive(Debug, Default, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct TidyReport {
    pub checked: usize,
    /// Books moved to a new folder.
    pub books_moved: usize,
    pub files_renamed: usize,
    /// Files the library lists that aren't on disk ("Author/Title (id)/file").
    pub missing: Vec<String>,
    /// Books that couldn't be tidied, with the reason; left exactly as they were.
    pub failed: Vec<String>,
}

impl Library {
    /// How many books tidying would rename.
    pub fn tidy_preview(&self) -> Result<TidyPreview> {
        let mut preview = TidyPreview::default();
        for (id, path, title, author) in self.name_sources()? {
            let (wanted_path, files) = expected_names(&self.conn, id, &title, &author)?;
            let renames = files.iter().filter(|f: &&FileName| f.current != f.wanted).count();
            if renames > 0 || wanted_path != path {
                preview.books += 1;
                preview.files += renames;
            }
        }
        Ok(preview)
    }

    /// Rename every book whose names don't match its metadata.
    pub fn tidy_file_names(&mut self, mut progress: impl FnMut(usize, usize)) -> Result<TidyReport> {
        let books = self.name_sources()?;
        let root = self.root.clone();
        let mut report = TidyReport { checked: books.len(), ..Default::default() };
        for (done, (id, _, title, author)) in books.into_iter().enumerate() {
            progress(done, report.checked);
            let tx = self.conn.transaction()?;
            let result = sync_names(&tx, &root, id, &title, &author).and_then(|synced| {
                report.missing.extend(synced.missing.iter().cloned());
                synced.commit(tx, &root)
            });
            match result {
                Ok(outcome) => {
                    report.books_moved += usize::from(outcome.moved);
                    report.files_renamed += outcome.renamed_files;
                }
                Err(e) => report.failed.push(format!("“{title}”: {e}")),
            }
        }
        progress(report.checked, report.checked);
        Ok(report)
    }

    /// (id, current folder, title, first author) for every book.
    fn name_sources(&self) -> Result<Vec<(BookId, String, String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT b.id, b.path, b.title, coalesce(a.name, 'Unknown')
             FROM books b
             LEFT JOIN book_authors ba ON ba.book_id = b.id AND ba.position = 0
             LEFT JOIN authors a ON a.id = ba.author_id
             ORDER BY b.id",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use super::*;
    use crate::import::add_files;

    fn add(lib: &mut Library, dir: &Path, name: &str) -> BookId {
        let src = dir.join(name);
        fs::write(&src, "bytes").unwrap();
        add_files(lib, &[src], |_| {}).unwrap().added[0]
    }

    #[test]
    fn renames_drifted_books_and_reports_missing_files() {
        let tmp = tempfile::tempdir().unwrap();
        let mut lib = Library::create(tmp.path().join("lib"), "Test").unwrap();
        let tidy_id = add(&mut lib, tmp.path(), "Already Tidy - Some Author.epub");
        let drift_id = add(&mut lib, tmp.path(), "Drifted - Some Author.epub");
        let gone_id = add(&mut lib, tmp.path(), "Gone - Other Author.pdf");
        assert_eq!(lib.tidy_preview().unwrap(), TidyPreview::default(), "freshly added books are tidy");

        // Simulate drift: the title changed in the database but not on disk
        // (e.g. renamed by an older version or another tool).
        lib.conn.execute("UPDATE books SET title = 'Renamed' WHERE id = ?", [drift_id]).unwrap();
        // And one book's file was deleted outside the app.
        let gone = lib.load_catalog().unwrap();
        let gone_book = gone.get(gone_id).unwrap();
        fs::remove_file(gone_book.format_path(lib.root(), &gone_book.formats[0])).unwrap();

        assert_eq!(lib.tidy_preview().unwrap(), TidyPreview { books: 1, files: 1 });
        let report = lib.tidy_file_names(|_, _| {}).unwrap();
        assert_eq!((report.checked, report.books_moved, report.files_renamed), (3, 1, 1));
        assert!(report.failed.is_empty(), "{:?}", report.failed);
        assert_eq!(report.missing, ["Other Author/Gone (3)/Gone - Other Author.pdf"]);

        let cat = lib.load_catalog().unwrap();
        let b = cat.get(drift_id).unwrap();
        let path = b.format_path(lib.root(), &b.formats[0]);
        assert!(
            path.ends_with("Some Author/Renamed (2)/Renamed - Some Author.epub") && path.is_file(),
            "{}",
            path.display()
        );
        assert_eq!(cat.get(tidy_id).unwrap().path, "Some Author/Already Tidy (1)");
        assert_eq!(lib.tidy_preview().unwrap(), TidyPreview::default(), "tidy afterwards");
    }
}
