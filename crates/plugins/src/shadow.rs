//! The shadow library: a calibre-format copy of the open library that GUI
//! plugins work on, with calibre's own database code.
//!
//! It is written from the library's catalog ([`calibre_compat::write`]) with
//! the same book ids, and its book folders hold links to the real files, not
//! copies. The host turns a link into a private copy (a clone where the disk
//! can) before calibre hands out its path or writes to it, so a plugin can
//! never change the user's files directly. Afterwards [`Shadow::changes`]
//! compares the shadow with the catalog it was written from, and Magnitide
//! applies what changed through its own edit path.
//!
//! Plugin settings kept in the library (calibre's `preferences` and
//! `books_plugin_data` tables) survive rebuilds. The shadow is never one of
//! the user's calibre libraries.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use calibre_compat::write::{WriteOptions, write_library};
use magnitide_catalog::diff::{AddedBook, BookChanges, ChangedBook, Changes, CoverChange, FileChange, book_changes};
use magnitide_catalog::{Book, BookId, Catalog, Datatype};

/// Preferences calibre derives from the library itself on open.
const DERIVED_PREFERENCES: &[&str] = &["field_metadata"];

pub struct Shadow {
    dir: PathBuf,
}

impl Shadow {
    /// A shadow kept in `dir` (one per library, in the app's cache).
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// The calibre library folder (holding `metadata.db`).
    pub fn library(&self) -> PathBuf {
        self.dir.join("library")
    }

    /// (Re)write the shadow from `catalog`, whose files live in `library_root`.
    /// Nothing may have the shadow open meanwhile (the host closes it first).
    pub fn build(&self, catalog: &Catalog, library_root: &Path, opts: &WriteOptions) -> Result<(), String> {
        let new = self.dir.join("library.new");
        let old = self.dir.join("library.old");
        for stale in [&new, &old] {
            remove_dir(stale)?;
        }
        write_library(&new, catalog, opts).map_err(|e| format!("writing the plugin library: {e:#}"))?;
        let current = self.library();
        if current.join("metadata.db").is_file() {
            carry_over(&current.join("metadata.db"), &new.join("metadata.db"))?;
        }
        for book in &catalog.books {
            link_files(book, library_root, &new)?;
        }
        if current.exists() {
            fs::rename(&current, &old).map_err(|e| e.to_string())?;
        }
        fs::rename(&new, &current).map_err(|e| e.to_string())?;
        remove_dir(&old)
    }

    /// What changed in the shadow since it was built from `before`.
    pub fn changes(&self, before: &Catalog, library_root: &Path) -> Result<Changes, String> {
        self.diff(before, &self.read()?, library_root)
    }

    /// The shadow as it is now.
    pub fn read(&self) -> Result<Catalog, String> {
        calibre_compat::Library::open(self.library())
            .and_then(|l| l.load())
            .map_err(|e| format!("reading the plugin library: {e:#}"))
    }

    /// How the shadow (`after`, from [`Shadow::read`]) differs from `before`,
    /// whose files are in `library_root`.
    pub fn diff(&self, before: &Catalog, after: &Catalog, library_root: &Path) -> Result<Changes, String> {
        let root = self.library();
        let mut out = Changes::default();
        let known: HashSet<String> = before.custom_columns.iter().map(|c| c.label.to_lowercase()).collect();
        out.new_columns = after
            .custom_columns
            .iter()
            .filter(|c| !known.contains(&c.label.to_lowercase()))
            .filter(|c| !matches!(c.datatype, Datatype::Composite | Datatype::Unknown(_)))
            .cloned()
            .collect();
        let present: HashSet<BookId> = after.books.iter().map(|b| b.id).collect();
        out.removed = before.books.iter().map(|b| b.id).filter(|id| !present.contains(id)).collect();

        for a in &after.books {
            let Some(b) = before.get(a.id) else {
                let files = a
                    .formats
                    .iter()
                    .map(|f| (f.format.clone(), a.format_path(&root, f)))
                    .filter(|(_, p)| p.is_file())
                    .collect();
                let cover = a.cover_path(&root).filter(|p| p.is_file());
                out.added.push(AddedBook { book: a.clone(), files, cover });
                continue;
            };
            // calibre stamps every metadata change; files are checked regardless,
            // since a plugin can rewrite a file without touching the database.
            let fields = if a.last_modified != b.last_modified { book_changes(b, a) } else { BookChanges::default() };
            let files = file_changes(b, a, library_root, &root)?;
            let cover = cover_change(b, a, library_root, &root)?;
            if !fields.is_empty() || !files.is_empty() || cover.is_some() {
                out.books.push(ChangedBook { id: a.id, fields, files, cover });
            }
        }
        Ok(out)
    }
}

/// What to measure a plugin's further changes against once the library has
/// caught up with the shadow mid-run (Magnitide applied its changes so far,
/// or the user edited or deleted books it asked about): the library's books
/// (`library`) under their ids in the shadow (`ids`: shadow id → library id
/// for the books added since the shadow was built; the rest share ids), with
/// the shadow's modification times, so only books the plugin changes after
/// this are compared field by field. Books not in the shadow are left out.
pub fn baseline(library: &Catalog, shadow: &Catalog, ids: &HashMap<BookId, BookId>) -> Catalog {
    let to_shadow: HashMap<BookId, BookId> = ids.iter().map(|(&theirs, &ours)| (ours, theirs)).collect();
    let books = library
        .books
        .iter()
        .filter_map(|b| {
            let id = to_shadow.get(&b.id).copied().unwrap_or(b.id);
            let there = shadow.get(id)?;
            // A shadow book with the same id that isn't this one (a book added
            // there whose id is also a book added here) isn't a match.
            if !to_shadow.contains_key(&b.id) && ids.contains_key(&id) {
                return None;
            }
            Some(Book { id, last_modified: there.last_modified, ..b.clone() })
        })
        .collect();
    Catalog::new(shadow.library_uuid.clone(), library.custom_columns.clone(), books, vec![])
}

/// Keep plugin settings stored in the library across a rebuild.
fn carry_over(from: &Path, to: &Path) -> Result<(), String> {
    let run = || -> rusqlite::Result<()> {
        let conn = rusqlite::Connection::open(to)?;
        conn.execute("ATTACH DATABASE ? AS old", [from.to_string_lossy()])?;
        let derived = DERIVED_PREFERENCES.iter().map(|k| format!("'{k}'")).collect::<Vec<_>>().join(", ");
        conn.execute_batch(&format!(
            "INSERT OR REPLACE INTO main.preferences (key, val)
                 SELECT key, val FROM old.preferences WHERE key NOT IN ({derived});
             INSERT OR REPLACE INTO main.books_plugin_data (book, name, val)
                 SELECT book, name, val FROM old.books_plugin_data WHERE book IN (SELECT id FROM main.books);"
        ))?;
        conn.execute("DETACH DATABASE old", [])?;
        Ok(())
    };
    run().map_err(|e| format!("keeping plugin settings: {e}"))
}

/// The book's folder in the shadow, with links to its files and cover.
fn link_files(book: &Book, library_root: &Path, shadow: &Path) -> Result<(), String> {
    let dir = book.dir(shadow);
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut pairs: Vec<(PathBuf, PathBuf)> =
        book.formats.iter().map(|f| (book.format_path(library_root, f), book.format_path(shadow, f))).collect();
    if let (Some(real), Some(link)) = (book.cover_path(library_root), book.cover_path(shadow)) {
        pairs.push((real, link));
    }
    for (real, link) in pairs {
        // A missing file stays missing, as it would in calibre.
        if real.is_file() {
            symlink(&real, &link).map_err(|e| format!("{}: {e}", link.display()))?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    // Symlinks need Developer Mode or admin rights on Windows; copy otherwise.
    std::os::windows::fs::symlink_file(target, link).or_else(|_| fs::copy(target, link).map(|_| ()))
}

fn file_changes(before: &Book, after: &Book, library_root: &Path, shadow: &Path) -> Result<Vec<FileChange>, String> {
    let mut out = Vec::new();
    for f in &after.formats {
        let path = after.format_path(shadow, f);
        if !is_own_file(&path) {
            continue; // still the link to the library's file, or missing
        }
        match before.formats.iter().find(|g| g.format == f.format) {
            Some(g) if same_contents(&before.format_path(library_root, g), &path)? => {}
            Some(_) => out.push(FileChange::Replaced { format: f.format.clone(), path }),
            None => out.push(FileChange::Added { format: f.format.clone(), path }),
        }
    }
    for g in &before.formats {
        if !after.formats.iter().any(|f| f.format == g.format) {
            out.push(FileChange::Removed { format: g.format.clone() });
        }
    }
    Ok(out)
}

fn cover_change(
    before: &Book,
    after: &Book,
    library_root: &Path,
    shadow: &Path,
) -> Result<Option<CoverChange>, String> {
    match (before.cover_path(library_root), after.cover_path(shadow)) {
        (Some(_), None) => Ok(Some(CoverChange::Removed)),
        (old, Some(new)) if is_own_file(&new) => match old {
            Some(old) if same_contents(&old, &new)? => Ok(None),
            _ => Ok(Some(CoverChange::Set(new))),
        },
        _ => Ok(None),
    }
}

/// A file of the shadow's own: written or copied there, not a link to the library.
fn is_own_file(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.is_file())
}

fn same_contents(a: &Path, b: &Path) -> Result<bool, String> {
    let (Ok(ma), Ok(mb)) = (fs::metadata(a), fs::metadata(b)) else { return Ok(false) };
    if ma.len() != mb.len() {
        return Ok(false);
    }
    let open = |p: &Path| File::open(p).map(BufReader::new).map_err(|e| format!("{}: {e}", p.display()));
    let (mut ra, mut rb) = (open(a)?, open(b)?);
    let (mut ba, mut bb) = (vec![0u8; 64 * 1024], vec![0u8; 64 * 1024]);
    loop {
        let n = read_full(&mut ra, &mut ba).map_err(|e| e.to_string())?;
        let m = read_full(&mut rb, &mut bb).map_err(|e| e.to_string())?;
        if n != m || ba[..n] != bb[..m] {
            return Ok(false);
        }
        if n == 0 {
            return Ok(true);
        }
    }
}

fn read_full(r: &mut impl Read, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match r.read(&mut buf[n..])? {
            0 => break,
            k => n += k,
        }
    }
    Ok(n)
}

fn remove_dir(path: &Path) -> Result<(), String> {
    // Doesn't follow links: the library's files behind them stay.
    match fs::remove_dir_all(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(format!("{}: {e}", path.display())),
        _ => Ok(()),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::collections::BTreeMap;

    use magnitide_catalog::{CustomColumn, CustomValue, Format};
    use rusqlite::Connection;

    use super::*;

    fn book(id: BookId, title: &str) -> Book {
        Book {
            id,
            title: title.into(),
            sort: None,
            authors: vec!["Author".into()],
            author_sort: None,
            series: None,
            series_index: 1.0,
            series_end: None,
            more_series: vec![],
            collects: vec![],
            collected_in: vec![],
            tags: vec![],
            shelves: vec![],
            publisher: None,
            rating: None,
            languages: vec![],
            identifiers: BTreeMap::new(),
            comments: None,
            timestamp: None,
            pubdate: None,
            last_modified: Some(chrono::Utc::now()),
            path: format!("Author/{title} ({id})"),
            uuid: None,
            has_cover: true,
            formats: vec![
                Format { format: "EPUB".into(), size: 4, name: format!("{title} - Author") },
                Format { format: "PDF".into(), size: 3, name: format!("{title} - Author") },
            ],
            custom: BTreeMap::new(),
        }
    }

    /// A library with two books, each with an EPUB, a PDF and a cover.
    fn library(root: &Path) -> Catalog {
        let books = vec![book(1, "One"), book(2, "Two")];
        for b in &books {
            fs::create_dir_all(b.dir(root)).unwrap();
            for f in &b.formats {
                fs::write(b.format_path(root, f), if f.format == "EPUB" { "epub" } else { "pdf" }).unwrap();
            }
            fs::write(b.cover_path(root).unwrap(), "jpeg").unwrap();
        }
        let cols = vec![CustomColumn {
            id: 2,
            label: "read".into(),
            name: "Read".into(),
            datatype: Datatype::Bool,
            is_multiple: false,
            normalized: false,
            editable: true,
            display: serde_json::Value::Null,
        }];
        Catalog::new(Some("lib".into()), cols, books, vec![])
    }

    /// Change the shadow as calibre would (its triggers need its SQL functions).
    fn sql(shadow: &Shadow, sql: &str) {
        let conn = Connection::open(shadow.library().join("metadata.db")).unwrap();
        calibre_compat::sqlfuncs::register(&conn).unwrap();
        conn.execute_batch(sql).unwrap();
    }

    #[test]
    fn links_files_and_finds_nothing_changed() {
        let tmp = tempfile::tempdir().unwrap();
        let (root, shadow) = (tmp.path().join("lib"), Shadow::new(tmp.path().join("shadow")));
        let catalog = library(&root);
        shadow.build(&catalog, &root, &WriteOptions::default()).unwrap();

        let b = catalog.get(1).unwrap();
        let link = b.format_path(&shadow.library(), &b.formats[0]);
        assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        assert_eq!(fs::read_link(&link).unwrap(), b.format_path(&root, &b.formats[0]));
        assert!(shadow.changes(&catalog, &root).unwrap().is_empty());
    }

    #[test]
    fn finds_metadata_file_and_cover_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let (root, shadow) = (tmp.path().join("lib"), Shadow::new(tmp.path().join("shadow")));
        let catalog = library(&root);
        shadow.build(&catalog, &root, &WriteOptions::default()).unwrap();
        let lib = shadow.library();
        let one = catalog.get(1).unwrap();

        // As calibre would: a new title, a custom value, a rewritten EPUB
        // (materialized, then changed), a materialized but unchanged PDF, a new cover.
        sql(
            &shadow,
            "UPDATE books SET title = 'Uno', last_modified = '2030-01-01 00:00:00+00:00' WHERE id = 1;
             INSERT INTO custom_column_2 (book, value) VALUES (1, 1);
             DELETE FROM data WHERE book = 2 AND format = 'PDF';",
        );
        let epub = one.format_path(&lib, &one.formats[0]);
        fs::remove_file(&epub).unwrap();
        fs::write(&epub, "epub with metadata").unwrap();
        let pdf = one.format_path(&lib, &one.formats[1]);
        fs::remove_file(&pdf).unwrap();
        fs::write(&pdf, "pdf").unwrap();
        let cover = one.cover_path(&lib).unwrap();
        fs::remove_file(&cover).unwrap();
        fs::write(&cover, "new jpeg").unwrap();

        let changes = shadow.changes(&catalog, &root).unwrap();
        assert_eq!(changes.books.len(), 2, "{changes:#?}");
        let c = &changes.books[0];
        assert_eq!(c.fields.title.as_deref(), Some("Uno"));
        assert_eq!(c.fields.custom, BTreeMap::from([("#read".into(), Some(CustomValue::Bool(true)))]));
        assert_eq!(c.files, [FileChange::Replaced { format: "EPUB".into(), path: epub }]);
        assert_eq!(c.cover, Some(CoverChange::Set(cover)));
        let c = &changes.books[1];
        assert!(c.fields.is_empty(), "files only: last_modified unchanged");
        assert_eq!(c.files, [FileChange::Removed { format: "PDF".into() }]);
        // The library's own files are untouched.
        assert_eq!(fs::read_to_string(one.format_path(&root, &one.formats[0])).unwrap(), "epub");
    }

    #[test]
    fn finds_added_and_removed_books_and_new_columns() {
        let tmp = tempfile::tempdir().unwrap();
        let (root, shadow) = (tmp.path().join("lib"), Shadow::new(tmp.path().join("shadow")));
        let catalog = library(&root);
        shadow.build(&catalog, &root, &WriteOptions::default()).unwrap();
        let lib = shadow.library();
        fs::create_dir_all(lib.join("New/Merged (3)")).unwrap();
        fs::write(lib.join("New/Merged (3)/Merged - New.epub"), "merged").unwrap();
        sql(
            &shadow,
            "DELETE FROM books WHERE id = 2;
             INSERT INTO books (id, title, path) VALUES (3, 'Merged', 'New/Merged (3)');
             INSERT INTO data (book, format, uncompressed_size, name) VALUES (3, 'EPUB', 6, 'Merged - New');
             INSERT INTO custom_columns (label, name, datatype, is_multiple, editable, display, normalized)
                 VALUES ('pages', 'Pages', 'int', 0, 1, '{}', 0);
             CREATE TABLE custom_column_3 (id INTEGER PRIMARY KEY, book INTEGER, value INT NOT NULL, UNIQUE(book));",
        );
        let changes = shadow.changes(&catalog, &root).unwrap();
        assert_eq!(changes.removed, [2]);
        assert_eq!(changes.added.len(), 1);
        assert_eq!(changes.added[0].book.title, "Merged");
        assert_eq!(changes.added[0].files, [("EPUB".into(), lib.join("New/Merged (3)/Merged - New.epub"))]);
        assert_eq!(changes.new_columns.iter().map(|c| c.label.as_str()).collect::<Vec<_>>(), ["pages"]);
    }

    #[test]
    fn a_baseline_after_catching_up_finds_only_later_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let (root, shadow) = (tmp.path().join("lib"), Shadow::new(tmp.path().join("shadow")));
        let catalog = library(&root);
        shadow.build(&catalog, &root, &WriteOptions::default()).unwrap();
        // The plugin adds book 3 and retitles book 1.
        sql(
            &shadow,
            "INSERT INTO books (id, title, path, last_modified) VALUES (3, 'Merged', 'Author/Merged (3)', '2030-01-01 00:00:00+00:00');
             UPDATE books SET title = 'Uno', last_modified = '2030-01-01 00:00:00+00:00' WHERE id = 1;",
        );
        let after = shadow.read().unwrap();
        assert_eq!(shadow.diff(&catalog, &after, &root).unwrap().added.len(), 1);

        // Magnitide applied that: book 3 became its book 7, book 1 is "Uno".
        let mut caught_up = catalog.books.clone();
        caught_up[0].title = "Uno".into();
        let mut merged = book(7, "Merged");
        merged.formats.clear();
        merged.has_cover = false;
        caught_up.push(merged);
        let library_now = Catalog::new(catalog.library_uuid.clone(), catalog.custom_columns.clone(), caught_up, vec![]);
        let ids = HashMap::from([(3, 7)]);
        let base = baseline(&library_now, &after, &ids);
        assert_eq!(base.books.iter().map(|b| b.id).collect::<Vec<_>>(), [1, 2, 3]);
        assert!(shadow.diff(&base, &after, &root).unwrap().is_empty(), "nothing applied twice");

        // A later change to the added book is a change to it, under its shadow id.
        sql(
            &shadow,
            "UPDATE books SET title = 'Merged Anthology', last_modified = '2030-02-01 00:00:00+00:00' WHERE id = 3;",
        );
        let later = shadow.diff(&base, &shadow.read().unwrap(), &root).unwrap();
        assert!(later.added.is_empty());
        assert_eq!(later.books.len(), 1);
        assert_eq!((later.books[0].id, later.books[0].fields.title.as_deref()), (3, Some("Merged Anthology")));
    }

    #[test]
    fn rebuilds_keep_plugin_settings() {
        let tmp = tempfile::tempdir().unwrap();
        let (root, shadow) = (tmp.path().join("lib"), Shadow::new(tmp.path().join("shadow")));
        let catalog = library(&root);
        shadow.build(&catalog, &root, &WriteOptions::default()).unwrap();
        sql(
            &shadow,
            "INSERT INTO preferences (key, val) VALUES ('namespaced:CountPages:settings', '{\"col\": \"#pages\"}');
             INSERT INTO preferences (key, val) VALUES ('field_metadata', '{}');
             INSERT INTO books_plugin_data (book, name, val) VALUES (1, 'plugin', '1'), (99, 'gone', '1');",
        );
        shadow.build(&catalog, &root, &WriteOptions::default()).unwrap();
        let conn = Connection::open(shadow.library().join("metadata.db")).unwrap();
        let keys: Vec<String> = conn
            .prepare("SELECT key FROM preferences")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(keys, ["namespaced:CountPages:settings"]);
        let data: i64 = conn.query_row("SELECT count(*) FROM books_plugin_data", [], |r| r.get(0)).unwrap();
        assert_eq!(data, 1, "data for books no longer in the library goes");
        assert!(!tmp.path().join("shadow/library.old").exists() && !tmp.path().join("shadow/library.new").exists());
    }
}
