use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use zip::ZipArchive;

use super::*;
use crate::convert::azw3;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
}

/// An EPUB's files.
fn entries(path: &Path) -> (Vec<String>, HashMap<String, Vec<u8>>) {
    let mut zip = ZipArchive::new(BufReader::new(File::open(path).unwrap())).unwrap();
    let mut names = Vec::new();
    let mut files = HashMap::new();
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).unwrap();
        let mut data = Vec::new();
        f.read_to_end(&mut data).unwrap();
        names.push(f.name().to_owned());
        files.insert(f.name().to_owned(), data);
    }
    (names, files)
}

fn resolve(base: &str, href: &str) -> String {
    let mut parts: Vec<&str> = base.split('/').collect();
    parts.pop();
    for p in href.split('/') {
        match p {
            "." | "" => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    parts.join("/")
}

/// The spine's XHTML files, in order.
fn spine(files: &HashMap<String, Vec<u8>>) -> Vec<String> {
    let opf = String::from_utf8(files["OEBPS/content.opf"].clone()).unwrap();
    let doc = roxmltree::Document::parse(&opf).unwrap();
    let items: HashMap<&str, &str> = doc
        .descendants()
        .filter(|n| n.has_tag_name("item"))
        .map(|n| (n.attribute("id").unwrap(), n.attribute("href").unwrap()))
        .collect();
    doc.descendants()
        .filter(|n| n.has_tag_name("itemref"))
        .map(|n| format!("OEBPS/{}", items[n.attribute("idref").unwrap()]))
        .collect()
}

/// What's wrong with an EPUB, as epubcheck would see it (the parts that
/// matter for opening cleanly): an empty list if nothing.
fn problems(path: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let (names, files) = entries(path);
    if names.first().map(String::as_str) != Some("mimetype") || files["mimetype"] != b"application/epub+zip" {
        out.push("mimetype isn't first".into());
    }
    let opf = String::from_utf8(files["OEBPS/content.opf"].clone()).unwrap();
    let doc = roxmltree::Document::parse(&opf).unwrap();
    let mut manifest: HashMap<String, String> = HashMap::new();
    for item in doc.descendants().filter(|n| n.has_tag_name("item")) {
        let path = format!("OEBPS/{}", item.attribute("href").unwrap());
        if !files.contains_key(&path) {
            out.push(format!("{path} is in the manifest but missing"));
        }
        manifest.insert(path, item.attribute("media-type").unwrap().to_owned());
    }
    for name in &names {
        if !manifest.contains_key(name)
            && !matches!(name.as_str(), "mimetype" | "META-INF/container.xml" | "OEBPS/content.opf")
        {
            out.push(format!("{name} isn't in the manifest"));
        }
    }
    if !doc.descendants().any(|n| n.attribute("properties").is_some_and(|p| p.contains("nav"))) {
        out.push("no nav".into());
    }
    // Ids in each document, then every reference checked against them.
    let mut ids: HashMap<String, HashSet<String>> = HashMap::new();
    let mut docs = Vec::new();
    for (path, media) in &manifest {
        if media != "application/xhtml+xml" {
            continue;
        }
        let text = String::from_utf8(files[path].clone()).unwrap();
        match roxmltree::Document::parse_with_options(
            &text,
            roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() },
        ) {
            Ok(d) => {
                let mut seen = HashSet::new();
                for n in d.descendants().filter(|n| n.is_element()) {
                    if let Some(id) = n.attribute("id")
                        && !seen.insert(id.to_owned())
                    {
                        out.push(format!("{path}: id {id} repeats"));
                    }
                }
                ids.insert(path.clone(), seen);
                docs.push((path.clone(), text));
            }
            Err(e) => out.push(format!("{path} isn't well formed: {e}")),
        }
    }
    for (path, text) in &docs {
        let d = roxmltree::Document::parse_with_options(
            text,
            roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() },
        )
        .unwrap();
        if d.root_element().tag_name().namespace() != Some("http://www.w3.org/1999/xhtml") {
            out.push(format!("{path}: not XHTML"));
        }
        for n in d.descendants().filter(|n| n.is_element()) {
            for a in n.attributes() {
                if !matches!(a.name(), "href" | "src") {
                    continue;
                }
                let v = a.value();
                if v.starts_with("http:") || v.starts_with("https:") || v.starts_with("mailto:") {
                    continue;
                }
                let (file, frag) = v.split_once('#').unwrap_or((v, ""));
                let target = if file.is_empty() { path.clone() } else { resolve(path, file) };
                if !manifest.contains_key(&target) {
                    out.push(format!("{path}: {v} points at a missing file"));
                } else if !frag.is_empty() && !ids.get(&target).is_some_and(|i| i.contains(frag)) {
                    out.push(format!("{path}: {v} points at a missing id"));
                }
            }
            if n.tag_name().name() == "svg" && n.tag_name().namespace() == Some("http://www.w3.org/2000/svg") {
                let item = doc
                    .descendants()
                    .find(|i| i.has_tag_name("item") && format!("OEBPS/{}", i.attribute("href").unwrap()) == *path);
                if !item.and_then(|i| i.attribute("properties")).is_some_and(|p| p.contains("svg")) {
                    out.push(format!("{path}: has SVG but isn't marked so"));
                }
            }
        }
    }
    out.extend(epubcheck(path));
    out
}

/// epubcheck's errors, if it's installed (it isn't required).
fn epubcheck(path: &Path) -> Vec<String> {
    let Ok(out) = std::process::Command::new("epubcheck").arg(path).output() else { return vec![] };
    if out.status.success() {
        return vec![];
    }
    let log = String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    log.lines().filter(|l| l.starts_with("ERROR") || l.starts_with("FATAL")).map(str::to_owned).collect()
}

/// The text of each spine document, blocks apart, whitespace collapsed.
fn texts(files: &HashMap<String, Vec<u8>>, spine: &[String]) -> Vec<String> {
    fn go(n: roxmltree::Node<'_, '_>, out: &mut String) {
        for c in n.children() {
            if c.is_text() {
                out.push_str(c.text().unwrap());
            } else if c.is_element() {
                go(c, out);
                if matches!(c.tag_name().name(), "p" | "div" | "h1" | "h2" | "h3" | "blockquote" | "li" | "br") {
                    out.push(' ');
                }
            }
        }
    }
    spine
        .iter()
        .map(|p| {
            let text = String::from_utf8(files[p].clone()).unwrap().replace("&nbsp;", "&#160;");
            let d = roxmltree::Document::parse_with_options(
                &text,
                roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() },
            )
            .unwrap();
            let mut out = String::new();
            go(d.descendants().find(|n| n.has_tag_name("body")).unwrap(), &mut out);
            out.split_whitespace().collect::<Vec<_>>().join(" ")
        })
        .collect()
}

/// The nav's table of contents: (depth, label, href).
fn nav_toc(files: &HashMap<String, Vec<u8>>) -> Vec<(usize, String, String)> {
    let nav = String::from_utf8(files["OEBPS/nav.xhtml"].clone()).unwrap();
    let d = roxmltree::Document::parse_with_options(
        &nav,
        roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() },
    )
    .unwrap();
    let toc = d.descendants().find(|n| n.has_tag_name("nav") && n.attributes().any(|a| a.value() == "toc")).unwrap();
    toc.descendants()
        .filter(|n| n.has_tag_name("a"))
        .map(|a| {
            let depth = a.ancestors().filter(|n| n.has_tag_name("ol")).count();
            (depth, a.text().unwrap_or_default().to_owned(), a.attribute("href").unwrap().to_owned())
        })
        .collect()
}

#[test]
fn round_trips_through_azw3() {
    let dir = tempfile::tempdir().unwrap();
    let (epub, azw3, back) = (dir.path().join("a.epub"), dir.path().join("a.azw3"), dir.path().join("b.epub"));
    azw3::tests::sample_epub(&epub);
    azw3::convert(&epub, &azw3).unwrap();
    let report = convert(&azw3, &back).unwrap();
    assert_eq!(report.source, "KF8");
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert_eq!(problems(&back), Vec::<String>::new());

    let (_, orig) = entries(&epub);
    let (_, files) = entries(&back);
    // Structure: the same two chapters, in order, with the same text.
    let orig_spine = vec!["OEBPS/text/ch1.xhtml".to_owned(), "OEBPS/text/ch2.xhtml".to_owned()];
    let new_spine = spine(&files);
    assert_eq!(new_spine.len(), 2, "{new_spine:?}");
    assert_eq!(texts(&files, &new_spine), texts(&orig, &orig_spine));

    // Metadata comes along.
    let meta = crate::metadata::epub::read(&back).unwrap();
    assert_eq!(meta.title.as_deref(), Some("Sample Book"));
    assert_eq!(meta.authors[0].name, "Ann Author");
    assert_eq!(meta.languages, ["eng"]);
    assert!(meta.cover.as_deref().is_some_and(|c| *c == orig["OEBPS/images/cover.png"][..]), "the cover, unchanged");

    // The image, its references, the stylesheet's and the SVG's.
    let ch1 = String::from_utf8(files[&new_spine[0]].clone()).unwrap();
    let ch2 = String::from_utf8(files[&new_spine[1]].clone()).unwrap();
    let image = files.keys().find(|k| k.starts_with("OEBPS/images/") && k.ends_with(".png")).unwrap().clone();
    let image_href = format!("../{}", image.trim_start_matches("OEBPS/"));
    assert!(ch2.contains(&format!(r#"<img src="{image_href}" alt=""/>"#)), "{ch2}");
    assert!(ch1.contains(&format!(r#"<image width="60" height="90" xlink:href="{image_href}"/>"#)), "{ch1}");
    let css: Vec<(String, String)> = files
        .iter()
        .filter(|(k, _)| k.ends_with(".css"))
        .map(|(k, v)| (k.trim_start_matches("OEBPS/").to_owned(), String::from_utf8(v.clone()).unwrap()))
        .collect();
    let sheet = css.iter().find(|(_, c)| c.contains("p { margin: 0; }")).unwrap();
    assert!(sheet.1.contains(&format!("url({image_href})")), "{css:?}");
    // The inline <style> became a stylesheet of its own; both are linked.
    let inline = css.iter().find(|(_, c)| c.contains("h1 { color: red; }")).unwrap();
    for (path, _) in [sheet, inline] {
        assert!(ch1.contains(&format!(r#"<link rel="stylesheet" type="text/css" href="../{path}"/>"#)), "{ch1}");
    }

    // Links between the chapters, both ways.
    let file = |p: &str| p.trim_start_matches("OEBPS/text/").to_owned();
    assert!(ch1.contains(&format!(r##"<a href="{}#end">Skip to the end</a>"##, file(&new_spine[1]))), "{ch1}");
    assert!(ch2.contains(&format!(r##"<a href="{}#middle">Back</a>"##, file(&new_spine[0]))), "{ch2}");

    // The table of contents, nested as it was. Entries for a chapter's start
    // may point at its first tagged element (as the Kindle file does).
    let toc: Vec<_> =
        nav_toc(&files).into_iter().map(|(d, l, h)| (d, l, h.split("#aid-").next().unwrap().to_owned())).collect();
    assert_eq!(
        toc,
        [
            (1, "Chapter One".to_owned(), format!("text/{}", file(&new_spine[0]))),
            (2, "The Middle".to_owned(), format!("text/{}#middle", file(&new_spine[0]))),
            (1, "Chapter Two".to_owned(), format!("text/{}", file(&new_spine[1]))),
        ]
    );
}

#[test]
fn converts_calibres_kf8_and_mobi6() {
    for (name, source) in [("test.azw3", "KF8"), ("test.mobi", "MOBI 6")] {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out.epub");
        let report = convert(&fixture(name), &out).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(report.source, source, "{name}");
        assert_eq!(problems(&out), Vec::<String>::new(), "{name}");
        let meta = crate::metadata::epub::read(&out).unwrap();
        assert_eq!(meta.title.as_deref(), Some("Test Book: A Story"), "{name}");
        let (_, files) = entries(&out);
        let text = texts(&files, &spine(&files)).join(" ");
        assert!(!text.trim().is_empty(), "{name}");
    }
}

/// A MOBI 6 file as kindlegen writes them: one stream of HTML with
/// `filepos` links, page breaks, a guide and `recindex` images.
fn mobi6_fixture(image: &[u8]) -> Vec<u8> {
    let markup = r#"<html><head><guide><reference title="Table of Contents" type="toc" {F:toc} /><reference title="Start" type="text" {F:one} /></guide></head><body><p height="1em" width="0pt" align="center"><font size="7"><b>The Sample</b></font></p><p align="center"><img recindex="00001" /></p><mbp:pagebreak/>{T:toc}<h2>Contents</h2><blockquote><a {F:one}>Chapter One</a></blockquote><blockquote><a {F:two}>Chapter Two</a></blockquote><mbp:pagebreak/>{T:one}<h1 align="center">Chapter One</h1><p width="2em">It was a bright cold day in April&#8212;the clocks were striking thirteen.</p><p width="-1em">Hanging. See <a {F:end}>the end</a>.</p><div height="2em"></div><p><i>caf&eacute;</i></p><mbp:pagebreak/>{T:two}<h1>Chapter Two</h1><p>Short.</p>{T:end}<p>The end.</p></body></html>"#;
    // Targets ({T:name}) become offsets that links ({F:name}) point to.
    let mut text = String::new();
    let mut targets = HashMap::new();
    let mut links = Vec::new();
    let mut rest = markup;
    while let Some(i) = rest.find('{') {
        text.push_str(&rest[..i]);
        let j = rest[i..].find('}').unwrap() + i;
        let (kind, name) = rest[i + 1..j].split_once(':').unwrap();
        if kind == "T" {
            targets.insert(name.to_owned(), text.len());
        } else {
            text.push_str("filepos=");
            links.push((text.len(), name.to_owned()));
            text.push_str("0000000000");
        }
        rest = &rest[j + 1..];
    }
    text.push_str(rest);
    let mut bytes = text.into_bytes();
    for (at, name) in links {
        bytes[at..at + 10].copy_from_slice(format!("{:010}", targets[&name]).as_bytes());
    }

    // Text records (PalmDOC, each with a multibyte-overlap byte), then the image.
    let mut records: Vec<Vec<u8>> = vec![Vec::new()];
    for chunk in bytes.chunks(4096) {
        let mut r = azw3::palmdoc::compress(chunk);
        r.push(0);
        records.push(r);
    }
    let first_image = records.len() as u32;
    records.push(image.to_vec());
    records.push(
        b"FLIS\0\0\0\x08\0\x41\0\0\0\0\0\0\xff\xff\xff\xff\0\x01\0\x03\0\0\0\x03\0\0\0\x01\xff\xff\xff\xff".to_vec(),
    );
    records.push(b"\xe9\x8e\r\n".to_vec());

    let mut exth_recs: Vec<(u32, Vec<u8>)> = vec![
        (100, b"Sam Sample".to_vec()),
        (503, b"The Sample".to_vec()),
        (524, b"en".to_vec()),
        (201, 0u32.to_be_bytes().to_vec()),
    ];
    let mut exth = Vec::new();
    for (k, v) in exth_recs.drain(..) {
        exth.extend_from_slice(&k.to_be_bytes());
        exth.extend_from_slice(&(v.len() as u32 + 8).to_be_bytes());
        exth.extend_from_slice(&v);
    }
    let mut exth_block = b"EXTH".to_vec();
    exth_block.extend_from_slice(&(exth.len() as u32 + 12).to_be_bytes());
    exth_block.extend_from_slice(&4u32.to_be_bytes());
    exth_block.extend_from_slice(&exth);
    while !exth_block.len().is_multiple_of(4) {
        exth_block.push(0);
    }

    let mut r0 = vec![0u8; 16 + 232];
    let put16 = |r: &mut Vec<u8>, at: usize, v: u16| r[at..at + 2].copy_from_slice(&v.to_be_bytes());
    let put32 = |r: &mut Vec<u8>, at: usize, v: u32| r[at..at + 4].copy_from_slice(&v.to_be_bytes());
    put16(&mut r0, 0, 2);
    put32(&mut r0, 4, bytes.len() as u32);
    put16(&mut r0, 8, (records.len() - 4) as u16);
    put16(&mut r0, 10, 4096);
    r0[16..20].copy_from_slice(b"MOBI");
    put32(&mut r0, 20, 232);
    put32(&mut r0, 24, 2);
    put32(&mut r0, 28, 65001);
    put32(&mut r0, 36, 6);
    for at in (40..80).step_by(4) {
        put32(&mut r0, at, 0xFFFF_FFFF);
    }
    put32(&mut r0, 0x50, first_image);
    put32(&mut r0, 0x5C, 9);
    put32(&mut r0, 0x68, 6);
    put32(&mut r0, 0x6C, first_image);
    put32(&mut r0, 0x80, 0x40);
    put32(&mut r0, 0xA4, 0xFFFF_FFFF);
    put32(&mut r0, 0xF0, 1); // extra data: multibyte overlap
    put32(&mut r0, 0xF4, 0xFFFF_FFFF);
    r0.extend_from_slice(&exth_block);
    let title_at = r0.len() as u32;
    r0.extend_from_slice(b"The Sample");
    r0.extend_from_slice(&[0; 2]);
    put32(&mut r0, 0x54, title_at);
    put32(&mut r0, 0x58, 10);
    records[0] = r0;

    let mut out = b"The_Sample".to_vec();
    out.resize(32, 0);
    out.extend_from_slice(&[0; 28]);
    out.extend_from_slice(b"BOOKMOBI");
    out.extend_from_slice(&[0; 8]);
    out.extend_from_slice(&(records.len() as u16).to_be_bytes());
    let mut offset = 78 + 8 * records.len() + 2;
    for (i, r) in records.iter().enumerate() {
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(2 * i as u32).to_be_bytes());
        offset += r.len();
    }
    out.extend_from_slice(&[0, 0]);
    for r in &records {
        out.extend_from_slice(r);
    }
    out
}

fn small_jpeg() -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    image::RgbImage::from_pixel(40, 60, image::Rgb([20, 90, 160]))
        .write_to(&mut out, image::ImageFormat::Jpeg)
        .unwrap();
    out.into_inner()
}

#[test]
fn converts_mobi6() {
    let dir = tempfile::tempdir().unwrap();
    let (src, out) = (dir.path().join("sample.mobi"), dir.path().join("sample.epub"));
    let jpeg = small_jpeg();
    std::fs::write(&src, mobi6_fixture(&jpeg)).unwrap();
    let report = convert(&src, &out).unwrap();
    assert_eq!(report.source, "MOBI 6");
    assert_eq!(problems(&out), Vec::<String>::new());

    let (_, files) = entries(&out);
    let spine = spine(&files);
    // Split at the three page breaks.
    assert_eq!(spine.len(), 4, "{spine:?}");
    let t = texts(&files, &spine);
    assert_eq!(t[0], "The Sample");
    assert_eq!(t[1], "Contents Chapter One Chapter Two");
    assert!(t[2].starts_with("Chapter One It was a bright cold day in April—the clocks"), "{}", t[2]);
    assert!(t[2].ends_with("café"), "{}", t[2]);
    assert_eq!(t[3], "Chapter Two Short. The end.");

    // The image, and the cover.
    assert_eq!(files["OEBPS/images/00001.jpg"], jpeg);
    let title = String::from_utf8(files[&spine[0]].clone()).unwrap();
    assert!(title.contains(r#"<img src="../images/00001.jpg" alt=""/>"#), "{title}");
    let opf = String::from_utf8(files["OEBPS/content.opf"].clone()).unwrap();
    assert!(opf.contains(r#"href="images/00001.jpg" media-type="image/jpeg" properties="cover-image""#), "{opf}");

    // Presentational attributes as CSS.
    let ch1 = String::from_utf8(files[&spine[2]].clone()).unwrap();
    assert!(ch1.contains(r#"style="text-align: center">Chapter One</h1>"#), "{ch1}");
    assert!(ch1.contains(r#"<p style="text-indent: 2em">"#), "{ch1}");
    assert!(ch1.contains(r#"style="text-indent: -1em; margin-left: 1em""#), "{ch1}");
    assert!(ch1.contains(r#"<div style="height: 2em">"#), "{ch1}");
    assert!(ch1.contains(r#"<link rel="stylesheet" type="text/css" href="../styles/style.css"/>"#), "{ch1}");

    // filepos links: to the start of chapter two's file, then to an anchor in it.
    let ch2_file = spine[3].trim_start_matches("OEBPS/text/");
    let end_link = ch1.split("<a href=\"").nth(1).unwrap().split('"').next().unwrap();
    assert!(end_link.starts_with(&format!("{ch2_file}#filepos")), "{ch1}");

    // The table of contents from the contents page the guide points to.
    let toc = nav_toc(&files);
    let labels: Vec<&str> = toc.iter().map(|t| t.1.as_str()).collect();
    assert_eq!(labels, ["Chapter One", "Chapter Two"]);
    assert!(toc[0].2.starts_with(&format!("text/{}", spine[2].trim_start_matches("OEBPS/text/"))), "{toc:?}");
    assert!(toc[1].2.starts_with(&format!("text/{ch2_file}")), "{toc:?}");
    let meta = crate::metadata::epub::read(&out).unwrap();
    assert_eq!(meta.title.as_deref(), Some("The Sample"));
    assert_eq!(meta.authors[0].name, "Sam Sample");
}

#[test]
fn refuses_drm_and_what_isnt_mobi() {
    let dir = tempfile::tempdir().unwrap();
    let mut book = mobi6_fixture(&small_jpeg());
    // Record 0's encryption field.
    let r0 = u32::from_be_bytes(book[78..82].try_into().unwrap()) as usize;
    book[r0 + 12..r0 + 14].copy_from_slice(&2u16.to_be_bytes());
    let src = dir.path().join("drm.azw");
    std::fs::write(&src, &book).unwrap();
    let err = convert(&src, &dir.path().join("drm.epub")).unwrap_err().to_string();
    assert!(err.contains("DRM"), "{err}");
    assert!(!dir.path().join("drm.epub").exists());

    for (name, data) in [("kfx.azw", &b"\xeaDRMION\xee and more"[..]), ("x.mobi", b"not a book at all")] {
        let p = dir.path().join(name);
        std::fs::write(&p, data).unwrap();
        assert!(convert(&p, &dir.path().join("x.epub")).is_err(), "{name}");
        assert!(!dir.path().join("x.epub").exists());
    }
}

/// Converts every MOBI/AZW/AZW3 under the folders in MAGNITIDE_MOBI_CORPUS
/// (separated like PATH) and checks each EPUB:
/// `MAGNITIDE_MOBI_CORPUS=~/Books cargo test -p magnitide-core mobi_corpus -- --ignored --nocapture`
#[test]
#[ignore = "needs books: set MAGNITIDE_MOBI_CORPUS"]
fn mobi_corpus() {
    let Some(dirs) = std::env::var_os("MAGNITIDE_MOBI_CORPUS") else { return };
    let keep = std::env::var_os("MAGNITIDE_MOBI_KEEP").map(PathBuf::from);
    let mut books = Vec::new();
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p
                .extension()
                .is_some_and(|x| matches!(x.to_ascii_lowercase().to_str(), Some("mobi" | "azw" | "azw3" | "prc")))
                && !p.file_name().unwrap().to_string_lossy().starts_with("._")
            {
                out.push(p);
            }
        }
    }
    for d in std::env::split_paths(&dirs) {
        walk(&d, &mut books);
    }
    let dir = tempfile::tempdir().unwrap();
    let mut failed = 0;
    for (i, b) in books.iter().enumerate() {
        let out = keep.as_ref().map_or_else(|| dir.path().join(format!("{i}.epub")), |k| k.join(format!("{i}.epub")));
        let _ = std::fs::remove_file(&out);
        let start = std::time::Instant::now();
        match convert(b, &out) {
            Ok(r) => {
                let p = problems(&out);
                println!(
                    "{} {:>6}ms {:7} {} warnings, {} problems  {}",
                    if p.is_empty() { "ok  " } else { "BAD " },
                    start.elapsed().as_millis(),
                    r.source,
                    r.warnings.len(),
                    p.len(),
                    b.display()
                );
                for w in r.warnings.iter().take(3) {
                    println!("       warning: {w}");
                }
                for x in p.iter().take(5) {
                    println!("       problem: {x}");
                }
                failed += usize::from(!p.is_empty());
            }
            Err(e) => {
                println!("FAIL {e}  {}", b.display());
                failed += 1;
            }
        }
    }
    println!("{} books, {failed} with problems", books.len());
}
