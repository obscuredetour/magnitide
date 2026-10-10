//! The plugin host in Estuary, with the test metadata source and the test
//! GUI plugin in tests/fixtures. Needs Estuary: the development build in
//! target/estuary/current (scripts/estuary/dev.sh) or MAGNITIDE_ESTUARY.
//! Skipped with a note otherwise, unless MAGNITIDE_REQUIRE_ESTUARY is set
//! (CI), when that's a failure.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use magnitide_catalog::diff::{CoverChange, FileChange};
use magnitide_catalog::{Book, Catalog, Format};
use magnitide_core::hooks::{ImportHooks, ImportOutcome, MetadataPlugins, Precedence};
use magnitide_core::metadata::{Author, BookMetadata};
use magnitide_plugins::shadow::Shadow;
use magnitide_plugins::sources::is_plugin_cover;
use magnitide_plugins::{Hook, HostChoice, PluginKind, Plugins};
use magnitide_sources::Lookup;
use serde_json::{Value, json};

/// A `Plugins` in `dir` using the Estuary found for development, or None
/// (and a note) when there isn't one.
fn plugins_in(dir: &Path) -> Option<Plugins> {
    let plugins = Plugins::new(dir.to_path_buf(), dir.join("hosts"));
    if plugins.estuary().is_none() {
        assert!(
            std::env::var_os("MAGNITIDE_REQUIRE_ESTUARY").is_none(),
            "no Estuary found, and MAGNITIDE_REQUIRE_ESTUARY is set"
        );
        eprintln!("skipped: no Estuary here (run scripts/estuary/dev.sh)");
        return None;
    }
    // The compat package from the source tree, so Python edits are tested directly.
    if std::env::var_os("MAGNITIDE_COMPAT_DIR").is_none() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("python");
        unsafe { std::env::set_var("MAGNITIDE_COMPAT_DIR", src) };
    }
    // The host's `test.exec`, for driving dialogs as a user would.
    unsafe { std::env::set_var("MAGNITIDE_TEST_HOOKS", "1") };
    // MAGNITIDE_PLUGIN_HOST=calibre runs the same tests inside an installed
    // calibre (the oracle for the compat package); otherwise Estuary.
    match std::env::var("MAGNITIDE_PLUGIN_HOST").as_deref() {
        Ok("calibre") => {
            let install = magnitide_plugins::calibre::find_installed();
            assert!(install.is_some(), "MAGNITIDE_PLUGIN_HOST=calibre but no calibre is installed");
            plugins.set_host_choice(HostChoice::Calibre).unwrap();
        }
        // Explicit, so a machine with calibre installed doesn't change what
        // `cargo test` covers (Auto would pick calibre there).
        _ => plugins.set_host_choice(HostChoice::Estuary).unwrap(),
    }
    Some(plugins)
}

/// Whether the tests run inside an installed calibre (MAGNITIDE_PLUGIN_HOST=calibre).
fn in_calibre() -> bool {
    std::env::var("MAGNITIDE_PLUGIN_HOST").as_deref() == Ok("calibre")
}

/// A fixture plugin as calibre expects it: a zip of the folder's files.
fn fixture_zip(dir: &Path, fixture: &str) -> PathBuf {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(fixture);
    let path = dir.join(format!("{fixture}.zip"));
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    for entry in std::fs::read_dir(&src).unwrap() {
        let entry = entry.unwrap();
        if !entry.path().is_file() {
            continue; // a stray __pycache__
        }
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
    let dir = tempfile::tempdir().unwrap();
    let Some(plugins) = plugins_in(&dir.path().join("calibre-plugins")) else { return };
    let status = plugins.host_status();
    assert!(status.installed, "{status:?}");
    assert_eq!(status.compat, "9.15.0");

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
    assert!(log_says(&plugins, "must not break the protocol"), "plugin output goes to the log: {:?}", plugins.log());

    // Removing it.
    plugins.remove(&installed.name).unwrap();
    assert!(plugins.list().is_empty());
    assert!(!dir.path().join("calibre-plugins/plugins/magnitide-test-source.zip").exists());
    // Plugin settings stay in Magnitide's folder.
    assert!(dir.path().join("calibre-plugins/config").is_dir());
}

#[test]
fn not_a_plugin() {
    let dir = tempfile::tempdir().unwrap();
    let Some(plugins) = plugins_in(&dir.path().join("p")) else { return };
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
    small_library_of(root, titles, "EPUB")
}

/// A library of `titles`, each with one file of `format`.
fn small_library_of(root: &Path, titles: &[&str], format: &str) -> Catalog {
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
                formats: vec![Format { format: format.into(), size: 4, name: format!("{title} - Author") }],
                custom: BTreeMap::new(),
            };
            std::fs::create_dir_all(book.dir(root)).unwrap();
            std::fs::write(book.format_path(root, &book.formats[0]), format.to_lowercase()).unwrap();
            book
        })
        .collect();
    Catalog::new(Some("test-library".into()), vec![], books, vec![])
}

/// Run a top-level item of the test GUI plugin's menu, by its text, and wait
/// until the plugin is done.
fn run(plugins: &Plugins, events: &mpsc::Receiver<Value>, item: &str, selection: &[i64]) -> Value {
    // No valid path: the host finds the item by its text.
    plugins.gui_trigger("Magnitide Test Action", &[u32::MAX], &[item.to_owned()], selection, "").unwrap();
    loop {
        let e = events.recv_timeout(Duration::from_secs(60)).expect("the plugin finishes");
        if e["event"] == "gui-idle" {
            return e;
        }
    }
}

#[test]
fn gui_plugin_end_to_end() {
    let dir = tempfile::tempdir().unwrap();
    let Some(plugins) = plugins_in(&dir.path().join("calibre-plugins")) else { return };
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
    // What plugins' settings pages read off the book list model.
    let model = plugins
        .host()
        .unwrap()
        .call(
            "test.exec",
            json!({ "code": "from magnitide_host.gui import main_window\nm = main_window().library_view.model()\nresult = (m.orig_headers['rating'], m.headers['title'], m.column_map[0], m.db is not None)\n" }),
            Duration::from_secs(10),
        )
        .unwrap();
    assert_eq!(model["result"], "('Rating', 'Title', 'title', True)");
    let menu = &opened.menus[0];
    assert_eq!((menu.plugin.as_str(), menu.text.as_str(), menu.main), ("Magnitide Test Action", "Test Action", true));
    let texts: Vec<&str> = menu.items.iter().filter(|i| !i.separator).map(|i| i.text.as_str()).collect();
    assert_eq!(texts[0], "Mark selected", "accelerator stripped");
    let more = menu.items.iter().find(|i| i.text == "More").unwrap();
    assert!(!more.children[0].enabled && more.children[1].checkable && more.children[1].checked);

    // Metadata through both of calibre's APIs; done only once the plugin is.
    let mark = menu.items.iter().find(|i| i.text == "Mark selected").unwrap();
    plugins.gui_trigger("Magnitide Test Action", &mark.path, std::slice::from_ref(&mark.text), &[1], "").unwrap();
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
    plugins.gui_trigger("Magnitide Test Action", &[u32::MAX], &["Say hello".into()], &[1, 2], "").unwrap();
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
    let dir = tempfile::tempdir().unwrap();
    let Some(plugins) = plugins_in(&dir.path().join("calibre-plugins")) else { return };
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
        plugins.gui_trigger("Magnitide Test Action", &[u32::MAX], &[item.to_owned()], selection, "").unwrap()
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

/// Wait for an event of `kind` (skipping others); the host's log is shown
/// if none comes in 60 s.
fn next_event(plugins: &Plugins, events: &mpsc::Receiver<Value>, kind: &str) -> Value {
    next_event_of(plugins, events, &[kind])
}

/// Wait for the first event of any of `kinds` (skipping others).
fn next_event_of(plugins: &Plugins, events: &mpsc::Receiver<Value>, kinds: &[&str]) -> Value {
    loop {
        let e = events.recv_timeout(Duration::from_secs(60)).unwrap_or_else(|_| {
            // Why the host thinks a plugin is still working.
            let why = plugins.host().and_then(|h| {
                h.call(
                    "test.exec",
                    json!({ "code": "from qt.core import QApplication\nfrom magnitide_host.gui import main_window, session\ngui = main_window()\nresult = {'running': session.state['running'], 'jobs': [(j.description, j.is_finished) for j in gui.job_manager.jobs], 'widgets': [(w.metaObject().className(), w.windowTitle()) for w in QApplication.topLevelWidgets() if w is not gui and w.isVisible()]}\n" }),
                    Duration::from_secs(10),
                )
            });
            panic!("no {} event; busy: {why:?}; the host said:\n{}", kinds.join("/"), plugins.log().join("\n"))
        });
        if kinds.iter().any(|k| e["event"] == *k) {
            return e;
        }
    }
}

/// Whether the host's log has a line saying `needle`, waiting a little:
/// what a plugin prints reaches the log on the host's stderr, read by its
/// own thread, so it can land just after the reply on stdout.
fn log_says(plugins: &Plugins, needle: &str) -> bool {
    let started = std::time::Instant::now();
    loop {
        if plugins.log().iter().any(|l| l.contains(needle)) {
            return true;
        }
        if started.elapsed() > Duration::from_secs(5) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn file_type_plugins_rewrite_added_files_and_run_afterwards() {
    let dir = tempfile::tempdir().unwrap();
    let Some(plugins) = plugins_in(&dir.path().join("calibre-plugins")) else { return };
    let installed = plugins.install(&fixture_zip(dir.path(), "test_filetype")).unwrap();
    assert_eq!((installed.kind, installed.enabled, installed.customizable), (PluginKind::FileType, true, true));
    let hooks = installed.hooks.as_ref().unwrap();
    assert_eq!(hooks.file_types, ["txt"]);
    assert!(hooks.on_import && hooks.on_postimport && hooks.on_postadd && hooks.on_postconvert && hooks.on_postdelete);
    assert!(!hooks.on_preprocess && hooks.priority == 2);
    assert_eq!(ImportHooks::file_types(&plugins), ["txt"]);
    assert!(plugins.wants_post(Hook::Postadd, "TXT") && !plugins.wants_post(Hook::Postadd, "EPUB"));

    // on_import: the plugin's rewritten copy is added instead, and deleted after.
    let txt = dir.path().join("story.txt");
    std::fs::write(&txt, "Once upon a time").unwrap();
    assert_eq!(plugins.on_import(&txt, "EPUB"), ImportOutcome::Unchanged, "not its format");
    let ImportOutcome::Replaced { path, temporary } = plugins.on_import(&txt, "TXT") else {
        panic!("not replaced");
    };
    assert!(temporary && path != txt);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "Once upon a time\n[hooked]");
    assert_eq!(std::fs::read_to_string(&txt).unwrap(), "Once upon a time", "the original is untouched");
    // A crashing plugin leaves the file as it is; the crash is in the log.
    let crash = dir.path().join("crash.txt");
    std::fs::write(&crash, "x").unwrap();
    assert_eq!(plugins.on_import(&crash, "TXT"), ImportOutcome::Unchanged);
    assert!(log_says(&plugins, "crashed on purpose"), "{:?}", plugins.log());

    // Post hooks, on the shadow library, without a plugin window.
    let root = dir.path().join("library");
    let catalog = small_library_of(&root, &["Dune", "Hyperion"], "TXT");
    let shadow = Shadow::new(dir.path().join("shadow"));
    shadow.build(&catalog, &root, &Default::default()).unwrap();
    let ran = plugins.run_post(Hook::Postimport, &shadow.library(), 1, &["TXT".into()]).unwrap();
    assert_eq!(ran, ["Magnitide Test File Type"]);
    plugins.run_post(Hook::Postadd, &shadow.library(), 1, &["TXT".into()]).unwrap();
    plugins.run_post(Hook::Postdelete, &shadow.library(), 2, &["EPUB".into()]).unwrap();
    assert!(plugins.run_post(Hook::Postconvert, &shadow.library(), 2, &["PDF".into()]).unwrap().is_empty());
    let changes = shadow.changes(&catalog, &root).unwrap();
    let one = changes.books.iter().find(|b| b.id == 1).unwrap();
    assert_eq!(one.fields.tags.as_deref(), Some(&["Hooked".to_owned()][..]));
    assert_eq!(one.fields.publisher, Some(Some("Postadd TXT".to_owned())));
    assert!(changes.books.iter().all(|b| b.id != 2), "the hook asked for only runs for its formats");
    assert!(plugins.errors().is_empty(), "{:?}", plugins.errors());
}

#[test]
fn metadata_reader_and_writer_plugins() {
    let dir = tempfile::tempdir().unwrap();
    let Some(plugins) = plugins_in(&dir.path().join("calibre-plugins")) else { return };
    assert_eq!(plugins.reads("TXT"), None);
    let reader = plugins.install(&fixture_zip(dir.path(), "test_metadata_reader")).unwrap();
    let writer = plugins.install(&fixture_zip(dir.path(), "test_metadata_writer")).unwrap();
    assert_eq!((reader.kind, writer.kind), (PluginKind::MetadataReader, PluginKind::MetadataWriter));
    assert_eq!(plugins.reads("TXT"), Some(Precedence::Override), "priority 2 beats a built-in reader");
    assert_eq!(plugins.reads("EPUB"), None);
    assert!(plugins.writes("TXT") && !plugins.writes("EPUB"));

    let txt = dir.path().join("book.txt");
    std::fs::write(&txt, "Title: The Left Hand of Darkness\nAuthor: Ursula K. Le Guin\nISBN: 9780441478125\nTags: SF, Hainish\n\nWinter.").unwrap();
    let meta = plugins.read(&txt, "TXT").unwrap();
    assert_eq!(meta.title.as_deref(), Some("The Left Hand of Darkness"));
    assert_eq!(meta.authors.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), ["Ursula K. Le Guin"]);
    assert_eq!(meta.identifiers.get("isbn").map(String::as_str), Some("9780441478125"));
    assert_eq!(meta.tags, ["SF", "Hainish"]);

    let wanted = BookMetadata {
        title: Some("The Dispossessed".into()),
        authors: vec![Author { name: "Ursula K. Le Guin".into(), sort: None }],
        tags: vec!["Anarres".into()],
        ..Default::default()
    };
    plugins.write(&txt, "TXT", &wanted).unwrap();
    let after = std::fs::read_to_string(&txt).unwrap();
    assert!(
        after.starts_with("Title: The Dispossessed\nAuthor: Ursula K. Le Guin\nTags: Anarres\n\nWinter."),
        "{after}"
    );
    assert_eq!(plugins.read(&txt, "TXT").unwrap().title.as_deref(), Some("The Dispossessed"));

    // A reader that crashes is an error with its name, not a hang or a dead host.
    std::fs::write(&txt, "Crash: now\n").unwrap();
    let err = plugins.read(&txt, "TXT").unwrap_err();
    assert!(err.contains("Magnitide Test Reader") && err.contains("crashed on purpose"), "{err}");
    assert_eq!(plugins.errors().get("Magnitide Test Reader"), Some(&err));
    assert!(plugins.host().unwrap().is_alive());
}

/// Have the host accept the next modal dialog as a user would, typing
/// `text` into its line edits first.
fn accept_next_dialog(plugins: &Plugins, text: &str) {
    let code = format!(
        r#"
from qt.core import QApplication, QLineEdit, QTimer
def tick():
    w = QApplication.activeModalWidget()
    if w is None:
        QTimer.singleShot(100, tick)
        return
    for edit in w.findChildren(QLineEdit):
        edit.setText({text:?})
    w.accept()
QTimer.singleShot(100, tick)
"#
    );
    plugins.host().unwrap().call("test.exec", json!({ "code": code }), Duration::from_secs(10)).unwrap();
}

#[test]
fn plugins_own_settings_dialogs() {
    let dir = tempfile::tempdir().unwrap();
    let Some(plugins) = plugins_in(&dir.path().join("calibre-plugins")) else { return };
    plugins.install(&fixture_zip(dir.path(), "test_filetype")).unwrap();
    let prefs = plugins.install(&fixture_zip(dir.path(), "test_preferences")).unwrap();
    assert_eq!((prefs.kind, prefs.enabled, prefs.customizable), (PluginKind::Preferences, true, true));

    // A plugin's config_widget, in calibre's dialog; save_settings ran.
    accept_next_dialog(&plugins, "configured by test");
    let done = plugins.configure("Magnitide Test File Type").unwrap();
    assert!(done.changed && !done.restart);
    let txt = dir.path().join("story.txt");
    std::fs::write(&txt, "Once").unwrap();
    let ImportOutcome::Replaced { path, .. } = plugins.on_import(&txt, "TXT") else {
        panic!("not replaced; the host said:\n{}", plugins.log().join("\n"))
    };
    assert_eq!(std::fs::read_to_string(path).unwrap(), "Once\n[configured by test]", "its new settings apply");
    let saved = dir.path().join("calibre-plugins/config/plugins/magnitide_test_filetype.json");
    assert!(std::fs::read_to_string(saved).unwrap().contains("configured by test"));

    // A preferences page: its widget's commit asked for a restart, so the host restarts.
    accept_next_dialog(&plugins, "good evening");
    let done = plugins.configure("Magnitide Test Preferences").unwrap();
    assert!(done.changed && done.restart);
    let saved = dir.path().join("calibre-plugins/config/plugins/magnitide_test_preferences.json");
    let text = std::fs::read_to_string(saved)
        .unwrap_or_else(|e| panic!("preferences not saved ({e}); the host said:\n{}", plugins.log().join("\n")));
    assert!(text.contains("good evening"), "{text}");

    // A GUI plugin's settings, before any library was opened in the host:
    // its action is loaded into the window first (it reads gui.keyboard).
    let (tx, events) = mpsc::channel();
    plugins.set_event_handler(Arc::new(move |e| {
        let _ = tx.send(e);
    }));
    let action = plugins.install(&fixture_zip(dir.path(), "test_action")).unwrap();
    assert!(action.customizable);
    accept_next_dialog(&plugins, "hello from settings");
    let done = plugins.configure("Magnitide Test Action").unwrap();
    assert!(done.changed && !done.restart);
    let saved = dir.path().join("calibre-plugins/config/plugins/magnitide_test_action.json");
    assert!(std::fs::read_to_string(saved).unwrap().contains("hello from settings"));
    assert_eq!(next_event(&plugins, &events, "gui-status")["message"], "Settings: hello from settings");

    // Cancelled: nothing saved.
    plugins
        .host()
        .unwrap()
        .call(
            "test.exec",
            json!({ "code": "from qt.core import QApplication, QTimer\ndef tick():\n    w = QApplication.activeModalWidget()\n    if w is None:\n        QTimer.singleShot(100, tick)\n        return\n    w.reject()\nQTimer.singleShot(100, tick)\n" }),
            Duration::from_secs(10),
        )
        .unwrap();
    let done = plugins.configure("Magnitide Test File Type").unwrap();
    assert!(!done.changed);
    assert!(plugins.configure("No Such Plugin").is_err());
}

#[test]
fn library_closed_plugins_run_when_a_host_is_up() {
    let dir = tempfile::tempdir().unwrap();
    let Some(plugins) = plugins_in(&dir.path().join("calibre-plugins")) else { return };
    let installed = plugins.install(&fixture_zip(dir.path(), "test_library_closed")).unwrap();
    assert_eq!((installed.kind, installed.enabled), (PluginKind::LibraryClosed, true));
    let root = dir.path().join("library");
    let catalog = small_library(&root, &["Dune", "Hyperion"]);
    let shadow = Shadow::new(dir.path().join("shadow"));
    shadow.build(&catalog, &root, &Default::default()).unwrap();
    let note = shadow.library().join("closed-by-plugin.txt");

    // No host running (a fresh start, with the plugin installed): nothing is started for it.
    let fresh = Plugins::new(dir.path().join("calibre-plugins"), dir.path().join("calibre-plugins/hosts"));
    assert_eq!(fresh.list().len(), 1);
    assert!(fresh.library_closed(&shadow.library()).unwrap().is_empty());
    assert!(!note.exists());
    // Installing started one; it runs the plugin.
    assert_eq!(plugins.library_closed(&shadow.library()).unwrap(), ["Magnitide Test Library Closed"]);
    assert_eq!(std::fs::read_to_string(&note).unwrap(), "2 books");
    assert!(shadow.changes(&catalog, &root).unwrap().is_empty());
}

#[test]
fn unsupported_kinds_are_kept_off_with_a_reason() {
    let dir = tempfile::tempdir().unwrap();
    let Some(plugins) = plugins_in(&dir.path().join("calibre-plugins")) else { return };
    let installed = plugins.install(&fixture_zip(dir.path(), "test_unsupported")).unwrap();
    assert_eq!((installed.kind, installed.enabled), (PluginKind::Catalog, false));
    assert!(!installed.kind.supported());
    assert!(installed.kind.why_unsupported().unwrap().contains("catalog"));
    assert!(installed.hooks.is_none());
    // The static scan found the part of calibre it uses that Magnitide doesn't have.
    if in_calibre() {
        assert!(installed.coverage.is_empty(), "a real calibre has it all: {:?}", installed.coverage);
    } else {
        assert!(installed.coverage.iter().any(|c| c.contains("calibre.ebooks.conversion")), "{:?}", installed.coverage);
    }
    assert!(plugins.install(&fixture_zip(dir.path(), "test_filetype")).unwrap().coverage.is_empty());
}

#[test]
fn jobs_search_view_download_and_convert_from_a_gui_plugin() {
    let dir = tempfile::tempdir().unwrap();
    let Some(plugins) = plugins_in(&dir.path().join("calibre-plugins")) else { return };
    let (tx, events) = mpsc::channel();
    plugins.set_event_handler(Arc::new(move |e| {
        let _ = tx.send(e);
    }));
    plugins.install(&fixture_zip(dir.path(), "test_action")).unwrap();
    let root = dir.path().join("library");
    let catalog = small_library(&root, &["Dune", "Hyperion"]);
    let shadow = Shadow::new(dir.path().join("shadow"));
    shadow.build(&catalog, &root, &Default::default()).unwrap();
    plugins.gui_open(&shadow.library()).unwrap();
    let trigger = |item: &str, selection: &[i64], search: &str| {
        plugins.gui_trigger("Magnitide Test Action", &[u32::MAX], &[item.to_owned()], selection, search).unwrap()
    };

    // A job through the job manager: reported as it goes; idle only once it's done.
    trigger("Run a job", &[1, 2], "");
    let started = next_event(&plugins, &events, "job-start");
    assert_eq!(started["description"], "Test job on 2 books");
    assert_eq!(started["plugin"], "Magnitide Test Action");
    // Progress is polled (every 500 ms), so a short job may be done before
    // any is reported: take whichever comes first.
    let first = next_event_of(&plugins, &events, &["job-progress", "job-done"]);
    let finished = if first["event"] == "job-progress" {
        assert!(first["percent"].as_u64().unwrap() > 0, "{first}");
        next_event(&plugins, &events, "job-done")
    } else {
        first
    };
    assert_eq!(finished["failed"], false);
    assert!(finished["details"].as_str().unwrap().contains("worked on 2"), "{finished}");
    next_event(&plugins, &events, "gui-idle");
    let changes = shadow.changes(&catalog, &root).unwrap();
    assert_eq!(changes.books.len(), 2);
    assert_eq!(changes.books[0].fields.tags.as_deref(), Some(&["Jobbed".to_owned()][..]));

    // calibre's search box: what Magnitide's search says, and changes go to Magnitide.
    trigger("Say the search", &[], "tag:Marked");
    assert_eq!(next_event(&plugins, &events, "gui-status")["message"], "Search: tag:Marked");
    next_event(&plugins, &events, "gui-idle");
    trigger("Search for Dune", &[], "");
    assert_eq!(next_event(&plugins, &events, "gui-search")["text"], "Dune");
    next_event(&plugins, &events, "gui-idle");

    // View: Magnitide opens the book.
    trigger("View book", &[2], "");
    let open = next_event(&plugins, &events, "gui-open-book");
    assert_eq!((open["id"].as_i64(), open["format"].as_str()), (Some(2), None));
    next_event(&plugins, &events, "gui-idle");

    // Metadata download: Magnitide's Find Metadata; the plugin sees what was saved.
    trigger("Download metadata", &[1], "");
    let downloaded = |e: &Value| {
        let id = e["ids"][0].as_i64().unwrap();
        json!({ "books": { id.to_string(): { "title": "Dune Messiah", "authors": ["Frank Herbert"], "tags": [], "languages": [], "identifiers": {} }}})
    };
    let (status, asked) = wait_for(&plugins, &events, "gui-status", &downloaded);
    assert_eq!(asked[0]["kind"], "download");
    assert_eq!(status["message"], "Downloaded: Dune Messiah");
    wait_for(&plugins, &events, "gui-idle", &downloaded);

    // The conversion dialog: Magnitide converts; the new format joins the shadow as its file.
    let azw3 = dir.path().join("converted.azw3");
    std::fs::write(&azw3, "azw3").unwrap();
    trigger("Convert selected", &[2], "");
    let converted = |e: &Value| {
        let id = e["ids"][0].as_i64().unwrap();
        json!({ "formats": { id.to_string(): [{ "format": "EPUB", "path": "/nonexistent" }, { "format": "AZW3", "path": azw3 }] }})
    };
    let (status, asked) = wait_for(&plugins, &events, "gui-status", &converted);
    assert_eq!(asked[0]["kind"], "convert");
    assert_eq!(status["message"], "Formats: AZW3,EPUB");
    wait_for(&plugins, &events, "gui-idle", &converted);
    let after = shadow.read().unwrap();
    let mut formats: Vec<&str> = after.get(2).unwrap().formats.iter().map(|f| f.format.as_str()).collect();
    formats.sort_unstable();
    assert_eq!(formats, ["AZW3", "EPUB"]);
    assert!(!plugins.gui_busy());
}

/// calibre's own build, downloaded by Magnitide, as the host: a large
/// download (calibre's release for this computer), so only with
/// MAGNITIDE_TEST_CALIBRE_DOWNLOAD set. The download is cached in
/// target/calibre-download/ between runs.
#[test]
fn downloaded_calibre_hosts_plugins() {
    if std::env::var_os("MAGNITIDE_TEST_CALIBRE_DOWNLOAD").is_none() {
        eprintln!("skipped: set MAGNITIDE_TEST_CALIBRE_DOWNLOAD to download calibre and run the host in it");
        return;
    }
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/calibre-download");
    let dir = tempfile::tempdir().unwrap();
    let plugins = Plugins::new(dir.path().join("calibre-plugins"), base.clone());
    unsafe { std::env::set_var("MAGNITIDE_COMPAT_DIR", Path::new(env!("CARGO_MANIFEST_DIR")).join("python")) };
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let mut last = String::new();
    let status = plugins
        .install_calibre(&cancel, &mut |p| {
            let line = format!("{p:?}");
            if line != last {
                eprintln!("{line}");
                last = line;
            }
        })
        .unwrap();
    assert!(status.calibre_download.installed, "{status:?}");
    assert_eq!(status.calibre_download.version, magnitide_plugins::calibre::CALIBRE_VERSION);
    let install = magnitide_plugins::calibre::locate_downloaded(&base).unwrap();
    assert_eq!(install.problem, None, "{install:?}");
    assert!(install.version.starts_with(&magnitide_plugins::calibre::CALIBRE_VERSION[..4]), "{install:?}");

    // Plugins run in it.
    plugins.set_host_choice(HostChoice::Downloaded).unwrap();
    assert!(matches!(plugins.launcher(), Ok(magnitide_plugins::Launcher::Calibre(_))));
    plugins.install(&test_plugin_zip(dir.path())).unwrap();
    let answers = plugins.identify(&Lookup { title: "Project Hail Mary".into(), ..Default::default() });
    assert_eq!(answers[0].1.as_ref().unwrap()[0].title, "Project Hail Mary");
    let hello = plugins.host().unwrap().call("hello", json!({}), Duration::from_secs(30)).unwrap();
    assert_eq!(hello["host"], "calibre");
}
