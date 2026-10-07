//! The plugin host against a real calibre, with the test metadata source and
//! the test GUI plugin in tests/fixtures. Skipped (with a note) where calibre
//! isn't installed, as on CI; set MAGNITIDE_CALIBRE_DEBUG to point at a
//! calibre-debug elsewhere.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use magnitide_catalog::diff::{CoverChange, FileChange};
use magnitide_catalog::{Book, Catalog, Format};
use magnitide_plugins::shadow::Shadow;
use magnitide_plugins::sources::is_plugin_cover;
use magnitide_plugins::{PluginKind, Plugins};
use magnitide_sources::Lookup;
use serde_json::Value;

fn calibre_debug() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("MAGNITIDE_CALIBRE_DEBUG") {
        return Some(p.into());
    }
    let candidates: &[&str] = if cfg!(target_os = "macos") {
        &["/Applications/calibre.app/Contents/MacOS/calibre-debug"]
    } else if cfg!(windows) {
        &[r"C:\Program Files\Calibre2\calibre-debug.exe"]
    } else {
        &["/opt/calibre/calibre-debug", "/usr/bin/calibre-debug"]
    };
    candidates.iter().map(PathBuf::from).find(|p| p.is_file())
}

/// A fixture plugin as calibre expects it: a zip of the folder's files.
fn fixture_zip(dir: &Path, fixture: &str) -> PathBuf {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(fixture);
    let path = dir.join(format!("{fixture}.zip"));
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    for entry in std::fs::read_dir(&src).unwrap() {
        let entry = entry.unwrap();
        zip.start_file(entry.file_name().to_string_lossy(), zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(&std::fs::read(entry.path()).unwrap()).unwrap();
    }
    zip.finish().unwrap();
    path
}

fn test_plugin_zip(dir: &Path) -> PathBuf {
    fixture_zip(dir, "test_source")
}

#[test]
fn metadata_source_end_to_end() {
    let Some(calibre) = calibre_debug() else {
        eprintln!("skipped: calibre isn't installed here");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let plugins = Plugins::new(dir.path().join("calibre-plugins"), move || Some(calibre.clone()));
    let info = plugins.calibre().expect("calibre found");
    assert!(info.version.as_ref().is_some_and(|v| v.chars().next().is_some_and(|c| c.is_ascii_digit())), "{info:?}");

    // Installing: inspected by calibre's own loader, kept under a tidy name, on.
    let installed = plugins.install(&test_plugin_zip(dir.path())).unwrap();
    assert_eq!((installed.name.as_str(), installed.version.as_str()), ("Magnitide Test Source", "1.2.3"));
    assert_eq!(
        (installed.kind, installed.enabled, installed.file.as_str()),
        (PluginKind::MetadataSource, true, "magnitide-test-source.zip")
    );
    assert_eq!(installed.capabilities, ["cover", "identify"]);
    // Reinstalling (an update) replaces it rather than adding another.
    plugins.set_enabled(&installed.name, false).unwrap();
    plugins.install(&test_plugin_zip(dir.path())).unwrap();
    assert_eq!(plugins.list().len(), 1);
    assert!(!plugins.list()[0].enabled, "an update keeps the user's choice");
    assert!(plugins.identify(&Lookup { title: "Dune".into(), ..Default::default() }).is_empty(), "off: not asked");
    plugins.set_enabled(&installed.name, true).unwrap();

    // A lookup: the plugin's record, converted.
    let lookup = Lookup { title: "Project Hail Mary".into(), authors: vec!["Andy Weir".into()], isbn: None };
    let answers = plugins.identify(&lookup);
    assert_eq!(answers.len(), 1);
    let (name, records) = &answers[0];
    assert_eq!(name, "Magnitide Test Source");
    let records = records.as_ref().unwrap();
    assert_eq!(records.len(), 1);
    let r = &records[0];
    assert_eq!((r.title.as_str(), r.authors.as_slice()), ("Project Hail Mary", &["Andy Weir".to_owned()][..]));
    assert_eq!((r.publisher.as_deref(), r.published.as_deref()), (Some("Ballantine Books"), Some("2021-05-04")));
    assert_eq!(r.identifiers.get("isbn").map(String::as_str), Some("9780593135204"));
    assert_eq!((r.languages.as_slice(), r.tags.len()), (&["eng".to_owned()][..], 2));
    assert_eq!(r.link.as_deref(), Some("https://example.invalid/book/42"));
    let description = r.description_html.as_deref().unwrap();
    assert!(description.contains("<p") && description.contains("&amp;"), "plain text became HTML: {description}");

    // Its cover, through the plugin (it remembered the URL from the lookup).
    let cover_url = r.cover_url.as_deref().unwrap();
    assert!(is_plugin_cover(cover_url));
    let png = plugins.cover(cover_url).unwrap();
    assert!(png.starts_with(b"\x89PNG"), "{} bytes", png.len());

    // Failures are the plugin's own message; a crash doesn't take the host down.
    let failed = plugins.identify(&Lookup { title: "Fail".into(), ..Default::default() });
    assert_eq!(failed[0].1.as_ref().unwrap_err(), "This source failed on purpose.");
    let crashed = plugins.identify(&Lookup { title: "Crash".into(), ..Default::default() });
    assert!(crashed[0].1.as_ref().unwrap_err().contains("the plugin crashed"), "{crashed:?}");
    assert!(plugins.host().unwrap().is_alive());
    assert!(plugins.log().iter().any(|l| l.contains("must not break the protocol")), "plugin output goes to the log");

    // Removing it.
    plugins.remove(&installed.name).unwrap();
    assert!(plugins.list().is_empty());
    assert!(!dir.path().join("calibre-plugins/plugins/magnitide-test-source.zip").exists());
    // Plugin settings stay in Magnitide's folder.
    assert!(dir.path().join("calibre-plugins/config").is_dir());
}

#[test]
fn not_a_plugin() {
    let Some(calibre) = calibre_debug() else {
        eprintln!("skipped: calibre isn't installed here");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let plugins = Plugins::new(dir.path().join("p"), move || Some(calibre.clone()));
    let junk = dir.path().join("junk.zip");
    std::fs::write(&junk, b"not a zip").unwrap();
    let err = plugins.install(&junk).unwrap_err();
    assert!(err.starts_with("That isn't a calibre plugin"), "{err}");
    assert!(plugins.list().is_empty());
    let leftovers: Vec<_> = std::fs::read_dir(dir.path().join("p/plugins")).unwrap().collect();
    assert!(leftovers.is_empty(), "the rejected copy is removed");
}

/// A library of `titles`, each with an EPUB, as Magnitide's catalog would have it.
fn small_library(root: &Path, titles: &[&str]) -> Catalog {
    let books = (1..)
        .zip(titles)
        .map(|(id, title)| {
            let book = Book {
                id,
                title: title.to_string(),
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
                has_cover: false,
                formats: vec![Format { format: "EPUB".into(), size: 4, name: format!("{title} - Author") }],
                custom: BTreeMap::new(),
            };
            std::fs::create_dir_all(book.dir(root)).unwrap();
            std::fs::write(book.format_path(root, &book.formats[0]), "epub").unwrap();
            book
        })
        .collect();
    Catalog::new(Some("test-library".into()), vec![], books, vec![])
}

/// Run a top-level item of the test GUI plugin's menu, by its text, and wait
/// until the plugin is done.
fn run(plugins: &Plugins, events: &mpsc::Receiver<Value>, item: &str, selection: &[i64]) -> Value {
    // No valid path: the host finds the item by its text.
    plugins.gui_trigger("Magnitide Test Action", &[u32::MAX], &[item.to_owned()], selection).unwrap();
    loop {
        let e = events.recv_timeout(Duration::from_secs(60)).expect("the plugin finishes");
        if e["event"] == "gui-idle" {
            return e;
        }
    }
}

#[test]
fn gui_plugin_end_to_end() {
    let Some(calibre) = calibre_debug() else {
        eprintln!("skipped: calibre isn't installed here");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let plugins = Plugins::new(dir.path().join("calibre-plugins"), move || Some(calibre.clone()));
    let (tx, events) = mpsc::channel();
    plugins.set_event_handler(Arc::new(move |e| {
        let _ = tx.send(e);
    }));
    let installed = plugins.install(&fixture_zip(dir.path(), "test_action")).unwrap();
    assert_eq!((installed.kind, installed.enabled), (PluginKind::InterfaceAction, true));

    let root = dir.path().join("library");
    let catalog = small_library(&root, &["Dune", "Hyperion"]);
    let shadow = Shadow::new(dir.path().join("shadow"));
    shadow.build(&catalog, &root, &Default::default()).unwrap();

    // Its menu, as the plugin builds it (rebuilt on show, with a submenu).
    let opened = plugins.gui_open(&shadow.library()).unwrap();
    assert!(opened.errors.is_empty(), "{:?}", opened.errors);
    let menu = &opened.menus[0];
    assert_eq!((menu.plugin.as_str(), menu.text.as_str(), menu.main), ("Magnitide Test Action", "Test Action", true));
    let texts: Vec<&str> = menu.items.iter().filter(|i| !i.separator).map(|i| i.text.as_str()).collect();
    assert_eq!(texts[0], "Mark selected", "accelerator stripped");
    let more = menu.items.iter().find(|i| i.text == "More").unwrap();
    assert!(!more.children[0].enabled && more.children[1].checkable && more.children[1].checked);

    // Metadata through both of calibre's APIs; done only once the plugin is.
    let mark = menu.items.iter().find(|i| i.text == "Mark selected").unwrap();
    plugins.gui_trigger("Magnitide Test Action", &mark.path, std::slice::from_ref(&mark.text), &[1]).unwrap();
    let done = loop {
        let e = events.recv_timeout(Duration::from_secs(60)).unwrap();
        if e["event"] == "gui-idle" {
            break e;
        }
    };
    assert_eq!(done["changed"], true, "it refreshed the book list");

    // A file written in place, a cover, a status message, a new book.
    run(&plugins, &events, "Rewrite EPUB", &[2]);
    run(&plugins, &events, "Set cover", &[2]);
    run(&plugins, &events, "Add a book", &[]);
    plugins.gui_trigger("Magnitide Test Action", &[u32::MAX], &["Say hello".into()], &[1, 2]).unwrap();
    let status = loop {
        let e = events.recv_timeout(Duration::from_secs(60)).unwrap();
        if e["event"] == "gui-status" {
            break e;
        }
    };
    assert_eq!(status["message"], "Hello from the test plugin (2 selected)");
    // A dialog, waited for.
    run(&plugins, &events, "Open dialog…", &[2]);
    assert!(!plugins.gui_busy());

    let changes = shadow.changes(&catalog, &root).unwrap();
    let one = changes.books.iter().find(|b| b.id == 1).unwrap();
    assert_eq!(one.fields.title.as_deref(), Some("Dune (marked)"));
    assert_eq!(one.fields.tags.as_deref(), Some(&["Marked".to_owned()][..]));
    let two = changes.books.iter().find(|b| b.id == 2).unwrap();
    assert_eq!(two.fields.title.as_deref(), Some("Hyperion (marked)"), "the dialog's OK marked it");
    assert!(matches!(&two.files[..], [FileChange::Replaced { format, .. }] if format == "EPUB"), "{:?}", two.files);
    assert!(matches!(two.cover, Some(CoverChange::Set(_))));
    assert_eq!(changes.added.len(), 1);
    assert_eq!(changes.added[0].book.title, "Added by a Plugin");
    // The library's own file is untouched; the plugin wrote to its own copy.
    let hyperion = catalog.get(2).unwrap();
    assert_eq!(std::fs::read_to_string(hyperion.format_path(&root, &hyperion.formats[0])).unwrap(), "epub");

    // Closed for a rebuild; reopened, the plugin sees the new library.
    assert!(plugins.gui_close().unwrap());
    shadow.build(&catalog, &root, &Default::default()).unwrap();
    let reopened = plugins.gui_open(&shadow.library()).unwrap();
    assert_eq!(reopened.menus.len(), 1);
    assert!(shadow.changes(&catalog, &root).unwrap().is_empty());

    // Turning it off restarts the host, so it's gone from the menus.
    plugins.set_enabled(&installed.name, false).unwrap();
    assert!(plugins.gui_open(&shadow.library()).unwrap().menus.is_empty());
}

/// Wait for an event of `kind`, answering any `gui-ask` with `answer` (as
/// Magnitide would after its editor closed or it deleted the books).
fn wait_for(
    plugins: &Plugins,
    events: &mpsc::Receiver<Value>,
    kind: &str,
    answer: &dyn Fn(&Value) -> Value,
) -> (Value, Vec<Value>) {
    let mut asked = Vec::new();
    loop {
        let e = events.recv_timeout(Duration::from_secs(60)).expect("the plugin carries on");
        if e["event"] == "gui-ask" {
            asked.push(e.clone());
            let token = e["token"].as_u64().unwrap();
            plugins.gui_answer(token, answer(&e)).unwrap();
            plugins.gui_resume(token).unwrap();
        } else if e["event"] == kind {
            return (e, asked);
        }
    }
}

#[test]
fn calibres_edit_metadata_and_remove_books_are_magnitides() {
    let Some(calibre) = calibre_debug() else {
        eprintln!("skipped: calibre isn't installed here");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let plugins = Plugins::new(dir.path().join("calibre-plugins"), move || Some(calibre.clone()));
    let (tx, events) = mpsc::channel();
    plugins.set_event_handler(Arc::new(move |e| {
        let _ = tx.send(e);
    }));
    plugins.install(&fixture_zip(dir.path(), "test_action")).unwrap();
    let root = dir.path().join("library");
    let catalog = small_library(&root, &["Dune", "Hyperion", "Solaris"]);
    let shadow = Shadow::new(dir.path().join("shadow"));
    shadow.build(&catalog, &root, &Default::default()).unwrap();
    plugins.gui_open(&shadow.library()).unwrap();
    let trigger = |item: &str, selection: &[i64]| {
        plugins.gui_trigger("Magnitide Test Action", &[u32::MAX], &[item.to_owned()], selection).unwrap()
    };

    // Edit Metadata: the plugin waits for the editor, then uses what was saved.
    trigger("New book, then edit it", &[1]);
    let edited = |e: &Value| {
        let id = e["ids"][0].as_i64().unwrap();
        serde_json::json!({ "books": { id.to_string(): {
            "title": "The Collected Dune", "authors": ["Frank Herbert"], "tags": ["Omnibus"],
            "series": "Dune", "series_index": 1.0, "languages": ["eng"], "identifiers": {}, "rating": 8
        }}})
    };
    let (status, asked) = wait_for(&plugins, &events, "gui-status", &edited);
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0]["kind"], "edit");
    let new_id = asked[0]["ids"][0].as_i64().unwrap();
    assert!(new_id > 3, "the book it made, selected: {:?}", asked[0]["ids"]);
    assert_eq!(status["message"], "Merged as “The Collected Dune”");
    wait_for(&plugins, &events, "gui-idle", &edited);
    let after = shadow.read().unwrap();
    let merged = after.get(new_id).unwrap();
    assert_eq!((merged.authors.as_slice(), merged.rating), (&["Frank Herbert".to_owned()][..], Some(8)));
    assert_eq!(merged.formats[0].format, "TXT");

    // Remove Books: Magnitide asks; the plugin sees only what was removed.
    trigger("Remove selected", &[2, 3]);
    let only_two = |_: &Value| serde_json::json!({ "removed": [2] });
    let (status, asked) = wait_for(&plugins, &events, "gui-status", &only_two);
    assert_eq!((asked[0]["kind"].as_str(), &asked[0]["ids"]), (Some("remove"), &serde_json::json!([2, 3])));
    assert_eq!(status["message"], "Removed 1 of 2");
    wait_for(&plugins, &events, "gui-idle", &only_two);
    let after = shadow.read().unwrap();
    assert!(after.get(2).is_none() && after.get(3).is_some());
    let solaris = catalog.get(3).unwrap();
    assert!(solaris.format_path(&root, &solaris.formats[0]).is_file(), "the library's files are Magnitide's to delete");
    assert!(catalog.get(2).unwrap().format_path(&root, &catalog.get(2).unwrap().formats[0]).is_file());

    // apply_metadata_changes: calibre's way, tags merged.
    trigger("Apply metadata changes", &[1, 3]);
    let (status, _) = wait_for(&plugins, &events, "gui-status", &|_| Value::Null);
    assert_eq!(status["message"], "Applied to 2");
    wait_for(&plugins, &events, "gui-idle", &|_| Value::Null);
    let changes = shadow.changes(&catalog, &root).unwrap();
    let dune = changes.books.iter().find(|b| b.id == 1).unwrap();
    assert_eq!(dune.fields.publisher, Some(Some("Plugin Press".into())));
    assert_eq!(dune.fields.tags.as_deref(), Some(&["Applied".to_owned()][..]));

    // Convert Books: the output format is Magnitide's setting; conversions
    // are asked for (Magnitide runs them), not done in calibre.
    plugins.set_output_format("txt").unwrap();
    plugins.gui_open(&shadow.library()).unwrap();
    trigger("Auto-convert selected", &[1, 3]);
    let (convert, _) = wait_for(&plugins, &events, "gui-convert", &|_| Value::Null);
    assert_eq!((&convert["ids"], convert["format"].as_str()), (&serde_json::json!([1, 3]), Some("TXT")));
    let (status, _) = wait_for(&plugins, &events, "gui-status", &|_| Value::Null);
    assert_eq!(status["message"], "Converting to TXT", "plugins see Magnitide's setting as calibre's");
    wait_for(&plugins, &events, "gui-idle", &|_| Value::Null);
    // Books that have the format already aren't converted again.
    plugins.set_output_format("EPUB").unwrap();
    plugins.gui_open(&shadow.library()).unwrap();
    trigger("Auto-convert selected", &[1]);
    let mut seen = Vec::new();
    loop {
        let e = events.recv_timeout(Duration::from_secs(60)).expect("the plugin finishes");
        seen.push(e["event"].as_str().unwrap_or_default().to_owned());
        if e["event"] == "gui-idle" {
            break;
        }
    }
    assert!(seen.contains(&"gui-status".to_owned()) && !seen.contains(&"gui-convert".to_owned()), "{seen:?}");
}
