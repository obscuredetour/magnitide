//! The fixture plugins (tests/fixtures/*, Rust components built here for
//! wasm32-wasip2) installed, called, and made to misbehave: a metadata
//! source, an import hook and a file-metadata plugin.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use magnitide_core::hooks::{ImportHooks, ImportOutcome, MetadataPlugins, Precedence};
use magnitide_core::metadata::{Author, BookMetadata};
use magnitide_plugins_wasm::{Capability, Kind, LibraryAccess, WasmPlugins};
use magnitide_sources::Lookup;

/// Build a fixture component once per test binary (cargo caches it).
fn fixture_component(name: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let target = root.join("../../target/wasm-fixtures");
    let status = std::process::Command::new(env!("CARGO"))
        .args(["build", "--release", "--target", "wasm32-wasip2", "--manifest-path"])
        .arg(root.join(format!("tests/fixtures/{name}/Cargo.toml")))
        .arg("--target-dir")
        .arg(&target)
        .status()
        .expect("cargo runs");
    assert!(status.success(), "the fixture plugin builds (rustup target add wasm32-wasip2)");
    target.join(format!("wasm32-wasip2/release/magnitide_{}.wasm", name.replace('-', "_")))
}

/// The metadata-source fixture as a `.mgp`, with the capabilities given.
fn mgp(dir: &Path, name: &str, capabilities: &str) -> PathBuf {
    mgp_of(dir, "test-source", "metadata-source", name, capabilities)
}

fn mgp_of(dir: &Path, fixture: &str, kind: &str, name: &str, capabilities: &str) -> PathBuf {
    let path = dir.join(format!("{name}.mgp"));
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    let opts = zip::write::SimpleFileOptions::default();
    zip.start_file("manifest.toml", opts).unwrap();
    write!(
        zip,
        "name = \"{name}\"\nversion = \"1.2.3\"\nauthor = \"Magnitide\"\ndescription = \"Answers from memory\"\napi = \"0.1\"\nkind = \"{kind}\"\ncapabilities = {capabilities}\ncomponent = \"plugin.wasm\"\n"
    )
    .unwrap();
    zip.start_file("plugin.wasm", opts).unwrap();
    zip.write_all(&std::fs::read(fixture_component(fixture)).unwrap()).unwrap();
    zip.start_file("README.md", opts).unwrap();
    zip.write_all(b"a resource beside the component").unwrap();
    zip.finish().unwrap();
    path
}

#[test]
fn a_metadata_source_plugin_end_to_end() {
    let dir = tempfile::tempdir().unwrap();
    let mut plugins = WasmPlugins::new(dir.path().join("plugins")).unwrap();
    plugins.set_call_timeout(Duration::from_secs(2));

    // Installing: the manifest read, the component checked, kept under a slug.
    let installed = plugins.install(&mgp(dir.path(), "Magnitide Test Source", "[]")).unwrap();
    assert_eq!(
        (installed.manifest.name.as_str(), installed.slug.as_str(), installed.enabled),
        ("Magnitide Test Source", "magnitide-test-source", true)
    );
    assert_eq!(installed.manifest.kind, Kind::MetadataSource);
    assert!(dir.path().join("plugins/magnitide-test-source/plugin.wasm").is_file());
    assert!(dir.path().join("plugins/magnitide-test-source/README.md").is_file(), "resources come along");
    assert_eq!(plugins.list().len(), 1);
    let (name, _, version) = plugins.info(&installed).unwrap();
    assert_eq!((name.as_str(), version.as_str()), ("Magnitide Test Source", "1.2.3"));

    // A lookup: the plugin's record, as a source record.
    let answers =
        plugins.identify(&Lookup { title: "Project Hail Mary".into(), authors: vec!["Andy Weir".into()], isbn: None });
    assert_eq!(answers.len(), 1);
    let records = answers[0].1.as_ref().unwrap();
    let r = &records[0];
    assert_eq!(
        (r.source_name.as_str(), r.title.as_str(), r.authors.as_slice()),
        ("Magnitide Test Source", "Project Hail Mary", &["Andy Weir".to_owned()][..])
    );
    assert_eq!(
        (r.publisher.as_deref(), r.published.as_deref(), r.pages),
        (Some("Ballantine Books"), Some("2021-05-04"), Some(496))
    );
    assert_eq!(r.identifiers.get("isbn").map(String::as_str), Some("9780593135204"));
    assert_eq!(r.languages, ["eng"]);
    assert!(r.source.is_none());
    let log = plugins.log("Magnitide Test Source");
    assert!(log.iter().any(|l| l.contains("info: identify") && l.contains("Project Hail Mary")), "{log:?}");

    // Failures are the plugin's own message, a crash, or the deadline; each is just an error.
    let fail = plugins.identify(&Lookup { title: "Fail".into(), ..Default::default() });
    assert_eq!(fail[0].1.as_ref().unwrap_err(), "This source failed on purpose.");
    let panic = plugins.identify(&Lookup { title: "Panic".into(), ..Default::default() });
    assert!(panic[0].1.as_ref().unwrap_err().contains("crashed"), "{panic:?}");
    let started = std::time::Instant::now();
    let looped = plugins.identify(&Lookup { title: "Loop".into(), ..Default::default() });
    assert!(looped[0].1.as_ref().unwrap_err().contains("took too long"), "{looped:?}");
    assert!(started.elapsed() < Duration::from_secs(10));
    assert!(plugins.errors().get("Magnitide Test Source").unwrap().contains("took too long"));
    // Still answers afterwards: each call is its own instance.
    assert!(plugins.identify(&Lookup { title: "Dune".into(), ..Default::default() })[0].1.is_ok());

    // No network capability: the fetch is refused, by Magnitide, not the network.
    let fetch = plugins.identify(&Lookup { title: "Fetch".into(), ..Default::default() });
    assert!(fetch[0].1.as_ref().unwrap_err().contains("manifest doesn't declare"), "{fetch:?}");

    // Off: not asked. Removed: gone.
    plugins.set_enabled("Magnitide Test Source", false).unwrap();
    assert!(plugins.identify(&Lookup { title: "Dune".into(), ..Default::default() }).is_empty());
    plugins.remove("Magnitide Test Source").unwrap();
    assert!(plugins.list().is_empty());
    assert!(!dir.path().join("plugins/magnitide-test-source").exists());
}

#[test]
fn the_network_capability_reaches_the_host() {
    let dir = tempfile::tempdir().unwrap();
    let plugins = WasmPlugins::new(dir.path().join("plugins")).unwrap();
    let installed = plugins.install(&mgp(dir.path(), "Networked", "[\"network\"]")).unwrap();
    assert_eq!(installed.manifest.capabilities, [Capability::Network]);
    // example.invalid never resolves: the request went out and failed at the network, not at the gate.
    let fetch = plugins.identify(&Lookup { title: "Fetch".into(), ..Default::default() });
    let err = fetch[0].1.as_ref().unwrap_err();
    assert!(!err.contains("manifest doesn't declare"), "{err}");
}

#[test]
fn bad_packages_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let plugins = WasmPlugins::new(dir.path().join("plugins")).unwrap();
    let junk = dir.path().join("junk.mgp");
    std::fs::write(&junk, b"not a zip").unwrap();
    assert!(plugins.install(&junk).unwrap_err().contains("not a zip"));
    // A zip without a component, and one whose API version is wrong.
    let path = dir.path().join("no-component.mgp");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    zip.start_file("manifest.toml", zip::write::SimpleFileOptions::default()).unwrap();
    zip.write_all(b"name = \"X\"\nversion = \"1\"\napi = \"0.1\"\nkind = \"metadata-source\"\n").unwrap();
    zip.finish().unwrap();
    assert!(plugins.install(&path).unwrap_err().contains("isn't in the zip"));
    let path = dir.path().join("old-api.mgp");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    zip.start_file("manifest.toml", zip::write::SimpleFileOptions::default()).unwrap();
    zip.write_all(b"name = \"X\"\nversion = \"1\"\napi = \"9.9\"\nkind = \"metadata-source\"\n").unwrap();
    zip.finish().unwrap();
    assert!(plugins.install(&path).unwrap_err().contains("built for plugin API 9.9"));
    assert!(plugins.list().is_empty() && !dir.path().join("plugins/x").exists());
}

/// The example plugin (plugins/openlibrary-source), built by
/// scripts/build-plugin.sh, looks a real book up. Needs the network:
/// only with `MAGNITIDE_PLUGIN_NETWORK=1`.
#[test]
fn the_open_library_example_plugin_finds_a_book() {
    if std::env::var("MAGNITIDE_PLUGIN_NETWORK").is_err() {
        eprintln!("skipped: set MAGNITIDE_PLUGIN_NETWORK=1 to ask Open Library through the example plugin");
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let status = std::process::Command::new("sh")
        .arg(root.join("scripts/build-plugin.sh"))
        .arg("plugins/openlibrary-source")
        .status()
        .expect("sh runs");
    assert!(status.success(), "the example plugin builds");
    let dir = tempfile::tempdir().unwrap();
    let plugins = WasmPlugins::new(dir.path().join("plugins")).unwrap();
    let installed = plugins.install(&root.join("target/plugins/openlibrary-source.mgp")).unwrap();
    assert_eq!(installed.manifest.capabilities, [Capability::Network]);
    let found = plugins.identify(&Lookup { title: "The Hobbit".into(), authors: vec!["Tolkien".into()], isbn: None });
    let records = found[0].1.as_ref().unwrap_or_else(|e| panic!("{e}\n{:?}", plugins.log("Open Library (plugin)")));
    let hobbit = records.iter().find(|r| r.title.to_lowercase().contains("hobbit")).expect("The Hobbit is found");
    assert!(hobbit.authors.iter().any(|a| a.contains("Tolkien")), "{:?}", hobbit.authors);
    assert!(hobbit.identifiers.iter().any(|(k, _)| k == "openlibrary"));
    assert!(plugins.log("Open Library (plugin)").iter().any(|l| l.contains("GET https://openlibrary.org")));
}

#[test]
fn an_import_hook_rewrites_files_in_its_sandbox() {
    let dir = tempfile::tempdir().unwrap();
    let mut plugins = WasmPlugins::new(dir.path().join("plugins")).unwrap();
    plugins.set_call_timeout(Duration::from_millis(500));
    // A metadata source can't be installed as a hook: wrong world.
    let wrong = plugins.install(&mgp_of(dir.path(), "test-source", "import-hook", "Wrong", "[]")).unwrap_err();
    assert!(wrong.contains("import-hook API"), "{wrong}");
    let hook = plugins.install(&mgp_of(dir.path(), "test-import-hook", "import-hook", "Upper", "[]")).unwrap();
    assert_eq!(
        (hook.manifest.kind, hook.handles.file_types.as_slice()),
        (Kind::ImportHook, &["txt".to_owned(), "mgtest".to_owned()][..])
    );
    assert_eq!(plugins.file_types(), ["mgtest", "txt"], "the extensions become addable");

    // A .mgtest becomes an upper-case .txt the caller must delete.
    let book = dir.path().join("story.mgtest");
    std::fs::write(&book, "once upon a time").unwrap();
    let ImportOutcome::Replaced { path, temporary } = plugins.on_import(&book, "MGTEST") else {
        panic!("{:?}", plugins.on_import(&book, "MGTEST"))
    };
    assert!(temporary);
    assert_eq!(path.extension().unwrap(), "txt");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "ONCE UPON A TIME");
    assert_eq!(std::fs::read_to_string(&book).unwrap(), "once upon a time", "the original is untouched");
    let scratch: Vec<_> = std::fs::read_dir(dir.path().join("plugins/tmp")).unwrap().flatten().collect();
    assert_eq!(scratch.len(), 1, "only the file made survives the call: {scratch:?}");
    assert_eq!(scratch[0].path(), path);
    std::fs::remove_file(&path).unwrap();

    // A plain .txt is left as it is; other formats aren't the hook's business.
    let plain = dir.path().join("plain.txt");
    std::fs::write(&plain, "nothing special").unwrap();
    assert_eq!(plugins.on_import(&plain, "TXT"), ImportOutcome::Unchanged);
    assert_eq!(plugins.on_import(&plain, "EPUB"), ImportOutcome::Unchanged);
    let log = plugins.log("Upper");
    assert!(log.iter().any(|l| l.contains("on-import /in/plain.txt (TXT)")), "{log:?}");

    // Failures: the hook's message, a crash, the deadline; each reported, the file added as it is.
    for (text, expect) in [("FAIL", "failed on purpose"), ("PANIC", "crashed"), ("LOOP", "took too long")] {
        std::fs::write(&plain, text).unwrap();
        let ImportOutcome::Failed(why) = plugins.on_import(&plain, "TXT") else { panic!("{text}") };
        assert!(why.starts_with("Upper: ") && why.contains(expect), "{why}");
    }
    assert!(
        std::fs::read_dir(dir.path().join("plugins/tmp")).unwrap().next().is_none(),
        "scratch folders are cleaned up"
    );
    // Turned off: nothing runs, nothing is addable.
    plugins.set_enabled("Upper", false).unwrap();
    assert!(plugins.file_types().is_empty());
    assert_eq!(plugins.on_import(&plain, "TXT"), ImportOutcome::Unchanged);
}

#[test]
fn a_file_metadata_plugin_reads_and_writes() {
    let dir = tempfile::tempdir().unwrap();
    let plugins = WasmPlugins::new(dir.path().join("plugins")).unwrap();
    let p = plugins.install(&mgp_of(dir.path(), "test-file-metadata", "file-metadata", "Headers", "[]")).unwrap();
    assert_eq!(p.manifest.kind, Kind::FileMetadata);
    let reads: Vec<(&str, bool)> = p.handles.reads.iter().map(|r| (r.format.as_str(), r.overrides)).collect();
    assert_eq!(reads, [("TXT", true), ("MGTEST", false)], "formats are upper-cased");
    assert_eq!(p.handles.writes, ["TXT"]);
    assert_eq!(plugins.reads("TXT"), Some(Precedence::Override));
    assert_eq!(plugins.reads("MGTEST"), Some(Precedence::Fallback));
    assert_eq!(plugins.reads("EPUB"), None);
    assert!(plugins.writes("TXT") && !plugins.writes("MGTEST"));

    let book = dir.path().join("book.txt");
    std::fs::write(&book, "Title: Dune\nAuthor: Frank Herbert\nTag: Classic\nPublished: 1965\nISBN: 9780441013593\n\nThe spice must flow.").unwrap();
    let meta = plugins.read(&book, "TXT").unwrap();
    assert_eq!(meta.title.as_deref(), Some("Dune"));
    assert_eq!(meta.authors, [Author { name: "Frank Herbert".into(), sort: None }]);
    assert_eq!(meta.tags, ["Classic"]);
    assert_eq!(meta.published.map(|d| d.to_string()).as_deref(), Some("1965-01-01"));
    assert_eq!(meta.identifiers.get("isbn").map(String::as_str), Some("9780441013593"));
    assert_eq!(meta.description.as_deref(), Some("The spice must flow."));
    assert_eq!(meta.languages, ["eng"]);

    // Writing: the body is kept, the headers are the metadata given.
    let new = BookMetadata {
        title: Some("Dune Messiah".into()),
        authors: vec![Author { name: "Frank Herbert".into(), sort: Some("Herbert, Frank".into()) }],
        series: Some(("Dune".into(), 2.0)),
        tags: vec!["Classic".into(), "Sequel".into()],
        ..Default::default()
    };
    plugins.write(&book, "TXT", &new).unwrap();
    let text = std::fs::read_to_string(&book).unwrap();
    assert_eq!(
        text,
        "Title: Dune Messiah\nAuthor: Frank Herbert\nSeries: Dune\nIndex: 2\nTag: Classic\nTag: Sequel\n\nThe spice must flow."
    );
    let back = plugins.read(&book, "TXT").unwrap();
    assert_eq!(back.series, Some(("Dune".into(), 2.0)));
    assert!(
        std::fs::read_dir(dir.path().join("plugins/tmp")).unwrap().next().is_none(),
        "scratch folders are cleaned up"
    );
    assert!(plugins.write(&book, "MGTEST", &new).unwrap_err().contains("no plugin writes"));

    // A reader's error is the plugin's message, named.
    std::fs::write(&book, "FAIL").unwrap();
    let err = plugins.read(&book, "TXT").unwrap_err();
    assert_eq!(err, "Headers: This reader failed on purpose.");
    assert_eq!(plugins.errors().get("Headers").map(String::as_str), Some("This reader failed on purpose."));
}

/// A library of three books in memory, as a library action sees it.
struct FakeLibrary {
    books: std::sync::Mutex<Vec<magnitide_catalog::Book>>,
    dir: PathBuf,
    status: std::sync::Mutex<Vec<String>>,
    formats_added: std::sync::Mutex<Vec<(i64, String, String)>>,
}

impl FakeLibrary {
    fn new(dir: &Path) -> Self {
        let book = |id: i64, title: &str, tags: &[&str], txt: Option<&str>| {
            let mut b = magnitide_catalog::Book { id, title: title.into(), ..Default::default() };
            b.tags = tags.iter().map(|t| t.to_string()).collect();
            b.path = format!("Author/{title} ({id})");
            if let Some(text) = txt {
                std::fs::create_dir_all(dir.join(&b.path)).unwrap();
                std::fs::write(dir.join(&b.path).join("book.txt"), text).unwrap();
                b.formats.push(magnitide_catalog::Format {
                    format: "TXT".into(),
                    size: text.len() as u64,
                    name: "book".into(),
                });
            }
            b
        };
        Self {
            books: std::sync::Mutex::new(vec![
                book(1, "Dune", &["unread", "Classic"], Some("the spice must flow")),
                book(2, "Emma", &["unread"], None),
                book(3, "Ulysses", &["Read"], Some("yes I said yes I will Yes")),
            ]),
            dir: dir.to_path_buf(),
            status: Default::default(),
            formats_added: Default::default(),
        }
    }
}

impl magnitide_plugins_wasm::LibraryAccess for FakeLibrary {
    fn fields(&self) -> Result<Vec<magnitide_core::custom::CustomField>, String> {
        Ok(vec![magnitide_core::custom::CustomField {
            id: 1,
            key: "read".into(),
            name: "Read".into(),
            kind: magnitide_core::custom::FieldKind::Bool,
            multiple: false,
            choices: vec![],
        }])
    }

    fn book(&self, id: i64) -> Option<magnitide_catalog::Book> {
        self.books.lock().unwrap().iter().find(|b| b.id == id).cloned()
    }

    fn all_ids(&self) -> Vec<i64> {
        self.books.lock().unwrap().iter().map(|b| b.id).collect()
    }

    fn search(&self, query: &str) -> Result<Vec<i64>, String> {
        let q = magnitide_catalog::search::Query::parse(query).map_err(|e| e.to_string())?;
        Ok(self.books.lock().unwrap().iter().filter(|b| q.matches(b)).map(|b| b.id).collect())
    }

    fn book_edit(&self, id: i64) -> Result<magnitide_core::edit::BookEdit, String> {
        let b = self.book(id).ok_or("no such book")?;
        Ok(magnitide_core::edit::BookEdit {
            title: b.title,
            authors: b.authors,
            tags: b.tags,
            description_html: b.comments,
            series_index: 1.0,
            ..Default::default()
        })
    }

    fn update_book(&self, id: i64, edit: &magnitide_core::edit::BookEdit) -> Result<(), String> {
        let mut books = self.books.lock().unwrap();
        let b = books.iter_mut().find(|b| b.id == id).ok_or("no such book")?;
        b.title = edit.title.clone();
        b.tags = edit.tags.clone();
        b.comments = edit.description_html.clone();
        for (key, value) in &edit.custom {
            match value {
                Some(magnitide_core::custom::FieldValue::Bool(v)) => {
                    b.custom.insert(format!("#{key}"), magnitide_catalog::CustomValue::Bool(*v));
                }
                Some(_) => return Err(format!("unexpected custom value for {key}")),
                None => {
                    b.custom.remove(&format!("#{key}"));
                }
            }
        }
        Ok(())
    }

    fn set_cover(&self, _id: i64, _image: &[u8]) -> Result<(), String> {
        Err("no covers here".into())
    }

    fn format_path(&self, id: i64, format: &str) -> Result<PathBuf, String> {
        let b = self.book(id).ok_or("no such book")?;
        let f = b.formats.iter().find(|f| f.format == format).ok_or_else(|| format!("no {format} file"))?;
        Ok(b.format_path(&self.dir, f))
    }

    fn add_format(&self, id: i64, format: &str, file: &Path) -> Result<(), String> {
        let text = std::fs::read_to_string(file).map_err(|e| e.to_string())?;
        self.formats_added.lock().unwrap().push((id, format.to_owned(), text));
        Ok(())
    }

    fn status(&self, text: &str) {
        self.status.lock().unwrap().push(text.to_owned());
    }
}

#[test]
fn a_library_action_edits_the_library_through_the_host() {
    use std::sync::Arc;

    let dir = tempfile::tempdir().unwrap();
    let mut plugins = WasmPlugins::new(dir.path().join("plugins")).unwrap();
    plugins.set_call_timeout(Duration::from_millis(100));
    let lib = Arc::new(FakeLibrary::new(&dir.path().join("library")));

    // Without capabilities: reading works, editing is refused by the host.
    let p = plugins.install(&mgp_of(dir.path(), "test-library-action", "library-action", "Reader", "[]")).unwrap();
    let menu: Vec<(&str, &str, bool)> =
        p.handles.menu.iter().map(|m| (m.id.as_str(), m.text.as_str(), m.needs_selection)).collect();
    assert_eq!(menu[..2], [("read", "Mark as Read", true), ("count", "Count Words", true)]);
    assert_eq!(plugins.library_actions().len(), 1);
    let report = plugins.run_action("Reader", "survey", &[], lib.clone()).unwrap();
    assert_eq!((report.summary.as_str(), report.updated.as_slice()), ("3 books, 2 unread", &[][..]));
    let refused = plugins.run_action("Reader", "read", &[1], lib.clone()).unwrap_err();
    assert!(refused.contains("edit the library, which its manifest doesn't declare"), "{refused}");
    assert!(lib.book(1).unwrap().tags.contains(&"unread".to_owned()), "nothing changed");
    assert!(plugins.run_action("Reader", "nothing", &[], lib.clone()).unwrap_err().contains("no menu item"));

    // With them: tags and a custom field set, progress reported, books listed as updated.
    let caps = "[\"write-library\", \"read-files\"]";
    plugins.install(&mgp_of(dir.path(), "test-library-action", "library-action", "Reader", caps)).unwrap();
    let report = plugins.run_action("Reader", "read", &[2, 1], lib.clone()).unwrap();
    assert_eq!(report.summary, "Marked 2 books as read");
    assert_eq!(report.updated, [2, 1], "in the order touched");
    let dune = lib.book(1).unwrap();
    assert_eq!(dune.tags, ["unread", "Classic", "Read"]);
    assert_eq!(dune.custom.get("#read"), Some(&magnitide_catalog::CustomValue::Bool(true)));
    // (The refused run reported its first status before the host said no.)
    assert_eq!(lib.status.lock().unwrap().as_slice(), ["Marking Dune…", "Marking Emma…", "Marking Dune…"]);

    // Book files come through the sandbox; a file the plugin wrote becomes a format.
    let report = plugins.run_action("Reader", "count", &[1, 2, 3], lib.clone()).unwrap();
    assert_eq!(report.summary, "Counted words in 2 books");
    assert_eq!(lib.book(1).unwrap().comments.as_deref(), Some("<p>4 words</p>"));
    assert_eq!(lib.book(3).unwrap().comments.as_deref(), Some("<p>7 words</p>"));
    let added = lib.formats_added.lock().unwrap();
    assert_eq!(added[0], (1, "SHOUT".to_owned(), "THE SPICE MUST FLOW".to_owned()));
    assert_eq!(added.len(), 2);
    drop(added);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("library/Author/Dune (1)/book.txt")).unwrap(),
        "the spice must flow"
    );
    assert!(
        std::fs::read_dir(dir.path().join("plugins/tmp")).unwrap().next().is_none(),
        "scratch folders are cleaned up"
    );

    // Failures: the plugin's message, and the deadline.
    assert_eq!(plugins.run_action("Reader", "fail", &[], lib.clone()).unwrap_err(), "This action failed on purpose.");
    let looped = plugins.run_action("Reader", "loop", &[], lib.clone()).unwrap_err();
    assert!(looped.contains("took too long"), "{looped}");
    let log = plugins.log("Reader");
    assert!(log.iter().any(|l| l.contains("status: Marking Dune…")), "{log:?}");
}
