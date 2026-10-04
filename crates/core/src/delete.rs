//! Deleting books from the library.

use std::fs;
use std::path::{Component, Path};

use serde::Serialize;

use crate::edit::dir_for;
use crate::error::{Error, Result};
use crate::library::{BookId, Library};

/// What a delete did.
#[derive(Debug, Default, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct DeleteReport {
    pub deleted: Vec<BookId>,
    /// "Title: reason" for each book that couldn't be deleted (it stays).
    pub failed: Vec<String>,
}

impl Library {
    /// Delete books: each one's record goes, and its folder (files, cover
    /// and anything else in it) is handed to `discard` (the app moves it to
    /// the Trash). Book by book: one that fails stays as it was and the rest
    /// carry on. Omnibus links, reading state and the like go with the record;
    /// names no book uses any more are removed.
    pub fn delete_books(
        &mut self,
        ids: &[BookId],
        mut discard: impl FnMut(&Path) -> std::io::Result<()>,
    ) -> Result<DeleteReport> {
        let mut report = DeleteReport::default();
        for &id in ids {
            let book = match self.book(id) {
                Ok(b) => b,
                Err(Error::NoSuchBook(_)) => continue,
                Err(e) => return Err(e),
            };
            match self.delete_one(id, &book.path, &mut discard) {
                Ok(()) => report.deleted.push(id),
                Err(e) => report.failed.push(format!("{}: {e}", book.title)),
            }
        }
        Ok(report)
    }

    fn delete_one(
        &mut self,
        id: BookId,
        rel: &str,
        discard: &mut impl FnMut(&Path) -> std::io::Result<()>,
    ) -> Result<()> {
        // Never hand over anything but a folder inside the library.
        let safe = !rel.is_empty() && Path::new(rel).components().all(|c| matches!(c, Component::Normal(_)));
        if !safe {
            return Err(Error::InvalidBook(format!("its folder {rel:?} isn't inside the library")));
        }
        // Nor a folder another book lives under.
        let shared: i64 = self.conn.query_row(
            "SELECT count(*) FROM books WHERE id <> ?1 AND substr(path, 1, length(?2) + 1) = ?2 || '/'",
            rusqlite::params![id, rel],
            |r| r.get(0),
        )?;
        if shared > 0 {
            return Err(Error::InvalidBook(format!("its folder {rel:?} holds other books too")));
        }
        let dir = dir_for(&self.root, rel);
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM books WHERE id = ?", [id])?;
        crate::library::prune_unused_names(&tx)?;
        if dir.exists() {
            discard(&dir)?;
        }
        tx.commit()?;
        // The author folder, if that left it empty.
        if let Some(parent) = dir.parent().filter(|p| *p != self.root) {
            let _ = fs::remove_dir(parent);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::add_files;

    fn add(lib: &mut Library, dir: &Path, name: &str) -> BookId {
        let src = dir.join(name);
        fs::write(&src, format!("{name} bytes")).unwrap();
        add_files(lib, &[src], |_| {}).unwrap().added[0]
    }

    #[test]
    fn deletes_books_and_their_folders() {
        let tmp = tempfile::tempdir().unwrap();
        let mut lib = Library::create(tmp.path().join("lib"), "Test").unwrap();
        let a = add(&mut lib, tmp.path(), "The Golden Compass - Philip Pullman.epub");
        let b = add(&mut lib, tmp.path(), "The Subtle Knife - Philip Pullman.epub");
        let c = add(&mut lib, tmp.path(), "Red Sister - Mark Lawrence.epub");
        let folder = |lib: &Library, id| dir_for(lib.root(), &lib.book(id).unwrap().path);
        let (dir_a, dir_c) = (folder(&lib, a), folder(&lib, c));
        // An omnibus link to a deleted book goes with it.
        lib.conn.execute("INSERT INTO book_collects (omnibus_id, book_id, position) VALUES (?, ?, 0)", [b, a]).unwrap();

        let mut discarded = Vec::new();
        let report = lib
            .delete_books(&[a, c, 999], |p| {
                discarded.push(p.to_owned());
                fs::remove_dir_all(p)
            })
            .unwrap();
        assert_eq!(report, DeleteReport { deleted: vec![a, c], failed: vec![] });
        assert_eq!(discarded, [dir_a.clone(), dir_c.clone()]);
        assert!(!dir_a.exists() && !dir_c.exists());
        assert!(!dir_c.parent().unwrap().exists(), "Mark Lawrence's empty folder removed");
        assert!(dir_a.parent().unwrap().exists(), "Philip Pullman still has a book");
        assert!(matches!(lib.book(a), Err(Error::NoSuchBook(_))));
        assert!(lib.book(b).unwrap().collects.is_empty());
        let authors: i64 = lib.conn.query_row("SELECT count(*) FROM authors", [], |r| r.get(0)).unwrap();
        assert_eq!(authors, 1, "unused authors pruned");
    }

    #[test]
    fn a_failed_discard_keeps_the_book() {
        let tmp = tempfile::tempdir().unwrap();
        let mut lib = Library::create(tmp.path().join("lib"), "Test").unwrap();
        let a = add(&mut lib, tmp.path(), "Grey Sister - Mark Lawrence.epub");
        let report = lib.delete_books(&[a], |_| Err(std::io::Error::other("no trash"))).unwrap();
        assert!(report.deleted.is_empty());
        assert_eq!(report.failed, ["Grey Sister: no trash"]);
        assert!(lib.book(a).is_ok());
        assert!(dir_for(lib.root(), &lib.book(a).unwrap().path).exists());
    }

    #[test]
    fn never_discards_outside_the_library() {
        let tmp = tempfile::tempdir().unwrap();
        let mut lib = Library::create(tmp.path().join("lib"), "Test").unwrap();
        let a = add(&mut lib, tmp.path(), "Holy Sister - Mark Lawrence.epub");
        for bad in ["", "..", "Mark Lawrence/../..", "/etc"] {
            lib.conn.execute("UPDATE books SET path = ? WHERE id = ?", rusqlite::params![bad, a]).unwrap();
            let report = lib.delete_books(&[a], |p| panic!("discarded {}", p.display())).unwrap();
            assert_eq!(report.failed.len(), 1, "{bad:?}");
        }
        // Nor a folder holding another book (paths are unique, so not one shared outright).
        let b = add(&mut lib, tmp.path(), "Red Sister - Mark Lawrence.epub");
        let author_dir = lib.book(b).unwrap().path.split('/').next().unwrap().to_owned();
        lib.conn.execute("UPDATE books SET path = ? WHERE id = ?", rusqlite::params![author_dir, a]).unwrap();
        let report = lib.delete_books(&[a], |p| panic!("discarded {}", p.display())).unwrap();
        assert_eq!(report.failed.len(), 1);
    }
}
