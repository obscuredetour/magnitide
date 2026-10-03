//! Golden test against calibre's own test library.
//!
//! `fixtures/calibre_test_metadata.db` is `src/calibre/db/tests/metadata.db`
//! from the calibre repo. The expected values below are copied from
//! calibre's `src/calibre/db/tests/reading.py::test_read`, and the setup
//! (covers + format files) mirrors `BaseTest.create_db` in `base.py`.

use std::fs;
use std::path::Path;

use calibre_compat::{CustomValue, Library, Query, sqlfuncs};
use chrono::{DateTime, TimeZone, Utc};

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/calibre_test_metadata.db");

fn setup_library(dir: &Path) {
    let db = dir.join("metadata.db");
    fs::copy(FIXTURE, &db).unwrap();
    let conn = rusqlite::Connection::open(&db).unwrap();
    // books_update_trg calls title_sort(), so even setting has_cover fails
    // without calibre's SQL functions registered.
    assert!(conn.execute("UPDATE books SET has_cover = 1 WHERE id = 1", []).is_err());
    sqlfuncs::register(&conn).unwrap();

    for (book, fmt, body) in [(1, "FMT1", "book1fmt1"), (1, "FMT2", "book1fmt2"), (2, "FMT1", "book2fmt1")] {
        let (path, title, author): (String, String, String) = conn
            .query_row("SELECT path, title, author_sort FROM books WHERE id = ?", [book], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .unwrap();
        let name = format!("{title} - {author}");
        conn.execute(
            "INSERT INTO data(book, format, uncompressed_size, name) VALUES (?, ?, ?, ?)",
            rusqlite::params![book, fmt, body.len() as i64, name],
        )
        .unwrap();
        let folder = dir.join(&path);
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join(format!("{name}.{}", fmt.to_lowercase())), body).unwrap();
    }
    for book in [1, 2] {
        conn.execute("UPDATE books SET has_cover = 1 WHERE id = ?", [book]).unwrap();
        let path: String = conn.query_row("SELECT path FROM books WHERE id = ?", [book], |r| r.get(0)).unwrap();
        fs::write(dir.join(path).join("cover.jpg"), b"jpeg").unwrap();
    }
}

fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(y, mo, d, h, mi, s).unwrap()
}

/// calibre's test compares at whole-second precision.
fn secs(d: Option<DateTime<Utc>>) -> Option<i64> {
    d.map(|d| d.timestamp())
}

fn sorted(v: &[String]) -> Vec<&str> {
    let mut v: Vec<&str> = v.iter().map(String::as_str).collect();
    v.sort();
    v
}

fn text_list(v: Option<&CustomValue>) -> Vec<&str> {
    match v {
        Some(CustomValue::TextList(l)) => l.iter().map(String::as_str).collect(),
        None => vec![],
        other => panic!("expected a text list, got {other:?}"),
    }
}

#[test]
fn matches_calibre_reading_test() {
    let tmp = tempfile::tempdir().unwrap();
    setup_library(tmp.path());

    let lib = Library::open(tmp.path()).unwrap();
    assert_eq!(lib.schema_version, 28);
    assert_eq!(lib.application_id, calibre_compat::library::CALIBRE_APPLICATION_ID);
    let snap = lib.load().unwrap();
    assert!(snap.warnings.is_empty(), "{:?}", snap.warnings);
    assert_eq!(snap.books.len(), 3);
    assert_eq!(snap.library_uuid.as_deref(), Some("95869367-2abd-4e0d-a201-3b640b601dde"));

    // Book 3: an empty "Unknown" record.
    let b = snap.get(3).unwrap();
    assert_eq!(b.title, "Unknown");
    assert_eq!(b.sort.as_deref(), Some("Unknown"));
    assert_eq!(b.authors, ["Unknown"]);
    assert_eq!(b.author_sort.as_deref(), Some("Unknown"));
    assert_eq!((b.series.as_deref(), b.series_index), (None, 1.0));
    assert_eq!(b.rating, None);
    assert!(b.tags.is_empty() && b.formats.is_empty() && b.identifiers.is_empty() && b.languages.is_empty());
    assert_eq!(b.publisher, None);
    assert_eq!(b.comments, None);
    assert_eq!(secs(b.timestamp), secs(Some(utc(2011, 9, 7, 19, 54, 41))));
    assert_eq!(secs(b.pubdate), secs(Some(utc(2011, 9, 7, 19, 54, 41))));
    assert_eq!(secs(b.last_modified), secs(Some(utc(2011, 9, 7, 19, 54, 41))));
    assert!(b.custom.is_empty(), "{:?}", b.custom);

    // Book 2.
    let b = snap.get(2).unwrap();
    assert_eq!(b.title, "Title One");
    assert_eq!(b.sort.as_deref(), Some("One"));
    assert_eq!(b.authors, ["Author One"]);
    assert_eq!(b.author_sort.as_deref(), Some("One, Author"));
    assert_eq!((b.series.as_deref(), b.series_index), (Some("A Series One"), 1.0));
    assert_eq!(sorted(&b.tags), ["Tag One", "Tag Two"]);
    assert_eq!(b.formats.iter().map(|f| f.format.as_str()).collect::<Vec<_>>(), ["FMT1"]);
    assert_eq!(b.rating, Some(4));
    assert_eq!(b.identifiers.get("test").map(String::as_str), Some("one"));
    assert_eq!(secs(b.timestamp), secs(Some(utc(2011, 9, 5, 21, 6, 0))));
    assert_eq!(secs(b.pubdate), secs(Some(utc(2011, 9, 5, 21, 6, 0))));
    assert_eq!(b.publisher.as_deref(), Some("Publisher One"));
    assert_eq!(b.languages, ["eng"]);
    assert_eq!(b.comments.as_deref(), Some("<p>Comments One</p>"));
    assert_eq!(b.custom.get("#enum"), Some(&CustomValue::Text("One".into())));
    assert_eq!(text_list(b.custom.get("#authors")), ["Custom One", "Custom Two"]);
    assert_eq!(b.custom.get("#date"), Some(&CustomValue::Datetime(utc(2011, 9, 5, 6, 0, 0))));
    assert_eq!(b.custom.get("#rating"), Some(&CustomValue::Rating(2)));
    assert_eq!(b.custom.get("#series"), Some(&CustomValue::Series { name: "My Series One".into(), index: 1.0 }));
    let mut tags = text_list(b.custom.get("#tags"));
    tags.sort();
    assert_eq!(tags, ["My Tag One", "My Tag Two"]);
    assert_eq!(b.custom.get("#yesno"), Some(&CustomValue::Bool(true)));
    assert_eq!(b.custom.get("#comments"), Some(&CustomValue::Text("<div>My Comments One<p></p></div>".into())));
    assert_eq!(b.formats.iter().map(|f| f.size).max(), Some(9));

    // Book 1: multiple authors, order matters.
    let b = snap.get(1).unwrap();
    assert_eq!(b.title, "Title Two");
    assert_eq!(b.sort.as_deref(), Some("Title Two"));
    assert_eq!(b.authors, ["Author Two", "Author One"]);
    assert_eq!(b.author_sort.as_deref(), Some("Two, Author & One, Author"));
    assert_eq!((b.series.as_deref(), b.series_index), (Some("A Series One"), 2.0));
    assert_eq!(b.rating, Some(6));
    assert_eq!(sorted(&b.tags), ["News", "Tag One"]);
    let mut fmts: Vec<&str> = b.formats.iter().map(|f| f.format.as_str()).collect();
    fmts.sort();
    assert_eq!(fmts, ["FMT1", "FMT2"]);
    assert_eq!(b.identifiers.get("test").map(String::as_str), Some("two"));
    assert_eq!(secs(b.timestamp), secs(Some(utc(2011, 9, 6, 6, 0, 0))));
    assert_eq!(secs(b.pubdate), secs(Some(utc(2011, 8, 5, 6, 0, 0))));
    assert_eq!(b.publisher.as_deref(), Some("Publisher Two"));
    assert_eq!(b.languages, ["deu"]);
    assert_eq!(b.comments.as_deref(), Some("<p>Comments Two</p>"));
    assert_eq!(b.custom.get("#enum"), Some(&CustomValue::Text("Two".into())));
    assert_eq!(text_list(b.custom.get("#authors")), ["My Author Two"]);
    assert_eq!(b.custom.get("#date"), Some(&CustomValue::Datetime(utc(2011, 9, 1, 6, 0, 0))));
    assert_eq!(b.custom.get("#rating"), Some(&CustomValue::Rating(4)));
    assert_eq!(b.custom.get("#series"), Some(&CustomValue::Series { name: "My Series Two".into(), index: 3.0 }));
    assert_eq!(text_list(b.custom.get("#tags")), ["My Tag Two"]);
    assert_eq!(b.custom.get("#yesno"), Some(&CustomValue::Bool(false)));
    // Not in calibre's test; checked against the raw custom_column_11 rows.
    assert_eq!(b.custom.get("#float"), Some(&CustomValue::Float(10.01)));

    // Files on disk resolve where calibre put them.
    let root = lib.root();
    for b in &snap.books {
        for f in &b.formats {
            assert!(b.format_path(root, f).is_file(), "{:?}", b.format_path(root, f));
        }
        if let Some(c) = b.cover_path(root) {
            assert!(c.is_file());
        }
    }

    // Composite columns are recognised but not stored.
    let isbn = snap.custom_columns.iter().find(|c| c.label == "isbn").unwrap();
    assert_eq!(isbn.composite_template(), Some("{identifiers:select(isbn)}"));

    // Preferences are JSON.
    let saved = lib.preference("saved_searches").unwrap().unwrap();
    assert!(saved.is_object());
}

#[test]
fn search_subset() {
    let lib = Library::open(FIXTURE).unwrap();
    let snap = lib.load().unwrap();
    let ids = |q: &str| {
        let q = Query::parse(q).unwrap();
        let mut v: Vec<i64> = snap.books.iter().filter(|b| q.matches(b)).map(|b| b.id).collect();
        v.sort();
        v
    };
    assert_eq!(ids("author:\"author one\""), [1, 2]);
    assert_eq!(ids("tag:news"), [1]);
    assert_eq!(ids("-tag:news"), [2, 3]);
    assert_eq!(ids("#series:\"my series two\""), [1]);
    assert_eq!(ids("#yesno:yes"), [2]);
    assert_eq!(ids("publisher:two series:\"a series\""), [1]);
    assert_eq!(ids("unknown"), [3]);
}

#[test]
fn opening_never_writes() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("metadata.db");
    fs::copy(FIXTURE, &db).unwrap();
    let before = fs::read(&db).unwrap();

    let lib = Library::open(tmp.path()).unwrap();
    lib.load().unwrap();
    drop(lib);

    assert_eq!(fs::read(&db).unwrap(), before);
    let mut names: Vec<_> = fs::read_dir(tmp.path()).unwrap().map(|e| e.unwrap().file_name()).collect();
    names.sort();
    assert_eq!(names, ["metadata.db"], "no journal/WAL files should be left behind");
}

#[test]
fn rejects_non_calibre_databases() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("metadata.db");
    rusqlite::Connection::open(&db).unwrap().execute_batch("CREATE TABLE t(x);").unwrap();
    let err = Library::open(&db).err().unwrap();
    assert!(err.to_string().contains("does not look like a calibre library"), "{err}");
}
