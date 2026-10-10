//! EPUB 2 and 3 metadata: the OPF package document, plus the cover image.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use roxmltree::{Document, Node, ParsingOptions};
use zip::ZipArchive;

use super::{Author, BookMetadata, language_code, parse_date};

pub(crate) type Zip = ZipArchive<BufReader<File>>;
pub(crate) type Res<T> = Result<T, Box<dyn std::error::Error>>;

/// Limits that keep a hostile or broken file from exhausting memory.
pub(crate) const MAX_XML: u64 = 4 << 20;
const MAX_COVER: u64 = 30 << 20;

/// Metadata from an EPUB, or None if it can't be read as one.
pub fn read(path: &Path) -> Option<BookMetadata> {
    read_inner(path).ok()
}

fn read_inner(path: &Path) -> Res<BookMetadata> {
    let mut zip = ZipArchive::new(BufReader::new(File::open(path)?))?;
    let opf_path = opf_path(&mut zip)?;
    let opf = String::from_utf8(read_entry(&mut zip, &opf_path, MAX_XML)?)?;
    let doc = parse_xml(&opf)?;
    let package = doc.root_element();
    let metadata = child(package, "metadata").ok_or("no <metadata>")?;
    let manifest = child(package, "manifest");

    // EPUB 3 attaches details to elements via <meta refines="#id" property="…">.
    let mut refines: HashMap<&str, Vec<(&str, String)>> = HashMap::new();
    for meta in metadata.children().filter(|n| n.has_tag_name_local("meta")) {
        if let (Some(target), Some(property)) = (attr(meta, "refines"), attr(meta, "property")) {
            refines.entry(target.trim_start_matches('#')).or_default().push((property, text(meta)));
        }
    }
    let refined = |node: Node<'_, '_>, property: &str| -> Option<String> {
        let id = attr(node, "id")?;
        refines.get(id)?.iter().find(|(p, _)| *p == property).map(|(_, v)| v.clone())
    };

    let mut out = BookMetadata::default();
    for node in metadata.children().filter(Node::is_element) {
        let value = text(node);
        match node.tag_name().name() {
            "title" if out.title.is_none() && !value.is_empty() => out.title = Some(value),
            "creator" if !value.is_empty() => {
                if is_author(node, metadata) {
                    let sort = attr(node, "file-as").map(str::to_owned).or_else(|| refined(node, "file-as"));
                    out.authors.push(Author { name: value, sort: sort.filter(|s| !s.trim().is_empty()) });
                }
            }
            "publisher" if !value.is_empty() => out.publisher = Some(value),
            "date" if out.published.is_none() => out.published = parse_date(&value),
            "language" if !value.is_empty() => out.languages.push(language_code(&value)),
            "subject" if !value.is_empty() => out.tags.push(value),
            "description" if !value.is_empty() => out.description = Some(value),
            "identifier" => {
                if let Some((scheme, id)) = identifier(attr(node, "scheme"), &value) {
                    out.identifiers.entry(scheme).or_insert(id);
                } else if let Some(uuid) = book_uuid(attr(node, "scheme"), &value) {
                    out.uuids.push(uuid);
                }
            }
            "meta" => match (attr(node, "name"), attr(node, "property")) {
                // calibre's EPUB 2 series convention.
                (Some("calibre:series"), _) => {
                    if let Some(name) = attr(node, "content").filter(|s| !s.trim().is_empty()) {
                        let index = out.series.as_ref().map_or(1.0, |s| s.1);
                        out.series = Some((name.trim().to_owned(), index));
                    }
                }
                (Some("calibre:series_index"), _) => {
                    if let Some(i) = attr(node, "content").and_then(|c| c.trim().parse::<f64>().ok()) {
                        match &mut out.series {
                            Some(s) => s.1 = i,
                            None => out.series = Some((String::new(), i)), // name may follow
                        }
                    }
                }
                // EPUB 3 collections; a series unless typed otherwise.
                (_, Some("belongs-to-collection")) if !value.is_empty() => {
                    let kind = refined(node, "collection-type");
                    if out.series.as_ref().is_none_or(|s| s.0.is_empty())
                        && kind.as_deref().is_none_or(|k| k == "series")
                    {
                        let index = refined(node, "group-position").and_then(|p| p.trim().parse().ok()).unwrap_or(1.0);
                        out.series = Some((value, index));
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }
    // An index without a series name is meaningless.
    out.series = out.series.filter(|s| !s.0.is_empty());

    if let Some(href) = manifest.and_then(|m| cover_href(metadata, m)) {
        let full = resolve(&opf_path, &href);
        if let Ok(bytes) = read_entry(&mut zip, &full, MAX_COVER) {
            out.cover = Some(bytes);
        }
    }
    Ok(out)
}

/// The OPF package document's path inside the zip, from container.xml.
pub(crate) fn opf_path(zip: &mut Zip) -> Res<String> {
    let container = String::from_utf8(read_entry(zip, "META-INF/container.xml", MAX_XML)?)?;
    let container = parse_xml(&container)?;
    Ok(container
        .descendants()
        .find(|n| n.has_tag_name_local("rootfile"))
        .and_then(|n| attr(n, "full-path"))
        .ok_or("no rootfile in container.xml")?
        .to_owned())
}

/// The cover image's href: the declared cover, else an image item with
/// "cover" in its id or file name.
fn cover_href(metadata: Node<'_, '_>, manifest: Node<'_, '_>) -> Option<String> {
    let declared = declared_cover(metadata, manifest);
    let item = declared.or_else(|| {
        manifest.children().filter(|n| n.has_tag_name_local("item")).find(|n| {
            let id = attr(*n, "id").unwrap_or("").to_lowercase();
            let h = attr(*n, "href").unwrap_or("").to_lowercase();
            attr(*n, "media-type").is_some_and(|t| t.starts_with("image/"))
                && (id.contains("cover") || h.contains("cover"))
        })
    })?;
    attr(item, "href").map(str::to_owned)
}

/// The manifest item the package declares as its cover: EPUB 3
/// `properties="cover-image"`, else EPUB 2 `<meta name="cover" content="item-id">`.
pub(crate) fn declared_cover<'a, 'i>(metadata: Node<'a, 'i>, manifest: Node<'a, 'i>) -> Option<Node<'a, 'i>> {
    let mut items = manifest.children().filter(|n| n.has_tag_name_local("item"));
    if let Some(item) =
        items.clone().find(|n| attr(*n, "properties").is_some_and(|p| p.split_whitespace().any(|p| p == "cover-image")))
    {
        return Some(item);
    }
    let cover_id = metadata
        .children()
        .find(|n| n.has_tag_name_local("meta") && attr(*n, "name") == Some("cover"))
        .and_then(|n| attr(n, "content"))?;
    items.find(|n| attr(*n, "id") == Some(cover_id))
}

/// (scheme, value) for identifiers worth keeping: ISBNs and named schemes.
/// Package uuids and calibre's internal ids are skipped. EPUB 3 has no
/// scheme attribute, so there the scheme is a prefix: "google:abc".
pub(crate) fn identifier(scheme: Option<&str>, value: &str) -> Option<(String, String)> {
    let v = value.trim();
    let lower = v.to_ascii_lowercase();
    for prefix in ["urn:isbn:", "isbn:"] {
        if let Some(rest) = lower.strip_prefix(prefix) {
            return Some(("isbn".into(), normalize_isbn(&v[v.len() - rest.len()..])));
        }
    }
    if lower.starts_with("urn:uuid:") || v.is_empty() {
        return None;
    }
    let (scheme, v) = match scheme {
        Some(s) => (s.trim().to_ascii_lowercase(), v),
        None => {
            let (s, rest) = v.split_once(':')?;
            let named = !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
            if !named || matches!(s.to_ascii_lowercase().as_str(), "urn" | "http" | "https") || rest.trim().is_empty() {
                return None;
            }
            (s.to_ascii_lowercase(), rest.trim())
        }
    };
    match scheme.as_str() {
        "" | "uuid" | "calibre" => None,
        "isbn" => Some(("isbn".into(), normalize_isbn(v))),
        _ => Some((scheme, v.to_owned())),
    }
}

/// A uuid identifier ("urn:uuid:…", or scheme uuid/calibre), lower case.
fn book_uuid(scheme: Option<&str>, value: &str) -> Option<String> {
    let v = value.trim().to_ascii_lowercase();
    let v = v.strip_prefix("urn:uuid:").or_else(|| v.strip_prefix("calibre:")).unwrap_or(&v).to_owned();
    let uuidish = v.len() == 36 && v.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-');
    let named = scheme.is_some_and(|s| s.eq_ignore_ascii_case("uuid") || s.eq_ignore_ascii_case("calibre"));
    (uuidish && (named || value.trim().to_ascii_lowercase().starts_with("urn:uuid:") || scheme.is_none())).then_some(v)
}

pub(crate) fn normalize_isbn(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_digit() || *c == 'X' || *c == 'x').collect::<String>().to_uppercase()
}

/// An href inside the OPF, relative to the OPF's folder, as a zip entry name.
pub(crate) fn resolve(opf_path: &str, href: &str) -> String {
    let href = percent_decode(href.split('#').next().unwrap_or(href));
    let base = opf_path.rsplit_once('/').map_or("", |(dir, _)| dir);
    let mut parts: Vec<&str> = if base.is_empty() { Vec::new() } else { base.split('/').collect() };
    for seg in href.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Some(b) = std::str::from_utf8(&bytes[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(b);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub(crate) fn read_entry(zip: &mut Zip, name: &str, limit: u64) -> Res<Vec<u8>> {
    let entry = zip.by_name(name)?;
    if entry.size() > limit {
        return Err(format!("{name} is too large").into());
    }
    let mut buf = Vec::with_capacity(entry.size() as usize);
    entry.take(limit).read_to_end(&mut buf)?;
    Ok(buf)
}

pub(crate) fn parse_xml(s: &str) -> Res<Document<'_>> {
    // Many OPFs carry a DOCTYPE; external entities are never resolved.
    Ok(Document::parse_with_options(
        s.trim_start_matches('\u{feff}'),
        ParsingOptions { allow_dtd: true, ..Default::default() },
    )?)
}

pub(crate) fn child<'a, 'i>(node: Node<'a, 'i>, local: &str) -> Option<Node<'a, 'i>> {
    node.children().find(|n| n.has_tag_name_local(local))
}

/// Whether a `<dc:creator>` is an author: it has no role at all, or any of
/// its roles is "aut". Roles come from `opf:role` and from EPUB 3 role
/// refinements, of which there can be several ("ann" then "aut", say).
pub(crate) fn is_author(creator: Node<'_, '_>, metadata: Node<'_, '_>) -> bool {
    let id = attr(creator, "id");
    let refined = metadata
        .children()
        .filter(|m| {
            m.has_tag_name_local("meta")
                && id.is_some()
                && attr(*m, "refines").map(|r| r.trim_start_matches('#')) == id
                && attr(*m, "property") == Some("role")
        })
        .map(text);
    let mut roles = attr(creator, "role").map(str::to_owned).into_iter().chain(refined).peekable();
    roles.peek().is_none() || roles.any(|r| r.eq_ignore_ascii_case("aut"))
}

/// Attribute by local name, whatever its namespace (opf:role, role, …).
pub(crate) fn attr<'a>(node: Node<'a, '_>, local: &str) -> Option<&'a str> {
    node.attributes().find(|a| a.name() == local).map(|a| a.value())
}

/// All text inside a node, whitespace-collapsed.
pub(crate) fn text(node: Node<'_, '_>) -> String {
    let raw: String = node.descendants().filter(Node::is_text).filter_map(|n| n.text()).collect();
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(crate) trait LocalName {
    fn has_tag_name_local(&self, local: &str) -> bool;
}

impl LocalName for Node<'_, '_> {
    fn has_tag_name_local(&self, local: &str) -> bool {
        self.is_element() && self.tag_name().name() == local
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::io::Write;

    use zip::write::SimpleFileOptions;

    use super::*;

    /// Write a minimal EPUB with the given OPF and optional cover bytes.
    pub(crate) fn make_epub(path: &Path, opf: &str, cover: Option<(&str, &[u8])>) {
        let mut z = zip::ZipWriter::new(File::create(path).unwrap());
        let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        z.start_file("mimetype", opts).unwrap();
        z.write_all(b"application/epub+zip").unwrap();
        z.start_file("META-INF/container.xml", opts).unwrap();
        z.write_all(br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
        z.start_file("OEBPS/content.opf", opts).unwrap();
        z.write_all(opf.as_bytes()).unwrap();
        if let Some((name, bytes)) = cover {
            z.start_file(format!("OEBPS/{name}"), opts).unwrap();
            z.write_all(bytes).unwrap();
        }
        z.finish().unwrap();
    }

    pub(crate) const EPUB2_OPF: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0" unique-identifier="uid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:opf="http://www.idpf.org/2007/opf">
    <dc:title>The Dispossessed</dc:title>
    <dc:creator opf:role="aut" opf:file-as="Le Guin, Ursula K.">Ursula K. Le Guin</dc:creator>
    <dc:creator opf:role="ill">Some Illustrator</dc:creator>
    <dc:publisher>Harper &amp; Row</dc:publisher>
    <dc:date>1974-05-01T00:00:00+00:00</dc:date>
    <dc:language>en</dc:language>
    <dc:identifier id="uid" opf:scheme="uuid">urn:uuid:5c4b5b2c-0000-4000-8000-000000000000</dc:identifier>
    <dc:identifier opf:scheme="ISBN">978-0-06-105488-4</dc:identifier>
    <dc:subject>Science Fiction</dc:subject>
    <dc:subject>Classics</dc:subject>
    <dc:description>&lt;p&gt;An &lt;b&gt;ambiguous&lt;/b&gt; utopia.&lt;/p&gt;</dc:description>
    <meta name="calibre:series" content="Hainish Cycle"/>
    <meta name="calibre:series_index" content="5"/>
    <meta name="cover" content="cover-img"/>
  </metadata>
  <manifest>
    <item id="cover-img" href="images/My%20Cover.jpg" media-type="image/jpeg"/>
  </manifest>
</package>"#;

    #[test]
    fn reads_epub2_metadata_and_cover() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("book.epub");
        make_epub(&path, EPUB2_OPF, Some(("images/My Cover.jpg", b"\xFF\xD8jpeg-bytes")));

        let m = read(&path).unwrap();
        assert_eq!(m.title.as_deref(), Some("The Dispossessed"));
        assert_eq!(m.authors, [Author { name: "Ursula K. Le Guin".into(), sort: Some("Le Guin, Ursula K.".into()) }]);
        assert_eq!(m.publisher.as_deref(), Some("Harper & Row"));
        assert_eq!(m.published, chrono::NaiveDate::from_ymd_opt(1974, 5, 1));
        assert_eq!(m.languages, ["eng"]);
        assert_eq!(m.identifiers.get("isbn").map(String::as_str), Some("9780061054884"));
        assert_eq!(m.identifiers.len(), 1, "package uuid skipped");
        assert_eq!(m.uuids, ["5c4b5b2c-0000-4000-8000-000000000000"]);
        assert_eq!(m.tags, ["Science Fiction", "Classics"]);
        assert_eq!(m.description.as_deref(), Some("<p>An <b>ambiguous</b> utopia.</p>"));
        assert_eq!(m.series, Some(("Hainish Cycle".into(), 5.0)));
        assert_eq!(m.cover.as_deref(), Some(&b"\xFF\xD8jpeg-bytes"[..]));
    }

    #[test]
    fn reads_epub3_refines_and_collections() {
        let opf = r##"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Leviathan Wakes</dc:title>
    <dc:creator id="c1">James S. A. Corey</dc:creator>
    <meta refines="#c1" property="role" scheme="marc:relators">aut</meta>
    <meta refines="#c1" property="file-as">Corey, James S. A.</meta>
    <dc:creator id="c2">A Translator</dc:creator>
    <meta refines="#c2" property="role">trl</meta>
    <meta property="belongs-to-collection" id="col">The Expanse</meta>
    <meta refines="#col" property="collection-type">series</meta>
    <meta refines="#col" property="group-position">1</meta>
    <dc:identifier>urn:isbn:9780316129084</dc:identifier>
  </metadata>
  <manifest>
    <item id="c" href="cover.png" media-type="image/png" properties="cover-image"/>
  </manifest>
</package>"##;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("book.epub");
        make_epub(&path, opf, Some(("cover.png", b"\x89PNG")));

        let m = read(&path).unwrap();
        assert_eq!(m.authors, [Author { name: "James S. A. Corey".into(), sort: Some("Corey, James S. A.".into()) }]);
        assert_eq!(m.series, Some(("The Expanse".into(), 1.0)));
        assert_eq!(m.identifiers.get("isbn").map(String::as_str), Some("9780316129084"));
        assert_eq!(m.cover.as_deref(), Some(&b"\x89PNG"[..]));
    }

    /// Creators with several role refinements, as Standard Ebooks writes
    /// them: an author whose first role isn't "aut", one whose is, an
    /// illustrator, and a creator with no role.
    pub(crate) const MIXED_ROLES_OPF: &str = r##"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="uid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="uid">urn:uuid:22222222-3333-4444-8555-666666666666</dc:identifier>
    <dc:title>A Study in Scarlet</dc:title>
    <dc:language>en</dc:language>
    <dc:creator id="c1">Arthur Conan Doyle</dc:creator>
    <meta refines="#c1" property="role" scheme="marc:relators">ann</meta>
    <meta refines="#c1" property="role" scheme="marc:relators">aut</meta>
    <meta refines="#c1" property="file-as">Doyle, Arthur Conan</meta>
    <dc:creator id="c2">Second Author</dc:creator>
    <meta refines="#c2" property="role" scheme="marc:relators">aut</meta>
    <meta refines="#c2" property="role" scheme="marc:relators">dto</meta>
    <dc:creator id="c3">Only Illustrator</dc:creator>
    <meta refines="#c3" property="role" scheme="marc:relators">ill</meta>
    <dc:creator id="c4">No Role</dc:creator>
  </metadata>
  <manifest><item id="a" href="a.xhtml" media-type="application/xhtml+xml"/></manifest>
</package>"##;

    #[test]
    fn a_creator_is_an_author_if_any_role_is_aut() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("book.epub");
        make_epub(&path, MIXED_ROLES_OPF, None);

        let m = read(&path).unwrap();
        assert_eq!(
            m.authors,
            [
                Author { name: "Arthur Conan Doyle".into(), sort: Some("Doyle, Arthur Conan".into()) },
                Author { name: "Second Author".into(), sort: None },
                Author { name: "No Role".into(), sort: None },
            ]
        );
    }

    #[test]
    fn broken_files_read_as_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("broken.epub");
        std::fs::write(&path, "not a zip").unwrap();
        assert!(read(&path).is_none());
    }

    #[test]
    fn resolves_relative_hrefs() {
        assert_eq!(resolve("OEBPS/content.opf", "images/a%20b.jpg"), "OEBPS/images/a b.jpg");
        assert_eq!(resolve("OEBPS/text/content.opf", "../img/c.jpg#x"), "OEBPS/img/c.jpg");
        assert_eq!(resolve("content.opf", "c.jpg"), "c.jpg");
    }
}
