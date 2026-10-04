//! A Magnitide library: a folder holding `magnitide.db` and the book files.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, NaiveDate, SecondsFormat, Utc};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::Serialize;

use crate::error::{Error, Result};
use crate::lock::LibraryLock;
use crate::{db, names, paths};

pub const DB_FILE: &str = "magnitide.db";

pub type BookId = i64;
pub type UserId = i64;

/// The single user of a desktop library. The server adds real accounts.
pub const LOCAL_USER: UserId = 1;

pub struct Library {
    pub(crate) root: PathBuf,
    pub(crate) conn: Connection,
    // Held while open: one writer per library (see lock.rs). Declared last so
    // the database closes before the lock is released.
    _lock: LibraryLock,
}

#[derive(Debug, Clone, Serialize)]
pub struct LibraryInfo {
    pub uuid: String,
    pub name: String,
    pub created_at: String,
    pub schema_version: u32,
    pub book_count: u64,
}

/// Metadata for a book being added. Only `title` is required.
#[derive(Debug, Clone, Default)]
pub struct NewBook {
    pub title: String,
    /// Defaults to [`names::title_sort`] of the title.
    pub title_sort: Option<String>,
    /// In display order; empty means "Unknown".
    pub authors: Vec<String>,
    /// Sort names for authors new to the library ("Le Guin, Ursula K."), by
    /// display name. Others get [`names::author_sort`]; existing authors keep theirs.
    pub author_sorts: BTreeMap<String, String>,
    /// Name and index.
    pub series: Option<(String, f64)>,
    pub publisher: Option<String>,
    pub published: Option<NaiveDate>,
    /// Must already be sanitized.
    pub description_html: Option<String>,
    pub tags: Vec<String>,
    /// ISO 639-2 codes, in order.
    pub languages: Vec<String>,
    pub identifiers: BTreeMap<String, String>,
    /// 1–10 half stars, recorded for [`LOCAL_USER`].
    pub rating: Option<u8>,
    /// Keep an existing identity (e.g. from calibre); otherwise a new one.
    pub uuid: Option<String>,
    /// Defaults to now.
    pub added_at: Option<DateTime<Utc>>,
}

/// A book as stored, with its personal state for [`LOCAL_USER`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Book {
    pub id: BookId,
    pub uuid: String,
    pub title: String,
    pub title_sort: String,
    pub authors: Vec<String>,
    pub series: Option<(String, f64)>,
    pub publisher: Option<String>,
    pub published: Option<NaiveDate>,
    pub description_html: Option<String>,
    pub tags: Vec<String>,
    pub languages: Vec<String>,
    pub identifiers: BTreeMap<String, String>,
    pub rating: Option<u8>,
    /// Folder relative to the library root, '/'-separated.
    pub path: String,
    pub cover_file: Option<String>,
    pub added_at: String,
    pub modified_at: String,
}

pub(crate) fn now() -> String {
    timestamp(Utc::now())
}

pub(crate) fn timestamp(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Secs, true)
}

impl Library {
    /// True if `root` holds a Magnitide library.
    pub fn exists_at(root: &Path) -> bool {
        root.join(DB_FILE).is_file()
    }

    /// Create a new, empty library in `root` (created if missing).
    pub fn create(root: impl AsRef<Path>, name: &str) -> Result<Self> {
        let root = root.as_ref();
        if Self::exists_at(root) {
            return Err(Error::AlreadyALibrary(root.to_path_buf()));
        }
        fs::create_dir_all(root)?;
        let lock = LibraryLock::acquire(root)?;
        let mut conn = db::open(&root.join(DB_FILE), true)?;
        db::migrate(&mut conn)?;
        let tx = conn.transaction()?;
        let created = now();
        for (key, value) in
            [("uuid", uuid::Uuid::new_v4().to_string()), ("name", name.to_owned()), ("created_at", created.clone())]
        {
            tx.prepare_cached("INSERT INTO library_info (key, value) VALUES (?, ?)")?.execute(params![key, value])?;
        }
        tx.prepare_cached("INSERT INTO users (id, name, created_at) VALUES (?, 'local', ?)")?
            .execute(params![LOCAL_USER, created])?;
        tx.commit()?;
        Ok(Self { root: root.to_path_buf(), conn, _lock: lock })
    }

    /// Open an existing library, upgrading its schema if it was made by an
    /// older Magnitide. Refuses libraries from a newer Magnitide.
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref();
        if !Self::exists_at(root) {
            return Err(Error::NotALibrary(root.to_path_buf()));
        }
        // Lock before migrating, so an upgrade never races another writer.
        let lock = LibraryLock::acquire(root)?;
        let mut conn = db::open(&root.join(DB_FILE), false)?;
        db::migrate(&mut conn)?;
        Ok(Self { root: root.to_path_buf(), conn, _lock: lock })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn info(&self) -> Result<LibraryInfo> {
        let get = |key: &str| -> Result<String> {
            Ok(self
                .conn
                .prepare_cached("SELECT value FROM library_info WHERE key = ?")?
                .query_row([key], |r| r.get(0))?)
        };
        Ok(LibraryInfo {
            uuid: get("uuid")?,
            name: get("name")?,
            created_at: get("created_at")?,
            schema_version: db::schema_version(&self.conn)?,
            book_count: self.conn.prepare_cached("SELECT count(*) FROM books")?.query_row([], |r| r.get::<_, i64>(0))?
                as u64,
        })
    }

    /// Rename the library (shown in the app; the folder keeps its name).
    pub fn set_name(&mut self, name: &str) -> Result<()> {
        let name = name.trim();
        if name.is_empty() {
            return Err(Error::InvalidName("a library needs a name".into()));
        }
        self.conn.execute("UPDATE library_info SET value = ? WHERE key = 'name'", [name])?;
        Ok(())
    }

    /// Record a book's metadata and return its id. Assigns its folder
    /// (`Author/Title (id)`) but creates nothing on disk: adding files is the
    /// import pipeline's job, so it can clean up if copying fails.
    pub fn add_book(&mut self, book: &NewBook) -> Result<BookId> {
        let tx = self.conn.transaction()?;
        let (id, _) = Self::insert_book(&tx, book)?;
        tx.commit()?;
        Ok(id)
    }

    /// Insert a book's metadata within `tx`, returning its id and folder.
    /// Lets callers (import) commit the book together with its files.
    pub(crate) fn insert_book(tx: &Transaction<'_>, book: &NewBook) -> Result<(BookId, String)> {
        let title = book.title.trim();
        if title.is_empty() {
            return Err(Error::InvalidBook("a book needs a title".into()));
        }
        if let Some(r) = book.rating
            && !(1..=10).contains(&r)
        {
            return Err(Error::InvalidBook(format!("rating {r} is outside 1–10")));
        }
        let authors: Vec<&str> =
            match book.authors.iter().map(|a| a.trim()).filter(|a| !a.is_empty()).collect::<Vec<_>>() {
                a if a.is_empty() => vec!["Unknown"],
                a => a,
            };

        let now = now();
        let added = book.added_at.map(timestamp).unwrap_or_else(|| now.clone());
        let uuid = book.uuid.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let series_id = book.series.as_ref().map(|(name, _)| series_id(tx, name)).transpose()?;
        let publisher_id = book
            .publisher
            .as_deref()
            .map(|p| p.trim())
            .filter(|p| !p.is_empty())
            .map(|p| publisher_id(tx, p))
            .transpose()?;

        // The folder name needs the id, so insert with a unique placeholder first.
        tx.prepare_cached(
            "INSERT INTO books (uuid, title, title_sort, path, series_id, series_index, publisher_id, published,
                                description_html, added_at, modified_at)
             VALUES (?1, ?2, ?3, ?1, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        )?
        .execute(params![
            uuid,
            title,
            book.title_sort.clone().unwrap_or_else(|| names::title_sort(title)),
            series_id,
            book.series.as_ref().map(|(_, i)| *i),
            publisher_id,
            book.published.map(|d| d.to_string()),
            book.description_html,
            added,
            now,
        ])?;
        let id = tx.last_insert_rowid();
        let path = paths::book_dir(authors[0], title, id);
        tx.prepare_cached("UPDATE books SET path = ? WHERE id = ?")?.execute(params![path, id])?;

        for (pos, name) in (0_i64..).zip(&authors) {
            let author = author_id(tx, name, book.author_sorts.get(*name).map(String::as_str))?;
            tx.prepare_cached("INSERT OR IGNORE INTO book_authors (book_id, author_id, position) VALUES (?, ?, ?)")?
                .execute(params![id, author, pos])?;
        }
        for tag in book.tags.iter().map(|t| t.trim()).filter(|t| !t.is_empty()) {
            let tag_id = tag_id(tx, tag)?;
            tx.prepare_cached("INSERT OR IGNORE INTO book_tags (book_id, tag_id) VALUES (?, ?)")?
                .execute(params![id, tag_id])?;
        }
        for (pos, lang) in (0_i64..).zip(&book.languages) {
            tx.prepare_cached("INSERT OR IGNORE INTO book_languages (book_id, lang, position) VALUES (?, ?, ?)")?
                .execute(params![id, lang, pos])?;
        }
        for (scheme, value) in &book.identifiers {
            tx.prepare_cached("INSERT OR REPLACE INTO identifiers (book_id, scheme, value) VALUES (?, ?, ?)")?
                .execute(params![id, scheme, value])?;
        }
        tx.prepare_cached("INSERT INTO user_books (user_id, book_id, rating) VALUES (?, ?, ?)")?.execute(params![
            LOCAL_USER,
            id,
            book.rating
        ])?;
        Ok((id, path))
    }

    pub fn book(&self, id: BookId) -> Result<Book> {
        let c = &self.conn;
        let mut book = c
            .query_row(
                "SELECT b.uuid, b.title, b.title_sort, s.name, b.series_index, p.name, b.published,
                        b.description_html, b.path, b.cover_file, b.added_at, b.modified_at, ub.rating
                 FROM books b
                 LEFT JOIN series s ON s.id = b.series_id
                 LEFT JOIN publishers p ON p.id = b.publisher_id
                 LEFT JOIN user_books ub ON ub.book_id = b.id AND ub.user_id = ?2
                 WHERE b.id = ?1",
                params![id, LOCAL_USER],
                |r| {
                    let series: Option<String> = r.get(3)?;
                    Ok(Book {
                        id,
                        uuid: r.get(0)?,
                        title: r.get(1)?,
                        title_sort: r.get(2)?,
                        series: series.map(|s| (s, r.get::<_, Option<f64>>(4).ok().flatten().unwrap_or(1.0))),
                        publisher: r.get(5)?,
                        published: r.get::<_, Option<String>>(6)?.and_then(|d| d.parse().ok()),
                        description_html: r.get(7)?,
                        path: r.get(8)?,
                        cover_file: r.get(9)?,
                        added_at: r.get(10)?,
                        modified_at: r.get(11)?,
                        rating: r.get(12)?,
                        authors: Vec::new(),
                        tags: Vec::new(),
                        languages: Vec::new(),
                        identifiers: BTreeMap::new(),
                    })
                },
            )
            .optional()?
            .ok_or(Error::NoSuchBook(id))?;

        let strings = |sql: &str| -> Result<Vec<String>> {
            let mut stmt = c.prepare(sql)?;
            let rows = stmt.query_map([id], |r| r.get(0))?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        };
        book.authors = strings(
            "SELECT a.name FROM book_authors ba JOIN authors a ON a.id = ba.author_id WHERE ba.book_id = ? ORDER BY ba.position",
        )?;
        book.tags = strings(
            "SELECT t.name FROM book_tags bt JOIN tags t ON t.id = bt.tag_id WHERE bt.book_id = ? ORDER BY t.name",
        )?;
        book.languages = strings("SELECT lang FROM book_languages WHERE book_id = ? ORDER BY position")?;
        let mut stmt = c.prepare("SELECT scheme, value FROM identifiers WHERE book_id = ?")?;
        book.identifiers = stmt.query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
        Ok(book)
    }
}

/// Look up a name in a name table, inserting it if new. Matching is
/// case-insensitive (the columns are COLLATE NOCASE); the first spelling wins.
pub(crate) fn get_or_insert(
    tx: &Transaction<'_>,
    select: &str,
    insert: &str,
    params: &[&dyn rusqlite::ToSql],
) -> Result<i64> {
    if let Some(id) = tx.prepare_cached(select)?.query_row([params[0]], |r| r.get(0)).optional()? {
        return Ok(id);
    }
    tx.prepare_cached(insert)?.execute(params)?;
    Ok(tx.last_insert_rowid())
}

pub(crate) fn author_id(tx: &Transaction<'_>, name: &str, sort: Option<&str>) -> Result<i64> {
    let sort = sort.map(str::to_owned).unwrap_or_else(|| names::author_sort(name));
    get_or_insert(
        tx,
        "SELECT id FROM authors WHERE name = ?",
        "INSERT INTO authors (name, sort_name) VALUES (?, ?)",
        &[&name, &sort],
    )
}

pub(crate) fn series_id(tx: &Transaction<'_>, name: &str) -> Result<i64> {
    let sort = names::title_sort(name);
    get_or_insert(
        tx,
        "SELECT id FROM series WHERE name = ?",
        "INSERT INTO series (name, sort_name) VALUES (?, ?)",
        &[&name, &sort],
    )
}

pub(crate) fn publisher_id(tx: &Transaction<'_>, name: &str) -> Result<i64> {
    get_or_insert(tx, "SELECT id FROM publishers WHERE name = ?", "INSERT INTO publishers (name) VALUES (?)", &[&name])
}

pub(crate) fn tag_id(tx: &Transaction<'_>, name: &str) -> Result<i64> {
    get_or_insert(tx, "SELECT id FROM tags WHERE name = ?", "INSERT INTO tags (name) VALUES (?)", &[&name])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dispossessed() -> NewBook {
        NewBook {
            title: "The Dispossessed".into(),
            authors: vec!["Ursula K. Le Guin".into()],
            series: Some(("Hainish Cycle".into(), 5.0)),
            publisher: Some("Harper & Row".into()),
            published: NaiveDate::from_ymd_opt(1974, 5, 1),
            tags: vec!["Science Fiction".into(), "Classics".into()],
            languages: vec!["eng".into()],
            identifiers: BTreeMap::from([("isbn".into(), "9780061054884".into())]),
            rating: Some(10),
            ..Default::default()
        }
    }

    #[test]
    fn create_then_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let info = Library::create(dir.path(), "Test").unwrap().info().unwrap();
        assert_eq!((info.name.as_str(), info.book_count, info.schema_version), ("Test", 0, db::SCHEMA_VERSION));
        assert!(Library::exists_at(dir.path()));

        let reopened = Library::open(dir.path()).unwrap().info().unwrap();
        assert_eq!(reopened.uuid, info.uuid);
        assert!(matches!(Library::create(dir.path(), "Again"), Err(Error::AlreadyALibrary(_))));
    }

    #[test]
    fn one_writer_at_a_time() {
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::create(dir.path(), "Test").unwrap();
        let err = Library::open(dir.path()).err().unwrap();
        assert!(matches!(err, Error::LibraryInUse(_)), "{err}");
        assert!(err.to_string().contains("already open"), "{err}");
        drop(lib);
        Library::open(dir.path()).unwrap();
    }

    #[test]
    fn renames() {
        let dir = tempfile::tempdir().unwrap();
        let mut lib = Library::create(dir.path(), "Old").unwrap();
        lib.set_name("  New Name ").unwrap();
        assert!(matches!(lib.set_name("   "), Err(Error::InvalidName(_))));
        drop(lib);
        assert_eq!(Library::open(dir.path()).unwrap().info().unwrap().name, "New Name");
    }

    #[test]
    fn open_refuses_non_libraries_and_newer_schemas() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(Library::open(dir.path()), Err(Error::NotALibrary(_))));

        drop(Library::create(dir.path(), "Test").unwrap());
        let conn = Connection::open(dir.path().join(DB_FILE)).unwrap();
        conn.pragma_update(None, "user_version", db::SCHEMA_VERSION + 1).unwrap();
        drop(conn);
        assert!(matches!(Library::open(dir.path()), Err(Error::SchemaTooNew { .. })));
    }

    #[test]
    fn add_and_read_back_a_book() {
        let dir = tempfile::tempdir().unwrap();
        let mut lib = Library::create(dir.path(), "Test").unwrap();
        let id = lib.add_book(&dispossessed()).unwrap();
        let b = lib.book(id).unwrap();

        assert_eq!(b.title, "The Dispossessed");
        assert_eq!(b.title_sort, "Dispossessed, The");
        assert_eq!(b.authors, ["Ursula K. Le Guin"]);
        assert_eq!(b.series, Some(("Hainish Cycle".into(), 5.0)));
        assert_eq!(b.publisher.as_deref(), Some("Harper & Row"));
        assert_eq!(b.published, NaiveDate::from_ymd_opt(1974, 5, 1));
        assert_eq!(b.tags, ["Classics", "Science Fiction"]);
        assert_eq!(b.languages, ["eng"]);
        assert_eq!(b.identifiers.get("isbn").map(String::as_str), Some("9780061054884"));
        assert_eq!(b.rating, Some(10));
        assert_eq!(b.path, format!("Ursula K. Le Guin/The Dispossessed ({id})"));
        assert_eq!(b.uuid.len(), 36);
        assert_eq!(lib.info().unwrap().book_count, 1);
    }

    #[test]
    fn shares_names_case_insensitively_and_keeps_author_order() {
        let dir = tempfile::tempdir().unwrap();
        let mut lib = Library::create(dir.path(), "Test").unwrap();
        let a = lib
            .add_book(&NewBook {
                title: "A".into(),
                authors: vec!["Neil Gaiman".into(), "Terry Pratchett".into()],
                tags: vec!["Fantasy".into()],
                ..Default::default()
            })
            .unwrap();
        let b = lib
            .add_book(&NewBook {
                title: "B".into(),
                authors: vec!["terry pratchett".into()],
                tags: vec!["fantasy".into()],
                ..Default::default()
            })
            .unwrap();

        assert_eq!(lib.book(a).unwrap().authors, ["Neil Gaiman", "Terry Pratchett"]);
        // Same author and tag rows, first spelling kept.
        assert_eq!(lib.book(b).unwrap().authors, ["Terry Pratchett"]);
        assert_eq!(lib.book(b).unwrap().tags, ["Fantasy"]);
        let authors: i64 = lib.conn.query_row("SELECT count(*) FROM authors", [], |r| r.get(0)).unwrap();
        assert_eq!(authors, 2);
    }

    #[test]
    fn validates_input_and_defaults_missing_authors() {
        let dir = tempfile::tempdir().unwrap();
        let mut lib = Library::create(dir.path(), "Test").unwrap();
        assert!(matches!(lib.add_book(&NewBook::default()), Err(Error::InvalidBook(_))));
        assert!(matches!(
            lib.add_book(&NewBook { title: "X".into(), rating: Some(11), ..Default::default() }),
            Err(Error::InvalidBook(_))
        ));

        let id = lib.add_book(&NewBook { title: "Anonymous Work".into(), ..Default::default() }).unwrap();
        let b = lib.book(id).unwrap();
        assert_eq!(b.authors, ["Unknown"]);
        assert_eq!(b.path, format!("Unknown/Anonymous Work ({id})"));
        assert!(matches!(lib.book(999), Err(Error::NoSuchBook(999))));
    }

    #[test]
    fn keeps_given_uuid() {
        let dir = tempfile::tempdir().unwrap();
        let mut lib = Library::create(dir.path(), "Test").unwrap();
        let uuid = "c67a09ae-ea43-4d7f-b754-d38847319512".to_string();
        let id = lib.add_book(&NewBook { title: "T".into(), uuid: Some(uuid.clone()), ..Default::default() }).unwrap();
        assert_eq!(lib.book(id).unwrap().uuid, uuid);
    }
}
