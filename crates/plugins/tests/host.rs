//! The plugin host against a real calibre, with the test metadata source in
//! tests/fixtures. Skipped (with a note) where calibre isn't installed, as
//! on CI; set MAGNITIDE_CALIBRE_DEBUG to point at a calibre-debug elsewhere.

use std::io::Write;
use std::path::{Path, PathBuf};

use magnitide_plugins::sources::is_plugin_cover;
use magnitide_plugins::{PluginKind, Plugins};
use magnitide_sources::Lookup;

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

/// The fixture plugin as calibre expects it: a zip of the folder's files.
fn test_plugin_zip(dir: &Path) -> PathBuf {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/test_source");
    let path = dir.join("Test Source.zip");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    for name in ["__init__.py", "plugin-import-name-magnitide_test_source.txt"] {
        zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(&std::fs::read(src.join(name)).unwrap()).unwrap();
    }
    zip.finish().unwrap();
    path
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
