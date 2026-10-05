use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use magnitide_catalog::{Book, Catalog, Format};

use super::*;

struct Lib {
    root: PathBuf,
    catalog: Catalog,
}

impl Served for Lib {
    fn name(&self) -> &str {
        "Test Library"
    }
    fn root(&self) -> &Path {
        &self.root
    }
    fn catalog(&self) -> &Catalog {
        &self.catalog
    }
}

struct Fixed(Option<Arc<dyn Served>>);

impl Source for Fixed {
    fn current(&self) -> Option<Arc<dyn Served>> {
        self.0.clone()
    }
}

fn book(id: i64, title: &str, author: &str) -> Book {
    Book {
        id,
        title: title.into(),
        sort: None,
        authors: vec![author.into()],
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
        last_modified: None,
        path: format!("{author}/{title} ({id})"),
        uuid: None,
        has_cover: false,
        formats: vec![],
        custom: BTreeMap::new(),
    }
}

fn write_epub(path: &Path) {
    let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    zip.start_file("mimetype", stored).unwrap();
    zip.write_all(b"application/epub+zip").unwrap();
    let files = [
        (
            "META-INF/container.xml",
            r#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#,
        ),
        (
            "OEBPS/content.opf",
            r#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">x</dc:identifier><dc:title>Dune</dc:title><dc:language>en</dc:language></metadata><manifest><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/></spine></package>"#,
        ),
        (
            "OEBPS/c1.xhtml",
            r#"<?xml version="1.0" encoding="utf-8"?><html xmlns="http://www.w3.org/1999/xhtml"><head><title>1</title></head><body><p>A beginning is a very delicate time.</p></body></html>"#,
        ),
    ];
    for (name, text) in files {
        zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(text.as_bytes()).unwrap();
    }
    zip.finish().unwrap();
}

/// A library of 30 books: "Dune" (EPUB, cover, on a shelf, in a series) and
/// 29 PDFs by one author.
fn fixture() -> (tempfile::TempDir, Arc<dyn Source>) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("lib");
    let mut dune = book(1, "Dune", "Frank Herbert");
    dune.series = Some("Dune".into());
    dune.shelves = vec!["To Read".into()];
    dune.comments = Some("<p>Spice <b>&amp;</b> sand</p><script>alert(1)</script>".into());
    dune.has_cover = true;
    dune.formats = vec![Format { format: "EPUB".into(), size: 0, name: "Dune - Frank Herbert".into() }];
    let book_dir = dune.dir(&root);
    std::fs::create_dir_all(&book_dir).unwrap();
    write_epub(&dune.format_path(&root, &dune.formats[0]));
    let cover = image::RgbImage::from_pixel(600, 900, image::Rgb([200, 30, 30]));
    cover.save(book_dir.join("cover.jpg")).unwrap();

    let mut books = vec![dune];
    for i in 2..=30 {
        let mut b = book(i, &format!("Manual {i:02}"), "Ann <Admin>");
        b.formats = vec![Format { format: "PDF".into(), size: 4, name: "m".into() }];
        let path = b.format_path(&root, &b.formats[0]);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"%PDF").unwrap();
        books.push(b);
    }
    let lib = Lib { root, catalog: Catalog::new(None, vec![], books, vec![]) };
    (dir, Arc::new(Fixed(Some(Arc::new(lib)))))
}

fn get(source: &Arc<dyn Source>, cache: &Path, target: &str) -> Reply {
    handle(&Request::new(target, "Mozilla/5.0"), source.as_ref(), cache)
}

#[test]
fn web_pages() {
    let (dir, source) = fixture();
    let cache = dir.path().join("cache");

    let home = get(&source, &cache, "/");
    assert_eq!(home.status, 200);
    assert!(home.content_type.starts_with("text/html"));
    assert!(home.text().contains("Test Library") && home.text().contains("action=\"/search\""));
    assert!(!home.text().contains("<script"), "no scripts on the page");

    // 30 books, 25 a page; titles sort "Dune" first.
    let all = get(&source, &cache, "/books").text();
    assert!(all.contains("href=\"/book/1\"") && all.contains("/books?page=2"));
    assert!(all.contains("Ann &lt;Admin&gt;"), "escaped");
    assert!(all.contains("href=\"/download/1/EPUB\""), "EPUB first for a non-Kobo browser");
    let second = get(&source, &cache, "/books?page=2").text();
    assert!(second.contains("Manual 30") && second.contains("/books?page=1") && !second.contains("page=3"));

    let search = get(&source, &cache, "/search?q=herbert").text();
    assert!(search.contains("Dune") && !search.contains("Manual"));
    assert_eq!(get(&source, &cache, "/search?q=bogus:x").status, 400);

    let shelves = get(&source, &cache, "/shelves").text();
    assert!(shelves.contains("href=\"/shelf?name=To%20Read\""));
    let shelf = get(&source, &cache, "/shelf?name=To%20Read").text();
    assert!(shelf.contains("Dune") && !shelf.contains("Manual"));
    assert!(get(&source, &cache, "/series?name=Dune").text().contains("/book/1"));
    assert!(get(&source, &cache, "/author?name=Frank+Herbert").text().contains("/book/1"));
    assert!(get(&source, &cache, "/authors").text().contains("Ann &lt;Admin&gt;"));

    let page = get(&source, &cache, "/book/1").text();
    assert!(page.contains("<b>&amp;</b>") && !page.contains("alert"), "description sanitized");
    assert!(page.contains("/download/1/KEPUB"), "a KEPUB is offered from the EPUB");
    assert!(get(&source, &cache, "/book/2").text().contains("No description available"));

    assert_eq!(get(&source, &cache, "/book/99").status, 404);
    assert_eq!(get(&source, &cache, "/nope").status, 404);
}

#[test]
fn kobo_gets_kepub() {
    let (dir, source) = fixture();
    let cache = dir.path().join("cache");
    let kobo = Request::new(
        "/books",
        "Mozilla/5.0 (Linux; U; Android 2.0; en-us;) AppleWebKit/538.1 (KHTML, like Gecko) Version/4.0 Mobile Safari/538.1 (Kobo Touch 0377/4.38.23171)",
    );
    assert!(handle(&kobo, source.as_ref(), &cache).text().contains("href=\"/download/1/KEPUB\""));

    let reply = get(&source, &cache, "/download/1/kepub");
    assert_eq!(reply.status, 200, "{}", reply.text());
    let Body::File(path) = &reply.body else { panic!("a file") };
    assert!(path.starts_with(&cache) && path.to_string_lossy().ends_with(".kepub.epub"));
    let disposition = reply.headers.iter().find(|(k, _)| k == "Content-Disposition").unwrap();
    assert!(disposition.1.contains("Dune - Frank Herbert.kepub.epub"));
    // Kobo spans went in, and a second request reuses the conversion.
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let mut text = String::new();
    zip.by_name("OEBPS/c1.xhtml").unwrap().read_to_string(&mut text).unwrap();
    assert!(text.contains("koboSpan"));
    let again = get(&source, &cache, "/download/1/KEPUB");
    assert!(matches!(&again.body, Body::File(p) if p == path));

    let pdf = get(&source, &cache, "/download/2/PDF");
    assert_eq!(pdf.content_type, "application/pdf");
    assert_eq!(get(&source, &cache, "/download/2/KEPUB").status, 404, "no EPUB to make one from");
    assert_eq!(get(&source, &cache, "/download/2/../../etc").status, 404);
}

#[test]
fn covers_and_thumbnails() {
    let (dir, source) = fixture();
    let cache = dir.path().join("cache");
    let thumb = get(&source, &cache, "/thumb/1");
    assert_eq!(thumb.status, 200);
    let Body::File(path) = &thumb.body else { panic!("a file") };
    assert!(path.starts_with(&cache));
    let (w, h) = image::image_dimensions(path).unwrap();
    assert!(w <= 200 && h <= 300, "{w}x{h}");
    assert!(matches!(get(&source, &cache, "/cover/1").body, Body::File(p) if !p.starts_with(&cache)));
    assert_eq!(get(&source, &cache, "/cover/2").status, 404);
}

#[test]
fn opds_feeds() {
    let (dir, source) = fixture();
    let cache = dir.path().join("cache");

    let root = get(&source, &cache, "/opds");
    assert!(root.content_type.contains("kind=navigation"));
    let root = root.text();
    roxmltree::Document::parse(&root).unwrap();
    assert!(root.contains("href=\"/opds/books\"") && root.contains("href=\"/opds/search.xml\""));

    let books = get(&source, &cache, "/opds/books");
    assert!(books.content_type.contains("kind=acquisition"));
    let books = books.text();
    let doc = roxmltree::Document::parse(&books).unwrap();
    let entries: Vec<_> = doc.descendants().filter(|n| n.has_tag_name("entry")).collect();
    assert_eq!(entries.len(), 25);
    assert!(books.contains("rel=\"next\" href=\"/opds/books?page=2\""));
    let dune = entries[0];
    let links: Vec<(&str, &str)> = dune
        .children()
        .filter(|n| n.has_tag_name("link"))
        .map(|n| (n.attribute("rel").unwrap(), n.attribute("href").unwrap()))
        .collect();
    assert!(links.contains(&("http://opds-spec.org/acquisition", "/download/1/EPUB")));
    assert!(links.contains(&("http://opds-spec.org/image/thumbnail", "/thumb/1")));
    let summary = dune.children().find(|n| n.has_tag_name("summary")).unwrap().text().unwrap();
    assert_eq!(summary, "Dune #1 — Spice & sand");

    let shelves = get(&source, &cache, "/opds/shelves").text();
    roxmltree::Document::parse(&shelves).unwrap();
    assert!(shelves.contains("href=\"/opds/shelf?name=To%20Read\""));
    roxmltree::Document::parse(&get(&source, &cache, "/opds/authors").text()).unwrap();
    let search = get(&source, &cache, "/opds/search?q=dune").text();
    assert!(search.contains("<title>Dune</title>") && !search.contains("Manual"));
    let open_search = get(&source, &cache, "/opds/search.xml").text();
    assert!(open_search.contains("template=\"/opds/search?q={searchTerms}\""));
    assert_eq!(get(&source, &cache, "/opds/book/1").status, 404, "books are entries, not pages, in OPDS");
}

#[test]
fn no_library() {
    let source: Arc<dyn Source> = Arc::new(Fixed(None));
    assert_eq!(get(&source, Path::new("/nonexistent"), "/").status, 503);
}

#[test]
fn passwords() {
    use base64::Engine;
    let basic = |s: &str| format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(s));
    assert!(authorized(None, None));
    assert!(authorized(Some(""), None), "an empty password is none");
    assert!(!authorized(Some("hunter2"), None));
    assert!(authorized(Some("hunter2"), Some(&basic("kobo:hunter2"))));
    assert!(authorized(Some("hunter2"), Some(&basic(":hunter2"))));
    assert!(!authorized(Some("hunter2"), Some(&basic("kobo:hunter3"))));
    assert!(!authorized(Some("hunter2"), Some(&basic("hunter2"))));
    assert!(!authorized(Some("hunter2"), Some("Bearer hunter2")));
}

fn http(port: u16, request: &str) -> String {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.write_all(request.as_bytes()).unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    out
}

#[test]
fn over_a_socket() {
    let (dir, source) = fixture();
    let config = Config { port: 0, password: Some("pw".into()), cache_dir: dir.path().join("cache") };
    let server = Server::start(config, source).unwrap();
    let port = server.port();
    assert_ne!(port, 0);

    let denied = http(port, "GET / HTTP/1.0\r\n\r\n");
    assert!(denied.starts_with("HTTP/1.1 401") || denied.starts_with("HTTP/1.0 401"), "{denied}");
    assert!(denied.contains("WWW-Authenticate: Basic"));

    let ok = http(port, "GET /opds HTTP/1.0\r\nAuthorization: Basic OnB3\r\n\r\n");
    assert!(ok.contains(" 200 ") && ok.contains("X-Content-Type-Options: nosniff") && ok.contains("<feed"), "{ok}");

    let post = http(port, "POST / HTTP/1.0\r\nAuthorization: Basic OnB3\r\nContent-Length: 0\r\n\r\n");
    assert!(post.contains(" 405 "), "{post}");

    let file = http(port, "GET /download/2/PDF HTTP/1.0\r\nAuthorization: Basic OnB3\r\n\r\n");
    assert!(file.contains("Content-Type: application/pdf") && file.ends_with("%PDF"), "{file}");
    server.stop();
    assert!(TcpStream::connect(("127.0.0.1", port)).is_err(), "stopped");
}
