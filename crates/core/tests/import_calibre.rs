//! Importing calibre's own test library (src/calibre/db/tests/metadata.db,
//! shared with calibre-compat's tests), with real files and covers on disk.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use magnitide_core::import::import_calibre;
use magnitide_core::{Error, Library};
use rusqlite::Connection;

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../calibre-compat/tests/fixtures/calibre_test_metadata.db");

/// A calibre library like calibre's test setup builds: the fixture database
/// plus format files (FMT1/FMT2) and covers for books 1 and 2.
fn calibre_library(dir: &Path) {
    let db = dir.join("metadata.db");
    fs::copy(FIXTURE, &db).unwrap();
    let conn = Connection::open(&db).unwrap();
    calibre_compat::sqlfuncs::register(&conn).unwrap();
    for (book, fmt, body) in [(1, "FMT1", "book1fmt1"), (1, "FMT2", "book1fmt2"), (2, "FMT1", "book2fmt1")] {
        let (path, title, author): (String, String, String) = conn
            .query_row("SELECT path, title, author_sort FROM books WHERE id = ?", [book], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .unwrap();
        let name = format!("{title} - {author}");
        conn.execute(
            "INSERT INTO data (book, format, uncompressed_size, name) VALUES (?, ?, ?, ?)",
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
        fs::write(dir.join(path).join("cover.jpg"), format!("cover of book {book}")).unwrap();
    }
}

/// Every file under `dir` with its contents, to prove nothing changed.
fn snapshot(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut out = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in fs::read_dir(&d).unwrap().flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.insert(p.strip_prefix(dir).unwrap().to_path_buf(), fs::read(&p).unwrap());
            }
        }
    }
    out
}

/// Magnitide id of an imported calibre book.
fn imported_id(dest: &Path, calibre_id: i64) -> i64 {
    Connection::open(dest.join("magnitide.db"))
        .unwrap()
        .query_row(
            "SELECT book_id FROM book_sources WHERE source = 'calibre' AND source_book = ?",
            [calibre_id.to_string()],
            |r| r.get(0),
        )
        .unwrap()
}

#[test]
fn imports_everything_and_leaves_calibre_untouched() {
    let tmp = tempfile::tempdir().unwrap();
    let (src, dest) = (tmp.path().join("calibre"), tmp.path().join("magnitide"));
    fs::create_dir(&src).unwrap();
    calibre_library(&src);
    let before = snapshot(&src);

    let mut seen = Vec::new();
    let report = import_calibre(&src, &dest, |p| seen.push((p.done, p.total))).unwrap();

    assert!(report.library_created);
    assert_eq!((report.books_imported, report.books_skipped), (3, 0));
    assert_eq!((report.files, report.covers), (3, 2));
    assert_eq!(report.cloned + report.copied, 5, "every file and cover is cloned or copied");
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    // 12 calibre custom columns, 3 of them computed (composite) templates.
    assert_eq!(report.custom_fields, 9);
    assert_eq!(report.warnings.iter().filter(|w| w.contains("computed by a calibre template")).count(), 3);
    assert_eq!(seen.last(), Some(&(3, 3)));

    // The calibre library is byte-for-byte unchanged, with nothing added.
    assert_eq!(snapshot(&src), before);

    let lib = Library::open(&dest).unwrap();
    assert_eq!(lib.info().unwrap().book_count, 3);
    assert_eq!(lib.info().unwrap().name, "magnitide", "named after the destination folder");

    // Book 1 ("Title Two"): two authors in order, series, rating, both formats, cover.
    let b = lib.book(imported_id(&dest, 1)).unwrap();
    assert_eq!(b.title, "Title Two");
    assert_eq!(b.authors, ["Author Two", "Author One"]);
    assert_eq!(b.series, Some(("A Series One".into(), 2.0)));
    assert_eq!(b.rating, Some(6));
    assert_eq!(b.languages, ["deu"]);
    assert_eq!(b.uuid, "c67a09ae-ea43-4d7f-b754-d38847319512", "calibre uuid kept");
    assert_eq!(b.cover_file.as_deref(), Some("cover.jpg"));
    let dir = dest.join(&b.path);
    assert_eq!(fs::read_to_string(dir.join("cover.jpg")).unwrap(), "cover of book 1");
    assert_eq!(fs::read_to_string(dir.join("Title Two - Author Two.fmt1")).unwrap(), "book1fmt1");
    assert_eq!(fs::read_to_string(dir.join("Title Two - Author Two.fmt2")).unwrap(), "book1fmt2");

    // Book 3 ("Unknown"): no files, no cover, still imported.
    let b3 = lib.book(imported_id(&dest, 3)).unwrap();
    assert_eq!((b3.title.as_str(), b3.cover_file.as_deref()), ("Unknown", None));

    // Custom values for book 2, by field key.
    let conn = Connection::open(dest.join("magnitide.db")).unwrap();
    let mut stmt = conn
        .prepare(
            "SELECT f.key, f.kind, v.text_value, v.num_value FROM custom_values v
             JOIN custom_fields f ON f.id = v.field_id WHERE v.book_id = ? ORDER BY f.key, v.position",
        )
        .unwrap();
    let values: Vec<(String, String, Option<String>, Option<f64>)> = stmt
        .query_map([imported_id(&dest, 2)], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let get = |key: &str| values.iter().filter(|v| v.0 == key).collect::<Vec<_>>();
    assert_eq!(
        get("authors").iter().map(|v| v.2.as_deref().unwrap()).collect::<Vec<_>>(),
        ["Custom One", "Custom Two"]
    );
    assert_eq!(get("series")[0].2.as_deref(), Some("My Series One"));
    assert_eq!(get("series")[0].3, Some(1.0));
    assert_eq!((get("rating")[0].1.as_str(), get("rating")[0].3), ("rating", Some(2.0)));
    assert_eq!((get("yesno")[0].1.as_str(), get("yesno")[0].3), ("bool", Some(1.0)));
    assert_eq!((get("enum")[0].1.as_str(), get("enum")[0].2.as_deref()), ("choice", Some("One")));
    assert_eq!(get("float")[0].3, Some(20.02));
    assert_eq!(get("comments")[0].1, "long_text");
}

#[test]
fn repeat_import_skips_existing_books() {
    let tmp = tempfile::tempdir().unwrap();
    let (src, dest) = (tmp.path().join("calibre"), tmp.path().join("magnitide"));
    fs::create_dir(&src).unwrap();
    calibre_library(&src);

    import_calibre(&src, &dest, |_| {}).unwrap();
    let again = import_calibre(&src, &dest, |_| {}).unwrap();
    assert!(!again.library_created);
    assert_eq!((again.books_imported, again.books_skipped, again.custom_fields), (0, 3, 0));
    assert_eq!(Library::open(&dest).unwrap().info().unwrap().book_count, 3);
}

#[test]
fn refuses_destination_inside_the_calibre_library() {
    let tmp = tempfile::tempdir().unwrap();
    calibre_library(tmp.path());
    let before = snapshot(tmp.path());
    let err = import_calibre(tmp.path(), &tmp.path().join("inside"), |_| {}).err().unwrap();
    assert!(matches!(err, Error::Import(ref m) if m.contains("must be outside")), "{err}");
    assert_eq!(snapshot(tmp.path()), before);
}

#[test]
fn a_failing_book_is_rolled_back_without_touching_existing_folders() {
    let tmp = tempfile::tempdir().unwrap();
    let (src, dest) = (tmp.path().join("calibre"), tmp.path().join("magnitide"));
    fs::create_dir(&src).unwrap();
    calibre_library(&src);

    // Pre-create the folder the first imported book will want, with a file in it.
    Library::create(&dest, "Existing").unwrap();
    let blocker = dest.join("Author Two/Title Two (1)");
    fs::create_dir_all(&blocker).unwrap();
    fs::write(blocker.join("keep.txt"), "not ours").unwrap();

    let report = import_calibre(&src, &dest, |_| {}).unwrap();
    assert_eq!(report.failed.len(), 1, "{:?}", report.failed);
    assert!(report.failed[0].contains("already exists"));
    assert_eq!(report.books_imported, 2, "the other books still import");
    assert_eq!(Library::open(&dest).unwrap().info().unwrap().book_count, 2, "failed book rolled back");
    assert_eq!(fs::read_to_string(blocker.join("keep.txt")).unwrap(), "not ours", "pre-existing folder kept");
}

/// The strongest check: after importing, the native library loads into the
/// catalog with the same data the calibre library does.
#[test]
fn imported_library_matches_the_calibre_catalog() {
    let tmp = tempfile::tempdir().unwrap();
    let (src, dest) = (tmp.path().join("calibre"), tmp.path().join("magnitide"));
    fs::create_dir(&src).unwrap();
    calibre_library(&src);
    import_calibre(&src, &dest, |_| {}).unwrap();

    let calibre = calibre_compat::Library::open(&src).unwrap().load().unwrap();
    let native = Library::open(&dest).unwrap().load_catalog().unwrap();
    assert!(native.warnings.is_empty(), "{:?}", native.warnings);
    assert_eq!(native.books.len(), calibre.books.len());

    for c in &calibre.books {
        let n = native.get(imported_id(&dest, c.id)).unwrap();
        let sorted = |v: &[String]| {
            let mut v = v.to_vec();
            v.sort();
            v
        };
        assert_eq!(n.title, c.title);
        assert_eq!(n.authors, c.authors, "author order kept");
        assert_eq!(
            (&n.series, n.series.as_ref().map(|_| n.series_index)),
            (&c.series, c.series.as_ref().map(|_| c.series_index))
        );
        assert_eq!(n.publisher, c.publisher);
        assert_eq!(n.rating, c.rating);
        assert_eq!(sorted(&n.tags), sorted(&c.tags));
        assert_eq!(n.languages, c.languages);
        assert_eq!(n.identifiers, c.identifiers);
        assert_eq!(n.uuid, c.uuid);
        assert_eq!(n.has_cover, c.has_cover);
        assert_eq!(n.pubdate.map(|d| d.date_naive()), c.pubdate.map(|d| d.date_naive()));
        assert_eq!(n.timestamp.map(|d| d.timestamp()), c.timestamp.map(|d| d.timestamp()));
        let formats =
            |b: &calibre_compat::Book| b.formats.iter().map(|f| (f.format.clone(), f.size)).collect::<Vec<_>>();
        assert_eq!(formats(n), formats(c));
        // Every stored custom value survives, keyed the same way.
        assert_eq!(n.custom, c.custom, "custom values for {:?}", c.title);
        // Files resolve on disk through the shared path helpers.
        for f in &n.formats {
            assert!(n.format_path(&dest, f).is_file());
        }
    }
}

#[test]
fn refuses_a_non_empty_folder_that_is_not_a_library() {
    let tmp = tempfile::tempdir().unwrap();
    let (src, dest) = (tmp.path().join("calibre"), tmp.path().join("Documents"));
    fs::create_dir(&src).unwrap();
    calibre_library(&src);
    fs::create_dir(&dest).unwrap();
    fs::write(dest.join("taxes.pdf"), "important").unwrap();

    let err = import_calibre(&src, &dest, |_| {}).err().unwrap();
    assert!(matches!(err, Error::Import(ref m) if m.contains("isn't empty")), "{err}");
    assert_eq!(fs::read_dir(&dest).unwrap().count(), 1, "nothing written");

    // An empty existing folder is fine.
    let empty = tmp.path().join("Empty");
    fs::create_dir(&empty).unwrap();
    assert_eq!(import_calibre(&src, &empty, |_| {}).unwrap().books_imported, 3);
}
