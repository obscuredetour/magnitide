//! Writing what a MOBI held as an EPUB 3 (with an NCX for older readers):
//! files split where the book had page breaks or ran long, links pointed at
//! where their targets ended up, the cover, table of contents and landmarks.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use zip::CompressionMethod;
use zip::write::SimpleFileOptions;

use super::html::{self, El, Node, Ns, escape_text};
use crate::metadata::epub::Res;
use crate::metadata::{BookMetadata, language_tag};

/// Files much bigger than this are cut up (some readers struggle with them).
const SPLIT_SIZE: usize = 256 * 1024;

/// One XHTML file as the book had it, named by `origin`; links between
/// files use these names ("part0001.xhtml#id").
pub(super) struct Page {
    pub origin: String,
    pub root: El,
}

/// An image, font, stylesheet or SVG, at `path` from the book's folder.
pub(super) struct Resource {
    pub path: String,
    pub media_type: &'static str,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct TocNode {
    pub label: String,
    /// "origin#id"
    pub href: String,
    pub children: Vec<TocNode>,
}

pub(super) struct Book {
    pub pages: Vec<Page>,
    pub resources: Vec<Resource>,
    pub toc: Vec<TocNode>,
    /// (type, title, href) as in an EPUB 2 guide.
    pub guide: Vec<(String, String, String)>,
    /// A resource path.
    pub cover: Option<String>,
    pub rtl: bool,
}

/// A file in the EPUB: its name in `text/`, the page it came from, and its tree.
struct Out {
    name: String,
    origin: String,
    root: El,
}

/// Write `book` as an EPUB at `dest` (which must not exist).
pub(super) fn write(book: Book, meta: &BookMetadata, dest: &Path) -> Res<()> {
    let title = meta.title.clone().filter(|t| !t.trim().is_empty()).unwrap_or_else(|| "Unknown".into());
    let resources: HashMap<&str, &Resource> = book.resources.iter().map(|r| (r.path.as_str(), r)).collect();

    // Tidy each page and cut it where it breaks or runs long.
    let mut outs: Vec<Out> = Vec::new();
    for mut page in book.pages {
        html::tidy(&mut page.root, &title);
        let mut root = page.root;
        let body_at = root.children.iter().position(|c| matches!(c, Node::El(e) if e.is("body"))).expect("tidied");
        let Node::El(mut body) = root.children.remove(body_at) else { unreachable!() };
        html::mark_long(&mut body, SPLIT_SIZE);
        for piece in html::split(body) {
            let mut r = root.clone();
            r.children.insert(body_at, Node::El(piece));
            let name = format!("part{:04}.xhtml", outs.len());
            outs.push(Out { name, origin: page.origin.clone(), root: r });
        }
    }
    if outs.is_empty() {
        return Err("the book has no text".into());
    }

    // Where every id ended up, and each page's first file.
    let mut ids: HashMap<(String, String), String> = HashMap::new();
    let mut first: HashMap<String, String> = HashMap::new();
    for o in &outs {
        first.entry(o.origin.clone()).or_insert_with(|| o.name.clone());
        o.root.walk(&mut |e| {
            if let Some(id) = e.attr("id") {
                ids.entry((o.origin.clone(), id.to_owned())).or_insert_with(|| o.name.clone());
            }
        });
    }
    let links = Links { ids: &ids, first: &first, resources: &resources };
    let mut has_svg = HashSet::new();
    let mut has_math = HashSet::new();
    for o in &mut outs {
        let (origin, name) = (o.origin.clone(), o.name.clone());
        o.root.rebuild(&mut |e| links.fix(e, &origin, &name));
        o.root.walk(&mut |e| {
            match e.ns {
                Ns::Svg => has_svg.insert(name.clone()),
                Ns::Math => has_math.insert(name.clone()),
                _ => false,
            };
        });
    }

    // The table of contents: the book's, or one entry per file.
    let mut toc = fix_toc(book.toc, &links);
    if toc.is_empty() {
        toc = outs
            .iter()
            .enumerate()
            .map(|(i, o)| TocNode { label: label_for(&o.root, i), href: o.name.clone(), children: vec![] })
            .collect();
    }
    let guide: Vec<(String, String, String)> =
        book.guide.into_iter().filter_map(|(kind, t, href)| Some((kind, t, links.target(&href, "")?))).collect();

    let mut zip = zip::ZipWriter::new(File::create_new(dest)?);
    let result = (|| -> Res<()> {
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        zip.start_file("mimetype", stored)?;
        zip.write_all(b"application/epub+zip")?;
        zip.start_file("META-INF/container.xml", deflated)?;
        zip.write_all(CONTAINER.as_bytes())?;
        for o in &outs {
            zip.start_file(format!("OEBPS/text/{}", o.name), deflated)?;
            zip.write_all(html::to_xhtml(&o.root).as_bytes())?;
        }
        for r in &book.resources {
            let data = if r.media_type == "text/css" {
                fix_css(&String::from_utf8_lossy(&r.data), &r.path, &resources).into_bytes()
            } else {
                r.data.clone()
            };
            let opts =
                if r.media_type.starts_with("image/") && r.media_type != "image/svg+xml" { stored } else { deflated };
            zip.start_file(format!("OEBPS/{}", r.path), opts)?;
            zip.write_all(&data)?;
        }
        let uid = meta.uuids.first().cloned().unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let lang = meta.languages.first().map(|l| language_tag(l)).unwrap_or_else(|| "und".into());
        zip.start_file("OEBPS/nav.xhtml", deflated)?;
        zip.write_all(nav(&title, &lang, &toc, &guide).as_bytes())?;
        zip.start_file("OEBPS/toc.ncx", deflated)?;
        zip.write_all(ncx(&title, &uid, &toc).as_bytes())?;
        zip.start_file("OEBPS/content.opf", deflated)?;
        let opf = opf(
            meta,
            &title,
            &uid,
            &lang,
            &outs,
            &book.resources,
            book.cover.as_deref(),
            &guide,
            &has_svg,
            &has_math,
            book.rtl,
        );
        zip.write_all(opf.as_bytes())?;
        Ok(())
    })();
    let result = result.and_then(|()| -> Res<()> {
        zip.finish()?.sync_all()?;
        Ok(())
    });
    if let Err(e) = result {
        let _ = std::fs::remove_file(dest);
        return Err(e);
    }
    Ok(())
}

const CONTAINER: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>
"#;

struct Links<'a> {
    ids: &'a HashMap<(String, String), String>,
    first: &'a HashMap<String, String>,
    resources: &'a HashMap<&'a str, &'a Resource>,
}

impl Links<'_> {
    /// Where an internal link from page `origin` goes now, as "file#id"
    /// (relative to `text/`); None if it goes nowhere.
    fn target(&self, href: &str, origin: &str) -> Option<String> {
        let (file, frag) = href.split_once('#').unwrap_or((href, ""));
        let file = if file.is_empty() { origin } else { file.rsplit('/').next().unwrap_or(file) };
        let frag = frag.split_whitespace().collect::<Vec<_>>().join("_");
        if !frag.is_empty()
            && let Some(name) = self.ids.get(&(file.to_owned(), frag.clone()))
        {
            return Some(format!("{name}#{frag}"));
        }
        self.first.get(file).cloned()
    }

    /// A resource path for a reference from `text/`, if the book has it.
    fn resource(&self, href: &str) -> Option<String> {
        let path = resolve("text/", href.split(['#', '?']).next().unwrap_or(href));
        self.resources.contains_key(path.as_str()).then(|| format!("../{path}"))
    }

    /// One element of file `name` (from page `origin`), links fixed.
    fn fix(&self, mut e: El, origin: &str, name: &str) -> Vec<Node> {
        let external = |v: &str| {
            v.split_once(':').is_some_and(|(scheme, _)| {
                !scheme.is_empty() && scheme.chars().all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
            })
        };
        for key in ["href", "xlink:href"] {
            let Some(v) = e.attr(key).map(str::to_owned) else { continue };
            let new = if external(&v) {
                matches!(v.split(':').next().map(str::to_ascii_lowercase).as_deref(), Some("http" | "https" | "mailto"))
                    .then_some(v)
            } else if e.is("link") || (e.ns == Ns::Svg && e.name == "image") {
                self.resource(&v)
            } else if let Some(r) = self.resource(&v) {
                Some(r)
            } else {
                self.target(&v, origin).map(|t| match t.strip_prefix(name) {
                    Some(rest) if rest.starts_with('#') => rest.to_owned(),
                    _ => t,
                })
            };
            match new {
                Some(n) => e.set_attr(key, &n),
                None => {
                    e.remove_attr(key);
                }
            }
        }
        for key in ["src", "poster", "data"] {
            let Some(v) = e.attr(key).map(str::to_owned) else { continue };
            match self.resource(&v) {
                Some(n) => e.set_attr(key, &n),
                None => {
                    e.remove_attr(key);
                }
            }
        }
        let missing = match e.ns {
            Ns::Html => match e.name.as_str() {
                "img" | "source" => e.attr("src").is_none(),
                "link" => e.attr("href").is_none(),
                "object" => e.attr("data").is_none(),
                _ => false,
            },
            Ns::Svg => e.name == "image" && e.attr("xlink:href").is_none() && e.attr("href").is_none(),
            _ => false,
        };
        if missing {
            return if e.is("object") { e.children } else { vec![] };
        }
        vec![Node::El(e)]
    }
}

/// Resolve `href` against the folder `base` ("text/"), as a path from the
/// book's folder.
fn resolve(base: &str, href: &str) -> String {
    let mut parts: Vec<&str> = base.split('/').filter(|p| !p.is_empty()).collect();
    for p in href.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    parts.join("/")
}

/// `url()` in a stylesheet that points at nothing the book has: dropped.
fn fix_css(css: &str, path: &str, resources: &HashMap<&str, &Resource>) -> String {
    static URL: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"(?i)url\(\s*(?:"([^"]*)"|'([^']*)'|([^'"()\s]*))\s*\)"#).expect("regex"));
    let dir = path.rsplit_once('/').map_or("", |(d, _)| d);
    URL.replace_all(css, |c: &regex::Captures<'_>| {
        let url = c.get(1).or(c.get(2)).or(c.get(3)).map_or("", |m| m.as_str());
        let target = resolve(dir, url.split(['#', '?']).next().unwrap_or(url));
        if url.starts_with("data:") || resources.contains_key(target.as_str()) {
            c[0].to_owned()
        } else {
            "none".to_owned()
        }
    })
    .into_owned()
}

fn fix_toc(nodes: Vec<TocNode>, links: &Links<'_>) -> Vec<TocNode> {
    nodes
        .into_iter()
        .filter_map(|n| {
            let children = fix_toc(n.children, links);
            match links.target(&n.href, "") {
                Some(href) => Some(vec![TocNode { label: n.label, href, children }]),
                // An entry that points nowhere: its children move up.
                None => (!children.is_empty()).then_some(children),
            }
        })
        .flatten()
        .collect()
}

/// A name for a file in a made-up table of contents: its first heading.
fn label_for(root: &El, i: usize) -> String {
    let mut found = None;
    root.walk(&mut |e| {
        if found.is_none() && e.ns == Ns::Html && matches!(e.name.as_str(), "h1" | "h2" | "h3") {
            let t = e.text().split_whitespace().collect::<Vec<_>>().join(" ");
            if !t.is_empty() {
                found = Some(t);
            }
        }
    });
    found.unwrap_or_else(|| format!("Part {}", i + 1))
}

fn label(n: &TocNode) -> String {
    let l = n.label.split_whitespace().collect::<Vec<_>>().join(" ");
    if l.is_empty() { "Untitled".into() } else { l }
}

fn nav(title: &str, lang: &str, toc: &[TocNode], guide: &[(String, String, String)]) -> String {
    fn list(nodes: &[TocNode], out: &mut String, indent: usize) {
        let pad = "  ".repeat(indent);
        out.push_str(&format!("{pad}<ol>\n"));
        for n in nodes {
            out.push_str(&format!(
                "{pad}  <li><a href=\"text/{}\">{}</a>",
                escape_text(&n.href),
                escape_text(&label(n))
            ));
            if !n.children.is_empty() {
                out.push('\n');
                list(&n.children, out, indent + 2);
                out.push_str(&format!("{pad}  "));
            }
            out.push_str("</li>\n");
        }
        out.push_str(&format!("{pad}</ol>\n"));
    }
    let mut out = format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" \
         xmlns:epub=\"http://www.idpf.org/2007/ops\" lang=\"{lang}\" xml:lang=\"{lang}\">\n<head><title>{}</title></head>\n<body>\n\
         <nav epub:type=\"toc\" id=\"toc\">\n<h1>Contents</h1>\n",
        escape_text(title)
    );
    list(toc, &mut out, 0);
    out.push_str("</nav>\n");
    let landmarks: Vec<(&str, &str, &str)> = guide
        .iter()
        .filter_map(|(kind, t, href)| {
            let epub_type = match kind.to_ascii_lowercase().as_str() {
                "cover" => "cover",
                "toc" => "toc",
                "text" | "start" | "bodymatter" => "bodymatter",
                "title-page" => "titlepage",
                "copyright-page" => "copyright-page",
                "acknowledgements" => "acknowledgments",
                "dedication" => "dedication",
                "preface" => "preface",
                "foreword" => "foreword",
                "glossary" => "glossary",
                "index" => "index",
                _ => return None,
            };
            Some((epub_type, t.as_str(), href.as_str()))
        })
        .collect();
    if !landmarks.is_empty() {
        out.push_str("<nav epub:type=\"landmarks\" id=\"landmarks\" hidden=\"hidden\">\n<ol>\n");
        let mut seen = HashSet::new();
        for (kind, t, href) in landmarks {
            if seen.insert(kind) {
                let t = if t.trim().is_empty() { kind } else { t };
                out.push_str(&format!(
                    "  <li><a epub:type=\"{kind}\" href=\"text/{}\">{}</a></li>\n",
                    escape_text(href),
                    escape_text(t)
                ));
            }
        }
        out.push_str("</ol>\n</nav>\n");
    }
    out.push_str("</body>\n</html>\n");
    out
}

fn ncx(title: &str, uid: &str, toc: &[TocNode]) -> String {
    fn points(nodes: &[TocNode], order: &mut usize, depth: usize, max: &mut usize, out: &mut String) {
        *max = (*max).max(depth);
        for n in nodes {
            *order += 1;
            let pad = "  ".repeat(depth + 1);
            out.push_str(&format!(
                "{pad}<navPoint id=\"np{order}\" playOrder=\"{order}\"><navLabel><text>{}</text></navLabel><content src=\"text/{}\"/>\n",
                escape_text(&label(n)),
                escape_text(&n.href)
            ));
            points(&n.children, order, depth + 1, max, out);
            out.push_str(&format!("{pad}</navPoint>\n"));
        }
    }
    let (mut order, mut depth) = (0, 0);
    let mut map = String::new();
    points(toc, &mut order, 1, &mut depth, &mut map);
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<ncx xmlns=\"http://www.daisy.org/z3986/2005/ncx/\" version=\"2005-1\">\n\
         <head>\n  <meta name=\"dtb:uid\" content=\"urn:uuid:{}\"/>\n  <meta name=\"dtb:depth\" content=\"{depth}\"/>\n  \
         <meta name=\"dtb:totalPageCount\" content=\"0\"/>\n  <meta name=\"dtb:maxPageNumber\" content=\"0\"/>\n</head>\n\
         <docTitle><text>{}</text></docTitle>\n<navMap>\n{map}</navMap>\n</ncx>\n",
        escape_text(uid),
        escape_text(title)
    )
}

#[allow(clippy::too_many_arguments)]
fn opf(
    meta: &BookMetadata,
    title: &str,
    uid: &str,
    lang: &str,
    outs: &[Out],
    resources: &[Resource],
    cover: Option<&str>,
    guide: &[(String, String, String)],
    has_svg: &HashSet<String>,
    has_math: &HashSet<String>,
    rtl: bool,
) -> String {
    let e = escape_text;
    let mut m = String::new();
    m.push_str(&format!("    <dc:identifier id=\"uid\">urn:uuid:{}</dc:identifier>\n", e(uid)));
    m.push_str(&format!("    <dc:title>{}</dc:title>\n", e(title)));
    for (i, a) in meta.authors.iter().enumerate() {
        m.push_str(&format!("    <dc:creator id=\"creator{i}\">{}</dc:creator>\n", e(&a.name)));
        m.push_str(&format!(
            "    <meta refines=\"#creator{i}\" property=\"role\" scheme=\"marc:relators\">aut</meta>\n"
        ));
        if let Some(sort) = a.sort.as_deref().filter(|s| !s.is_empty()) {
            m.push_str(&format!("    <meta refines=\"#creator{i}\" property=\"file-as\">{}</meta>\n", e(sort)));
        }
    }
    m.push_str(&format!("    <dc:language>{}</dc:language>\n", e(lang)));
    if let Some(p) = &meta.publisher {
        m.push_str(&format!("    <dc:publisher>{}</dc:publisher>\n", e(p)));
    }
    if let Some(d) = meta.published {
        m.push_str(&format!("    <dc:date>{}</dc:date>\n", d.format("%Y-%m-%d")));
    }
    if let Some(d) = &meta.description {
        // Kindle files often hold entities ("&#8217;") in plain text.
        let d = if d.contains('<') { d.clone() } else { crate::convert::azw3::dom::unescape(d) };
        m.push_str(&format!("    <dc:description>{}</dc:description>\n", e(&d)));
    }
    for t in &meta.tags {
        m.push_str(&format!("    <dc:subject>{}</dc:subject>\n", e(t)));
    }
    if let Some(isbn) = meta.identifiers.get("isbn") {
        m.push_str(&format!("    <dc:identifier>urn:isbn:{}</dc:identifier>\n", e(isbn)));
    }
    let modified = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ");
    m.push_str(&format!("    <meta property=\"dcterms:modified\">{modified}</meta>\n"));
    if cover.is_some() {
        m.push_str("    <meta name=\"cover\" content=\"cover\"/>\n");
    }

    let mut items = String::new();
    items.push_str(
        "    <item id=\"nav\" href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"/>\n",
    );
    items.push_str("    <item id=\"ncx\" href=\"toc.ncx\" media-type=\"application/x-dtbncx+xml\"/>\n");
    let mut spine = String::new();
    for (i, o) in outs.iter().enumerate() {
        let props: Vec<&str> = [(has_svg.contains(&o.name), "svg"), (has_math.contains(&o.name), "mathml")]
            .iter()
            .filter(|(on, _)| *on)
            .map(|(_, p)| *p)
            .collect();
        let props = if props.is_empty() { String::new() } else { format!(" properties=\"{}\"", props.join(" ")) };
        items.push_str(&format!(
            "    <item id=\"p{i}\" href=\"text/{}\" media-type=\"application/xhtml+xml\"{props}/>\n",
            e(&o.name)
        ));
        spine.push_str(&format!("    <itemref idref=\"p{i}\"/>\n"));
    }
    for (i, r) in resources.iter().enumerate() {
        let (id, props) = if Some(r.path.as_str()) == cover {
            ("cover".to_owned(), " properties=\"cover-image\"")
        } else {
            (format!("r{i}"), "")
        };
        items.push_str(&format!(
            "    <item id=\"{id}\" href=\"{}\" media-type=\"{}\"{props}/>\n",
            e(&r.path),
            r.media_type
        ));
    }
    let mut refs = String::new();
    let mut seen = HashSet::new();
    for (kind, t, href) in guide {
        const TYPES: &[&str] = &[
            "cover",
            "title-page",
            "toc",
            "index",
            "glossary",
            "acknowledgements",
            "bibliography",
            "colophon",
            "copyright-page",
            "dedication",
            "epigraph",
            "foreword",
            "loi",
            "lot",
            "notes",
            "preface",
            "text",
        ];
        let kind = match kind.as_str() {
            "start" => "text".to_owned(),
            k if TYPES.contains(&k) || k.starts_with("other.") => k.to_owned(),
            k => format!("other.{}", k.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect::<String>()),
        };
        if seen.insert(kind.clone()) {
            refs.push_str(&format!(
                "    <reference type=\"{}\" title=\"{}\" href=\"text/{}\"/>\n",
                e(&kind),
                e(t),
                e(href)
            ));
        }
    }
    let guide = if refs.is_empty() { String::new() } else { format!("  <guide>\n{refs}  </guide>\n") };
    let direction = if rtl { " page-progression-direction=\"rtl\"" } else { "" };
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" \
         unique-identifier=\"uid\" xml:lang=\"{lang}\">\n  <metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\n{m}  \
         </metadata>\n  <manifest>\n{items}  </manifest>\n  <spine toc=\"ncx\"{direction}>\n{spine}  </spine>\n{guide}</package>\n"
    )
}
