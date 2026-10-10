//! Real calibre plugins (tests/plugins/manifest.json, fetched by
//! scripts/fetch-plugins.py into target/plugin-corpus/) against Magnitide's
//! compat package: each installs, is inspected (kind, hooks, the calibre
//! features it uses that Magnitide lacks), and the GUI ones load in the
//! host on a small shadow library with their menus. The outcome is written
//! to docs/calibre-plugins-coverage.md.
//!
//! Runs only with MAGNITIDE_PLUGIN_CORPUS set (and Estuary, as
//! tests/host.rs); plugins not downloaded are skipped. Plugins failing to
//! load is reported, not a failure: the table is the point.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use magnitide_catalog::{Book, Catalog, Format};
use magnitide_plugins::shadow::Shadow;
use magnitide_plugins::{PluginKind, Plugins};
use serde::Deserialize;

#[derive(Deserialize)]
struct Manifest {
    plugins: Vec<Entry>,
}

#[derive(Deserialize)]
struct Entry {
    name: String,
    file: String,
    #[serde(default)]
    expect: Option<String>,
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn small_library(root: &Path) -> Catalog {
    let books = (1..=2)
        .map(|id| {
            let title = if id == 1 { "Dune" } else { "Hyperion" };
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
    Catalog::new(Some("corpus-library".into()), vec![], books, vec![])
}

struct Outcome {
    name: String,
    version: String,
    kind: String,
    installed: Result<(), String>,
    coverage: Vec<String>,
    /// For GUI plugins: menu items, or why it didn't load.
    loaded: Option<Result<usize, String>>,
    expect: Option<String>,
}

#[test]
fn real_plugins_install_and_load() {
    if std::env::var_os("MAGNITIDE_PLUGIN_CORPUS").is_none() {
        eprintln!("skipped: set MAGNITIDE_PLUGIN_CORPUS to run the plugin corpus");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let plugins = Plugins::new(dir.path().join("calibre-plugins"), dir.path().join("hosts"));
    // MAGNITIDE_PLUGIN_HOST=calibre runs the corpus inside an installed calibre instead.
    let in_calibre = std::env::var("MAGNITIDE_PLUGIN_HOST").as_deref() == Ok("calibre");
    if in_calibre {
        assert!(magnitide_plugins::calibre::find_installed().is_some(), "no installed calibre");
    } else {
        assert!(plugins.estuary().is_some(), "no Estuary (run scripts/estuary/dev.sh)");
    }
    if std::env::var_os("MAGNITIDE_COMPAT_DIR").is_none() {
        unsafe { std::env::set_var("MAGNITIDE_COMPAT_DIR", Path::new(env!("CARGO_MANIFEST_DIR")).join("python")) };
    }
    let manifest: Manifest =
        serde_json::from_str(&std::fs::read_to_string(repo_root().join("tests/plugins/manifest.json")).unwrap())
            .unwrap();
    let corpus = repo_root().join("target/plugin-corpus");

    let root = dir.path().join("library");
    let catalog = small_library(&root);
    let shadow = Shadow::new(dir.path().join("shadow"));
    shadow.build(&catalog, &root, &Default::default()).unwrap();

    let mut outcomes = Vec::new();
    for entry in &manifest.plugins {
        let zip = corpus.join(&entry.file);
        if !zip.is_file() {
            eprintln!("skipped {}: {} not downloaded (scripts/fetch-plugins.py)", entry.name, entry.file);
            continue;
        }
        // Each plugin alone in the host, so one failing to load doesn't hide another.
        let plugins = Plugins::new(dir.path().join(format!("plugins-{}", outcomes.len())), dir.path().join("hosts"));
        plugins
            .set_host_choice(if in_calibre {
                magnitide_plugins::HostChoice::Calibre
            } else {
                magnitide_plugins::HostChoice::Estuary
            })
            .unwrap();
        let mut outcome = Outcome {
            name: entry.name.clone(),
            version: String::new(),
            kind: String::new(),
            installed: Ok(()),
            coverage: vec![],
            loaded: None,
            expect: entry.expect.clone(),
        };
        match plugins.install(&zip) {
            Ok(installed) => {
                outcome.version = installed.version.clone();
                outcome.kind = format!("{:?}", installed.kind);
                outcome.coverage = installed.coverage.clone();
                if installed.kind == PluginKind::InterfaceAction {
                    outcome.loaded = Some(match plugins.gui_open(&shadow.library()) {
                        Ok(menus) if menus.errors.is_empty() => {
                            Ok(menus.menus.iter().map(|m| m.items.len().max(1)).sum())
                        }
                        Ok(menus) => Err(menus.errors.join("; ")),
                        Err(e) => Err(e),
                    });
                    let _ = plugins.gui_close();
                }
            }
            Err(e) => outcome.installed = Err(e),
        }
        eprintln!(
            "{}: {} {:?} loaded={:?} coverage={:?}",
            outcome.name, outcome.kind, outcome.installed, outcome.loaded, outcome.coverage
        );
        outcomes.push(outcome);
    }

    let mut doc = String::new();
    writeln!(doc, "# calibre plugin coverage\n").unwrap();
    if in_calibre {
        writeln!(
            doc,
            "Generated by `MAGNITIDE_PLUGIN_HOST=calibre MAGNITIDE_PLUGIN_CORPUS=1 cargo test -p magnitide-plugins \
             --test corpus` from the plugins in tests/plugins/manifest.json, inside an installed calibre {}.\n",
            magnitide_plugins::calibre::find_installed().map(|c| c.version).unwrap_or_default()
        )
        .unwrap();
    } else {
        writeln!(
            doc,
            "Generated by `MAGNITIDE_PLUGIN_CORPUS=1 cargo test -p magnitide-plugins --test corpus` \
             from the plugins in tests/plugins/manifest.json, against the compat package in \
             crates/plugins/python/compat (calibre {} API).\n",
            magnitide_plugins::CALIBRE_API_VERSION
        )
        .unwrap();
    }
    writeln!(doc, "| Plugin | Version | Kind | Installs | Loads in the host | Uses calibre features Magnitide lacks |")
        .unwrap();
    writeln!(doc, "|---|---|---|---|---|---|").unwrap();
    for o in &outcomes {
        let installs = match &o.installed {
            Ok(()) => "yes".to_owned(),
            Err(e) => format!("no: {}", e.replace('|', "\\|")),
        };
        let loads = match &o.loaded {
            None => "–".to_owned(),
            Some(Ok(items)) => format!("yes ({items} menu items)"),
            Some(Err(e)) => format!("no: {}", e.replace('|', "\\|").replace('\n', " ")),
        };
        let coverage = if o.coverage.is_empty() { "–".to_owned() } else { o.coverage.join(", ") };
        let expect = o.expect.as_deref().map(|e| format!(" (expected: {e})")).unwrap_or_default();
        writeln!(
            doc,
            "| {} | {} | {} | {} | {} | {}{} |",
            o.name, o.version, o.kind, installs, loads, coverage, expect
        )
        .unwrap();
    }
    let name = if in_calibre { "docs/calibre-plugins-coverage-calibre.md" } else { "docs/calibre-plugins-coverage.md" };
    std::fs::write(repo_root().join(name), doc).unwrap();
    assert!(!outcomes.is_empty(), "no corpus plugins found in {}", corpus.display());
}
