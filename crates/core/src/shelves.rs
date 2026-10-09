//! Shelves: named collections of books the user curates (Magnitide only).
//! They show in the sidebar, filter the library (`shelf:"Name"`), and are
//! written to e-readers as collections.

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::library::{BookId, Library, now};

pub type ShelfId = i64;

/// A shelf, with how many books are on it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct Shelf {
    pub id: ShelfId,
    pub name: String,
    pub count: usize,
}

/// What shelves are made from (`make_shelves`).
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "lowercase")]
pub enum ShelfSource {
    Series,
    Authors,
}

/// What `make_shelves` did.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ShelvesMade {
    /// Shelves created (a shelf of the same name already there is reused).
    pub shelves: usize,
    /// Books put on a shelf they weren't on.
    pub books: usize,
}

/// A shelf name: trimmed, not empty, at most 100 characters.
fn clean_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::InvalidBook("a shelf needs a name".into()));
    }
    if name.chars().count() > 100 {
        return Err(Error::InvalidBook("a shelf name can be at most 100 characters".into()));
    }
    Ok(name.to_owned())
}

impl Library {
    /// Every shelf, by name (case-insensitively), with its book count.
    pub fn shelves(&self) -> Result<Vec<Shelf>> {
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.name, count(bs.book_id) FROM shelves s
             LEFT JOIN book_shelves bs ON bs.shelf_id = s.id
             GROUP BY s.id ORDER BY s.name COLLATE NOCASE",
        )?;
        let rows =
            stmt.query_map([], |r| Ok(Shelf { id: r.get(0)?, name: r.get(1)?, count: r.get::<_, i64>(2)? as usize }))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// A new, empty shelf. Names are unique, ignoring case.
    pub fn create_shelf(&mut self, name: &str) -> Result<Shelf> {
        let name = clean_name(name)?;
        if self.shelf_named(&name)?.is_some() {
            return Err(Error::InvalidBook(format!("there's already a shelf called \u{201c}{name}\u{201d}")));
        }
        self.conn.execute("INSERT INTO shelves (name, created_at) VALUES (?, ?)", params![name, now()])?;
        Ok(Shelf { id: self.conn.last_insert_rowid(), name, count: 0 })
    }

    pub fn rename_shelf(&mut self, id: ShelfId, name: &str) -> Result<()> {
        let name = clean_name(name)?;
        if self.shelf_named(&name)?.is_some_and(|other| other != id) {
            return Err(Error::InvalidBook(format!("there's already a shelf called \u{201c}{name}\u{201d}")));
        }
        let n = self.conn.execute("UPDATE shelves SET name = ? WHERE id = ?", params![name, id])?;
        if n == 0 {
            return Err(Error::InvalidBook("that shelf no longer exists".into()));
        }
        Ok(())
    }

    /// Delete a shelf; its books stay in the library.
    pub fn delete_shelf(&mut self, id: ShelfId) -> Result<()> {
        self.conn.execute("DELETE FROM shelves WHERE id = ?", [id])?;
        Ok(())
    }

    /// Put books on a shelf (ones already on it stay where they are). How many were added.
    pub fn add_to_shelf(&mut self, id: ShelfId, books: &[BookId]) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let exists: bool = tx.query_row("SELECT count(*) FROM shelves WHERE id = ?", [id], |r| r.get(0))?;
        if !exists {
            return Err(Error::InvalidBook("that shelf no longer exists".into()));
        }
        let at = now();
        let mut added = 0;
        {
            let mut stmt = tx.prepare(
                "INSERT OR IGNORE INTO book_shelves (shelf_id, book_id, added_at)
                 SELECT ?1, id, ?3 FROM books WHERE id = ?2",
            )?;
            for &book in books {
                added += stmt.execute(params![id, book, at])?;
            }
        }
        tx.commit()?;
        Ok(added)
    }

    /// Take books off a shelf. How many were on it.
    pub fn remove_from_shelf(&mut self, id: ShelfId, books: &[BookId]) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let mut removed = 0;
        {
            let mut stmt = tx.prepare("DELETE FROM book_shelves WHERE shelf_id = ? AND book_id = ?")?;
            for &book in books {
                removed += stmt.execute(params![id, book])?;
            }
        }
        tx.commit()?;
        Ok(removed)
    }

    /// A shelf per series, or per author, that has at least `min_books`
    /// books, named after it and holding those books. A shelf already called
    /// that gets the books instead of a new one.
    pub fn make_shelves(&mut self, source: ShelfSource, min_books: usize) -> Result<ShelvesMade> {
        // Grouped case-insensitively, keeping the first spelling seen.
        let mut groups: std::collections::BTreeMap<String, (String, Vec<BookId>)> = Default::default();
        for b in &self.load_catalog()?.books {
            let names: Vec<&str> = match source {
                ShelfSource::Series => {
                    b.series.iter().map(String::as_str).chain(b.more_series.iter().map(|s| s.name.as_str())).collect()
                }
                ShelfSource::Authors => b.authors.iter().map(String::as_str).collect(),
            };
            for name in names {
                let name: String = name.trim().chars().take(100).collect();
                if name.is_empty() {
                    continue;
                }
                groups.entry(name.to_lowercase()).or_insert_with(|| (name, Vec::new())).1.push(b.id);
            }
        }
        let mut made = ShelvesMade { shelves: 0, books: 0 };
        for (name, books) in groups.into_values() {
            if books.len() < min_books.max(1) {
                continue;
            }
            let id = match self.shelf_named(&name)? {
                Some(id) => id,
                None => {
                    made.shelves += 1;
                    self.create_shelf(&name)?.id
                }
            };
            made.books += self.add_to_shelf(id, &books)?;
        }
        Ok(made)
    }

    /// Each shelf's books, by name, in the order they were added (for writing to devices).
    pub fn shelf_books(&self) -> Result<Vec<(String, Vec<BookId>)>> {
        let mut out: Vec<(String, Vec<BookId>)> = Vec::new();
        let mut stmt = self.conn.prepare(
            "SELECT s.name, bs.book_id FROM shelves s JOIN book_shelves bs ON bs.shelf_id = s.id
             ORDER BY s.name COLLATE NOCASE, bs.rowid",
        )?;
        let mut rows = stmt.query([])?;
        while let Some(r) = rows.next()? {
            let (name, book): (String, BookId) = (r.get(0)?, r.get(1)?);
            match out.last_mut() {
                Some((last, books)) if *last == name => books.push(book),
                _ => out.push((name, vec![book])),
            }
        }
        Ok(out)
    }

    fn shelf_named(&self, name: &str) -> Result<Option<ShelfId>> {
        Ok(self.conn.query_row("SELECT id FROM shelves WHERE name = ?", [name], |r| r.get(0)).optional()?)
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
        let ids = ["Red Sister - Mark Lawrence.epub", "Grey Sister - Mark Lawrence.epub", "Dune - Frank Herbert.epub"]
            .iter()
            .map(|name| {
                let src = tmp.path().join(name);
                fs::write(&src, name).unwrap();
                add_files(&mut lib, &[src], |_| {}).unwrap().added[0]
            })
            .collect();
        (tmp, lib, ids)
    }

    #[test]
    fn shelves_hold_books() {
        let (_tmp, mut lib, ids) = library();
        let tbr = lib.create_shelf("  To Read ").unwrap();
        assert_eq!(tbr.name, "To Read");
        assert!(lib.create_shelf("to read").is_err(), "names are unique, ignoring case");
        assert!(lib.create_shelf("   ").is_err());

        assert_eq!(lib.add_to_shelf(tbr.id, &[ids[1], ids[0], 999]).unwrap(), 2, "unknown books are skipped");
        assert_eq!(lib.add_to_shelf(tbr.id, &[ids[0]]).unwrap(), 0, "already there");
        let fav = lib.create_shelf("Favourites").unwrap();
        lib.add_to_shelf(fav.id, &[ids[2]]).unwrap();

        let shelves = lib.shelves().unwrap();
        assert_eq!(
            shelves.iter().map(|s| (s.name.as_str(), s.count)).collect::<Vec<_>>(),
            [("Favourites", 1), ("To Read", 2)]
        );
        assert_eq!(
            lib.shelf_books().unwrap(),
            [("Favourites".to_owned(), vec![ids[2]]), ("To Read".to_owned(), vec![ids[1], ids[0]])],
            "in the order added"
        );
        // The catalog knows each book's shelves, so search can filter by them.
        let cat = lib.load_catalog().unwrap();
        assert_eq!(cat.get(ids[0]).unwrap().shelves, ["To Read"]);

        assert_eq!(lib.remove_from_shelf(tbr.id, &[ids[0], ids[2]]).unwrap(), 1);
        lib.rename_shelf(tbr.id, "Next Up").unwrap();
        assert!(lib.rename_shelf(tbr.id, "favourites").is_err(), "taken");
        lib.rename_shelf(tbr.id, "next up").unwrap();

        // Deleting a book takes it off its shelves; deleting a shelf keeps its books.
        lib.delete_books(&[ids[2]], |p| fs::remove_dir_all(p)).unwrap();
        lib.delete_shelf(tbr.id).unwrap();
        assert_eq!(
            lib.shelves().unwrap().iter().map(|s| (s.name.as_str(), s.count)).collect::<Vec<_>>(),
            [("Favourites", 0)]
        );
        assert!(lib.book(ids[1]).is_ok());
    }
}
