//! The fixture plugin (tests/fixtures/test-source, a Rust component built
//! here for wasm32-wasip2) installed, asked, and made to misbehave.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use magnitide_plugins_wasm::{Capability, Kind, WasmPlugins};
use magnitide_sources::Lookup;

/// Build the fixture component once per test binary (cargo caches it).
fn fixture_component() -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let target = root.join("../../target/wasm-fixtures");
    let status = std::process::Command::new(env!("CARGO"))
        .args(["build", "--release", "--target", "wasm32-wasip2", "--manifest-path"])
        .arg(root.join("tests/fixtures/test-source/Cargo.toml"))
        .arg("--target-dir")
        .arg(&target)
        .status()
        .expect("cargo runs");
    assert!(status.success(), "the fixture plugin builds (rustup target add wasm32-wasip2)");
    target.join("wasm32-wasip2/release/magnitide_test_source.wasm")
}

/// The fixture as a `.mgp`, with the capabilities given.
fn mgp(dir: &Path, name: &str, capabilities: &str) -> PathBuf {
    let path = dir.join(format!("{name}.mgp"));
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    let opts = zip::write::SimpleFileOptions::default();
    zip.start_file("manifest.toml", opts).unwrap();
    write!(
        zip,
        "name = \"{name}\"\nversion = \"1.2.3\"\nauthor = \"Magnitide\"\ndescription = \"Answers from memory\"\napi = \"0.1\"\nkind = \"metadata-source\"\ncapabilities = {capabilities}\ncomponent = \"plugin.wasm\"\n"
    )
    .unwrap();
    zip.start_file("plugin.wasm", opts).unwrap();
    zip.write_all(&std::fs::read(fixture_component()).unwrap()).unwrap();
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
