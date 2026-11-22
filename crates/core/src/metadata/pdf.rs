//! PDF metadata: the document Info dictionary and the XMP packet, and a
//! cover when page 1 is one big image (a scanned book's).
//!
//! PDFs are written by every kind of software, and many say nothing useful
//! about the book: "Microsoft Word - draft3.docx" as the title, the user's
//! login or the producing program as the author. Values like that are
//! dropped here so the file name fills in for them instead. XMP wins over
//! the Info dictionary where both have a usable value.
//!
//! Pages aren't rendered (that would take a C library like pdfium): the
//! cover is only taken when page 1 draws a JPEG (or plain 8-bit RGB or grey
//! pixels) over nearly all of the page.

use std::path::Path;

use chrono::NaiveDate;
use lopdf::content::Content;
use lopdf::{Dictionary, Document, Object};

use super::{Author, BookMetadata, parse_date};

/// Files bigger than this aren't read: the whole file is loaded to parse it.
const MAX_FILE: u64 = 256 << 20;
/// Page 1's content stream is decoded only up to this size.
const MAX_CONTENT: usize = 4 << 20;
/// A cover image decoded from raw pixels is at most this big.
const MAX_PIXELS: u64 = 40_000_000;

const DC: &str = "http://purl.org/dc/elements/1.1/";
const XMP: &str = "http://ns.adobe.com/xap/1.0/";
const PDF: &str = "http://ns.adobe.com/pdf/1.3/";
const RDF: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";

/// Metadata from a PDF, or None if it can't be read (not a PDF, broken, or
/// encrypted with a password).
pub fn read(path: &Path) -> Option<BookMetadata> {
    if std::fs::metadata(path).ok()?.len() > MAX_FILE {
        return None;
    }
    parse(&std::fs::read(path).ok()?)
}

pub(crate) fn parse(d: &[u8]) -> Option<BookMetadata> {
    // A PDF with only an owner password (printing or copying restricted)
    // opens with the empty user password; lopdf does that itself.
    let doc = Document::load_mem(d).ok()?;
    if doc.is_encrypted() {
        return None;
    }
    let info = Fields::from_info(&doc);
    let xmp = xmp_packet(&doc).map(|x| Fields::from_xmp(&x)).unwrap_or_default();

    let software: Vec<String> =
        info.software.iter().chain(&xmp.software).map(|s| s.to_lowercase()).filter(|s| !s.is_empty()).collect();
    let pick = |a: &Option<String>, b: &Option<String>, junk: &dyn Fn(&str) -> bool| {
        [a, b].into_iter().flatten().map(|v| tidy(v)).find(|v| !junk(v))
    };

    let mut out = BookMetadata {
        title: pick(&xmp.title, &info.title, &|t| junk_title(t, &software)),
        publisher: pick(&xmp.publisher, &info.publisher, &|p| junk_name(p, &software)),
        published: xmp.date.or(info.date),
        ..Default::default()
    };
    // XMP lists authors one by one; the Info dictionary has one string.
    for names in [&xmp.authors, &info.authors] {
        let found: Vec<Author> =
            names.iter().flat_map(|n| split_authors(n)).filter(|a| !junk_name(&a.name, &software)).collect();
        if !found.is_empty() {
            out.authors = found;
            break;
        }
    }
    out.tags = [&xmp.keywords, &info.keywords]
        .into_iter()
        .map(|k| clean_tags(k, &software))
        .find(|t| !t.is_empty())
        .unwrap_or_default();
    if let Some(subject) =
        pick(&xmp.subject, &info.subject, &|s| !is_tag(s, &software) || s.split_whitespace().count() > 3)
        && !out.tags.iter().any(|t| t.eq_ignore_ascii_case(&subject))
    {
        out.tags.push(subject);
    }
    // Keywords often repeat the title.
    if let Some(title) = &out.title {
        out.tags.retain(|t| !t.eq_ignore_ascii_case(title));
    }
    if let Some(isbn) = xmp.isbn {
        out.identifiers.insert("isbn".into(), isbn);
    }
    out.cover = cover(&doc);
    Some(out)
}

/// What one source (Info or XMP) says, before junk is weeded out.
#[derive(Default)]
struct Fields {
    title: Option<String>,
    authors: Vec<String>,
    subject: Option<String>,
    keywords: Vec<String>,
    publisher: Option<String>,
    date: Option<NaiveDate>,
    isbn: Option<String>,
    /// The programs that made the file (Creator, Producer, CreatorTool):
    /// a title or author that's one of these is junk.
    software: Vec<String>,
}

impl Fields {
    fn from_info(doc: &Document) -> Fields {
        let Some(info) =
            doc.trailer.get(b"Info").ok().and_then(|i| doc.dereference(i).ok()).and_then(|(_, i)| i.as_dict().ok())
        else {
            return Fields::default();
        };
        let text = |key: &[u8]| {
            let obj = info.get(key).ok().and_then(|o| doc.dereference(o).ok())?.1;
            lopdf::decode_text_string(obj).ok().map(|s| s.trim().to_owned()).filter(|s| !s.is_empty())
        };
        Fields {
            title: text(b"Title"),
            authors: text(b"Author").into_iter().collect(),
            subject: text(b"Subject"),
            keywords: text(b"Keywords").into_iter().collect(),
            // Not a standard key, but some publishers add it.
            publisher: text(b"Publisher"),
            date: text(b"CreationDate").and_then(|d| info_date(&d)),
            isbn: None,
            software: [text(b"Creator"), text(b"Producer")].into_iter().flatten().collect(),
        }
    }

    fn from_xmp(xml: &str) -> Fields {
        let Ok(doc) = roxmltree::Document::parse_with_options(
            xml,
            roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() },
        ) else {
            return Fields::default();
        };
        let first = |ns: &str, name: &str| xmp_values(&doc, ns, name).into_iter().next();
        let isbn = doc
            .descendants()
            .flat_map(|n| {
                let own = n.is_element() && n.tag_name().name().eq_ignore_ascii_case("isbn");
                let mut v: Vec<String> = Vec::new();
                if own {
                    v.extend(element_values(n));
                }
                if n.has_tag_name((DC, "identifier")) || n.has_tag_name((XMP, "Identifier")) {
                    v.extend(element_values(n));
                }
                v.extend(
                    n.attributes().filter(|a| a.name().eq_ignore_ascii_case("isbn")).map(|a| a.value().to_owned()),
                );
                v
            })
            .find_map(|v| valid_isbn(&v));
        Fields {
            title: first(DC, "title"),
            authors: xmp_values(&doc, DC, "creator"),
            subject: first(DC, "description"),
            keywords: {
                let subjects = xmp_values(&doc, DC, "subject");
                if subjects.is_empty() { xmp_values(&doc, PDF, "Keywords") } else { subjects }
            },
            publisher: first(DC, "publisher"),
            date: [first(DC, "date"), first(XMP, "CreateDate")].into_iter().flatten().find_map(|d| parse_date(&d)),
            isbn,
            software: [first(XMP, "CreatorTool"), first(PDF, "Producer")].into_iter().flatten().collect(),
        }
    }
}

/// The catalog's XMP metadata stream, as text.
fn xmp_packet(doc: &Document) -> Option<String> {
    let meta = doc.catalog().ok()?.get(b"Metadata").ok()?;
    let stream = doc.dereference(meta).ok()?.1.as_stream().ok()?;
    let bytes = stream.decompressed_content().unwrap_or_else(|_| stream.content.clone());
    let text = String::from_utf8_lossy(&bytes);
    let start = text.find('<')?;
    let end = text.rfind('>')?;
    Some(text.get(start..=end)?.to_owned())
}

/// Every value of an XMP property, as element (simple, or an rdf:Alt, Seq
/// or Bag of rdf:li) or as an attribute of rdf:Description. For a language
/// alternative the x-default entry comes first.
fn xmp_values(doc: &roxmltree::Document, ns: &str, name: &str) -> Vec<String> {
    let mut out = Vec::new();
    for n in doc.descendants() {
        if n.has_tag_name((ns, name)) {
            out.extend(element_values(n));
        }
        if let Some(v) = n.attribute((ns, name)) {
            out.push(v.trim().to_owned());
        }
    }
    out.retain(|v| !v.is_empty());
    out
}

fn element_values(n: roxmltree::Node) -> Vec<String> {
    let mut items: Vec<roxmltree::Node> = n.descendants().filter(|c| c.has_tag_name((RDF, "li"))).collect();
    if items.is_empty() {
        return n.text().map(|t| t.trim().to_owned()).into_iter().collect();
    }
    let x_default = |c: &roxmltree::Node| {
        c.attribute(("http://www.w3.org/XML/1998/namespace", "lang"))
            .is_some_and(|l| l.eq_ignore_ascii_case("x-default"))
    };
    items.sort_by_key(|c| !x_default(c));
    items.iter().map(|c| c.text().unwrap_or("").trim().to_owned()).filter(|v| !v.is_empty()).collect()
}

/// "D:20210315120000+01'00'" → 2021-03-15.
fn info_date(s: &str) -> Option<NaiveDate> {
    let s = s.trim().trim_start_matches("D:");
    let digits: String = s.chars().take_while(char::is_ascii_digit).collect();
    let iso = match digits.len() {
        n if n >= 8 => format!("{}-{}-{}", &digits[..4], &digits[4..6], &digits[6..8]),
        6 | 7 => format!("{}-{}", &digits[..4], &digits[4..6]),
        4 | 5 => digits[..4].to_owned(),
        // Some writers put an ISO date here instead.
        _ => s.to_owned(),
    };
    parse_date(&iso)
}

/// ISBN-10 or -13 digits, if `s` is one with a valid check digit. Takes
/// "urn:isbn:…", "ISBN 978-…" and bare numbers.
fn valid_isbn(s: &str) -> Option<String> {
    let lower = s.trim().to_ascii_lowercase();
    let rest = lower.trim_start_matches("urn:").trim_start_matches("isbn").trim_start_matches([':', ' ', '-']);
    if !rest.chars().all(|c| c.is_ascii_digit() || matches!(c, 'x' | '-' | ' ')) {
        return None;
    }
    let isbn = super::epub::normalize_isbn(rest);
    let d: Vec<u32> = isbn.chars().map(|c| c.to_digit(10).unwrap_or(10)).collect();
    let ok = match d.len() {
        10 => d[..9].iter().all(|&x| x < 10) && d.iter().zip((1..=10).rev()).map(|(x, w)| x * w).sum::<u32>() % 11 == 0,
        13 => {
            d.iter().all(|&x| x < 10)
                && d.iter().enumerate().map(|(i, x)| x * if i % 2 == 0 { 1 } else { 3 }).sum::<u32>() % 10 == 0
        }
        _ => false,
    };
    ok.then_some(isbn)
}

fn tidy(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// File extensions a junk title ends with: a document's own file name.
const FILE_EXTENSIONS: &[&str] = &[
    "doc", "docx", "rtf", "odt", "txt", "pdf", "tex", "dvi", "ps", "eps", "indd", "qxd", "qxp", "p65", "pm6", "fm",
    "ppt", "pptx", "xls", "xlsx", "htm", "html", "wpd", "pages", "key", "md", "jpg", "jpeg", "png", "tif", "tiff",
    "gif", "bmp", "vsd", "vsdx", "ai", "psd", "cdr", "pub", "sla", "sxw", "odp", "ods", "odg", "wps", "xps", "dwg",
    "svg", "numbers", "dot", "dotx", "epub", "mobi", "djvu",
];

/// Placeholder titles, compared after trailing numbers are dropped
/// ("Document1", "Untitled-2", "Layout 1").
const PLACEHOLDER_TITLES: &[&str] = &[
    "untitled",
    "untitled document",
    "no title",
    "title",
    "document",
    "new document",
    "unknown",
    "none",
    "null",
    "n/a",
    "default",
    "doc",
    "pdf",
    "book",
    "cover",
    "print",
    "layout",
    "slide",
    "presentation",
    "powerpoint presentation",
    "word document",
    "microsoft word document",
    "blank",
    "test",
    "draft",
    "temp",
    "scan",
    "scanned document",
    "page",
    "img",
    "image",
];

/// Words that make an author or publisher the name of a program.
const SOFTWARE_WORDS: &[&str] = &[
    "microsoft",
    "adobe",
    "acrobat",
    "distiller",
    "pdftex",
    "latex",
    "xetex",
    "luatex",
    "ghostscript",
    "pscript5",
    "pscript5.dll",
    "quartz",
    "pdfcontext",
    "abbyy",
    "finereader",
    "openoffice",
    "libreoffice",
    "indesign",
    "framemaker",
    "pdfcreator",
    "itext",
    "itextsharp",
    "nitro",
    "foxit",
    "scansoft",
    "omnipage",
    "tcpdf",
    "fpdf",
    "wkhtmltopdf",
    "skia",
    "chromium",
    "pdfium",
    "pdflib",
    "reportlab",
    "cairo",
    "dvips",
    "dvipdfm",
    "dvipdfmx",
];

/// A title that isn't one: empty, a placeholder, a file name or path, or
/// the producing program's name.
fn junk_title(t: &str, software: &[String]) -> bool {
    let lower = t.to_lowercase();
    let bare = lower.trim_end_matches(|c: char| c.is_ascii_digit() || matches!(c, ' ' | '-' | '_' | '.'));
    if !t.chars().any(char::is_alphabetic) || t.chars().filter(|c| c.is_alphanumeric()).count() < 2 {
        return true;
    }
    if PLACEHOLDER_TITLES.contains(&bare) || PLACEHOLDER_TITLES.contains(&lower.as_str()) {
        return true;
    }
    // "Microsoft Word - draft3.docx", "Microsoft PowerPoint - talk".
    if lower.starts_with("microsoft ") && lower.contains(" - ") {
        return true;
    }
    if let Some((_, ext)) = lower.rsplit_once('.')
        && FILE_EXTENSIONS.contains(&ext.trim())
    {
        return true;
    }
    // A path, or a working file name: underscores ("Love Letter_r2"), a
    // layout ("Booklet:Layout 1"), a print version ("Guide-lo-res").
    let path = t.contains('\\') || t.starts_with('/') || t.get(1..3) == Some(":\\") || t.get(1..3) == Some(":/");
    let layout =
        lower.match_indices("layout ").any(|(i, m)| lower[i + m.len()..].starts_with(|c: char| c.is_ascii_digit()));
    let print = ["lo-res", "hi-res", "lowres", "hires", "print-ready"].iter().any(|m| lower.contains(m));
    if path || t.contains('_') || layout || print {
        return true;
    }
    // The program's own name. (Not any title naming a program: books
    // about Photoshop or LaTeX are real books.)
    software.contains(&lower) || lower.contains("pscript5.dll")
}

/// An author or publisher that isn't one: a placeholder, a login, an email
/// address, or a program.
fn junk_name(name: &str, software: &[String]) -> bool {
    const PLACEHOLDERS: &[&str] = &[
        "unknown",
        "author",
        "authors",
        "user",
        "admin",
        "administrator",
        "owner",
        "default",
        "guest",
        "test",
        "name",
        "your name",
        "me",
        "standard",
        "valued customer",
        "preferred customer",
        "windows user",
        "office user",
        "microsoft office user",
        "registered user",
        "pc",
        "home",
        "publisher",
        "none",
        "null",
        "n/a",
        "anonymous user",
    ];
    let lower = name.trim().to_lowercase();
    if !name.chars().any(char::is_alphabetic) || PLACEHOLDERS.contains(&lower.as_str()) {
        return true;
    }
    // An email address, a domain login, a website.
    if name.contains(['@', '\\', '/']) || [".com", ".org", ".net", "www."].iter().any(|d| lower.contains(d)) {
        return true;
    }
    // A login: one word, lower case, with digits or underscores, or with
    // capitals inside ("jsmith", "user01", "TrevorK"). "Plato" is a name.
    if !name.contains(char::is_whitespace)
        && (name.chars().any(|c| c.is_ascii_digit() || c == '_')
            || name.chars().all(|c| !c.is_uppercase())
            || name.chars().skip(1).any(char::is_uppercase))
    {
        return true;
    }
    software.iter().any(|s| *s == lower || s.starts_with(&format!("{lower} "))) || has_software_word(&lower)
}

fn has_software_word(lower: &str) -> bool {
    lower
        .split(|c: char| !(c.is_alphanumeric() || c == '.'))
        .map(|w| w.trim_end_matches('.'))
        .any(|w| SOFTWARE_WORDS.contains(&w))
}

/// "A; B", "A & B", "A and B", "Surname, Given" or "A B, C D" → names.
fn split_authors(s: &str) -> Vec<Author> {
    let mut out = Vec::new();
    for part in s.split([';', '&']).flat_map(|p| p.split(" and ")).map(str::trim).filter(|p| !p.is_empty()) {
        let full_name = |n: &str| {
            let words: Vec<&str> = n.split_whitespace().collect();
            words.len() >= 2
                && words
                    .last()
                    .is_some_and(|w| w.chars().filter(|c| c.is_alphabetic()).count() >= 2 && !w.ends_with('.'))
        };
        let commas = part.matches(',').count();
        let surname_first = commas == 1 && {
            let (a, b) = part.split_once(',').unwrap_or_default();
            !(full_name(a.trim()) && full_name(b.trim()))
        };
        if surname_first && let Some((name, sort)) = crate::names::from_surname_first(part) {
            out.push(Author { name, sort: Some(sort) });
            continue;
        }
        out.extend(part.split(',').map(tidy).filter(|n| !n.is_empty()).map(|name| Author { name, sort: None }));
    }
    out
}

/// A keyword or subject that reads as a tag: short, words not a sentence.
fn is_tag(t: &str, software: &[String]) -> bool {
    let lower = t.to_lowercase();
    let len = t.chars().count();
    (2..=40).contains(&len)
        && t.split_whitespace().count() <= 4
        && t.chars().any(char::is_alphabetic)
        && !lower.contains("http")
        && !lower.contains("www.")
        && !matches!(lower.as_str(), "keyword" | "keywords" | "tag" | "tags" | "subject")
        && !junk_title(t, software)
}

/// Keywords as tags, if they're clean: a list of short terms. A sentence,
/// or a list that's mostly junk, gives none.
fn clean_tags(raw: &[String], software: &[String]) -> Vec<String> {
    let terms: Vec<String> = raw
        .iter()
        .flat_map(|k| k.split([',', ';', '\n', '\r']))
        .map(|t| tidy(t.trim_matches(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '.'))))
        .filter(|t| !t.is_empty())
        .collect();
    let good: Vec<String> = terms.iter().filter(|t| is_tag(t, software)).cloned().collect();
    if good.len() * 2 < terms.len() {
        return Vec::new();
    }
    let mut out: Vec<String> = Vec::new();
    for t in good {
        if !out.iter().any(|o| o.eq_ignore_ascii_case(&t)) {
            out.push(t);
        }
    }
    out.truncate(20);
    out
}

/// A 2D transform: [a b c d e f], as PDF writes them.
type Matrix = [f32; 6];

fn multiply(m: &Matrix, n: &Matrix) -> Matrix {
    [
        m[0] * n[0] + m[1] * n[2],
        m[0] * n[1] + m[1] * n[3],
        m[2] * n[0] + m[3] * n[2],
        m[2] * n[1] + m[3] * n[3],
        m[4] * n[0] + m[5] * n[2] + n[4],
        m[4] * n[1] + m[5] * n[3] + n[5],
    ]
}

/// Page 1's image, when it covers (almost) the whole page: a scan.
fn cover(doc: &Document) -> Option<Vec<u8>> {
    let page_id = *doc.get_pages().values().next()?;
    let page = doc.get_dictionary(page_id).ok()?;
    let [x0, y0, x1, y1] = page_box(doc, page)?;
    let (page_w, page_h) = ((x1 - x0).abs(), (y1 - y0).abs());
    if page_w < 1.0 || page_h < 1.0 {
        return None;
    }
    let xobjects = inherited(doc, page, b"Resources")
        .and_then(|r| r.as_dict().ok())
        .and_then(|r| r.get(b"XObject").ok())
        .and_then(|x| doc.dereference(x).ok())
        .and_then(|(_, x)| x.as_dict().ok())?;
    let content = doc.get_page_content_with_limit(page_id, MAX_CONTENT).ok()?;

    let mut ctm: Matrix = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    let mut stack = Vec::new();
    for op in Content::decode_iter(&content) {
        let Ok(op) = op else { break };
        match op.operator.as_str() {
            "q" => stack.push(ctm),
            "Q" => ctm = stack.pop().unwrap_or(ctm),
            "cm" if op.operands.len() == 6 => {
                let n: Vec<f32> = op.operands.iter().filter_map(|o| o.as_float().ok()).collect();
                if let Ok(m) = <[f32; 6]>::try_from(n) {
                    ctm = multiply(&m, &ctm);
                }
            }
            "Do" => {
                let Some(name) = op.operands.first().and_then(|o| o.as_name().ok()) else { continue };
                // Where the unit square the image fills ends up on the page.
                let corners = [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)]
                    .map(|(x, y)| (ctm[0] * x + ctm[2] * y + ctm[4], ctm[1] * x + ctm[3] * y + ctm[5]));
                let span = |f: fn(&(f32, f32)) -> f32| {
                    let v = corners.iter().map(f);
                    v.clone().fold(f32::MIN, f32::max) - v.fold(f32::MAX, f32::min)
                };
                if span(|c| c.0) < page_w * 0.85 || span(|c| c.1) < page_h * 0.85 {
                    continue;
                }
                let image =
                    xobjects.get(name).ok().and_then(|x| doc.dereference(x).ok()).and_then(|(_, x)| x.as_stream().ok());
                if let Some(bytes) = image.and_then(|s| image_bytes(doc, s)) {
                    return Some(bytes);
                }
            }
            _ => {}
        }
    }
    None
}

/// A page's CropBox, else its MediaBox, inherited from the page tree.
fn page_box(doc: &Document, page: &Dictionary) -> Option<[f32; 4]> {
    let b = inherited(doc, page, b"CropBox").or_else(|| inherited(doc, page, b"MediaBox"))?;
    let n: Vec<f32> = b.as_array().ok()?.iter().filter_map(|o| doc.dereference(o).ok()?.1.as_float().ok()).collect();
    n.try_into().ok()
}

/// A page attribute, from the page or the nearest ancestor that has it.
fn inherited<'a>(doc: &'a Document, page: &'a Dictionary, key: &[u8]) -> Option<&'a Object> {
    let mut node = page;
    for _ in 0..32 {
        if let Ok(v) = node.get(key) {
            return doc.dereference(v).ok().map(|(_, v)| v);
        }
        node = doc.get_dictionary(node.get(b"Parent").ok()?.as_reference().ok()?).ok()?;
    }
    None
}

/// An image XObject's bytes as an image file: a JPEG as stored, or plain
/// 8-bit RGB or grey pixels as PNG. Other encodings (JPEG 2000, fax, JBIG2,
/// CMYK, indexed colour) are skipped.
fn image_bytes(doc: &Document, s: &lopdf::Stream) -> Option<Vec<u8>> {
    let d = &s.dict;
    if d.get(b"Subtype").ok()?.as_name().ok()? != b"Image" {
        return None;
    }
    let num = |k: &[u8]| d.get(k).ok().and_then(|o| doc.dereference(o).ok()?.1.as_i64().ok());
    let (w, h) = (u32::try_from(num(b"Width")?).ok()?, u32::try_from(num(b"Height")?).ok()?);
    if w < 200 || h < 200 || d.has(b"Decode") || d.has(b"ImageMask") {
        return None;
    }
    let channels = match d.get(b"ColorSpace").ok().and_then(|c| doc.dereference(c).ok()).map(|(_, c)| c)? {
        Object::Name(n) if n == b"DeviceRGB" || n == b"CalRGB" => 3,
        Object::Name(n) if n == b"DeviceGray" || n == b"CalGray" => 1,
        Object::Array(a) if a.first().and_then(|f| f.as_name().ok()) == Some(b"ICCBased") => {
            let profile = a.get(1).and_then(|p| doc.dereference(p).ok()).and_then(|(_, p)| p.as_stream().ok())?;
            match profile.dict.get(b"N").ok()?.as_i64().ok()? {
                n @ (1 | 3) => n,
                _ => return None,
            }
        }
        _ => return None,
    };
    let filters: Vec<&[u8]> = match d.get(b"Filter").ok().and_then(|f| doc.dereference(f).ok()).map(|(_, f)| f) {
        None => Vec::new(),
        Some(Object::Name(n)) => vec![n.as_slice()],
        Some(Object::Array(a)) => a.iter().filter_map(|f| f.as_name().ok()).collect(),
        Some(_) => return None,
    };
    match filters.as_slice() {
        [b"DCTDecode"] => s.content.starts_with(&[0xFF, 0xD8]).then(|| s.content.clone()),
        [] | [b"FlateDecode"] => {
            if num(b"BitsPerComponent") != Some(8) || u64::from(w) * u64::from(h) > MAX_PIXELS {
                return None;
            }
            // A predictor would need undoing first; lopdf does that.
            let pixels = if filters.is_empty() { s.content.clone() } else { s.decompressed_content().ok()? };
            let want = w as usize * h as usize * channels as usize;
            let pixels = pixels.get(..want)?.to_vec();
            let color = if channels == 3 { image::ExtendedColorType::Rgb8 } else { image::ExtendedColorType::L8 };
            let mut png = Vec::new();
            image::ImageEncoder::write_image(image::codecs::png::PngEncoder::new(&mut png), &pixels, w, h, color)
                .ok()?;
            Some(png)
        }
        _ => None,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use lopdf::{Stream, dictionary};

    /// What goes into a generated test PDF.
    #[derive(Default)]
    pub(crate) struct Spec<'a> {
        pub info: Vec<(&'a str, &'a str)>,
        pub xmp: Option<&'a str>,
        /// An image drawn on page 1: (stream, drawn width and height in points).
        pub image: Option<(Stream, f32, f32)>,
        pub user_password: Option<&'a str>,
    }

    /// A one-page PDF (US Letter) built to `spec`.
    pub(crate) fn make_pdf(spec: Spec) -> Vec<u8> {
        let mut doc = Document::with_version("1.7");
        let pages_id = doc.new_object_id();
        let mut resources = Dictionary::new();
        let mut ops = String::from("BT /F1 12 Tf 72 720 Td (Hello) Tj ET\n");
        if let Some((image, w, h)) = spec.image {
            let id = doc.add_object(image);
            resources.set("XObject", dictionary! { "Im0" => id });
            ops = format!("q {w} 0 0 {h} 0 0 cm /Im0 Do Q\n{ops}");
        }
        let content = doc.add_object(Stream::new(Dictionary::new(), ops.into_bytes()));
        let page = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content,
            "Resources" => resources,
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![page.into()],
                "Count" => 1,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            }),
        );
        let mut catalog = dictionary! { "Type" => "Catalog", "Pages" => pages_id };
        if let Some(xmp) = spec.xmp {
            let meta =
                doc.add_object(Stream::new(dictionary! { "Type" => "Metadata", "Subtype" => "XML" }, xmp.into()));
            catalog.set("Metadata", meta);
        }
        let catalog = doc.add_object(catalog);
        doc.trailer.set("Root", catalog);
        if !spec.info.is_empty() {
            let mut info = Dictionary::new();
            for (k, v) in &spec.info {
                info.set(k.as_bytes(), lopdf::text_string(v));
            }
            let info = doc.add_object(info);
            doc.trailer.set("Info", info);
        }
        if let Some(user) = spec.user_password {
            doc.trailer.set(
                "ID",
                vec![Object::string_literal(*b"0123456789abcdef"), Object::string_literal(*b"0123456789abcdef")],
            );
            let state = lopdf::EncryptionState::try_from(lopdf::EncryptionVersion::V2 {
                document: &doc,
                owner_password: "owner",
                user_password: user,
                key_length: 128,
                permissions: lopdf::Permissions::empty(),
            })
            .unwrap();
            doc.encrypt(&state).unwrap();
        }
        let mut out = Vec::new();
        doc.save_to(&mut out).unwrap();
        out
    }

    pub(crate) fn xmp(body: &str) -> String {
        format!(
            r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about="" xmlns:dc="http://purl.org/dc/elements/1.1/"
    xmlns:xmp="http://ns.adobe.com/xap/1.0/" xmlns:pdf="http://ns.adobe.com/pdf/1.3/"
    xmlns:prism="http://prismstandard.org/namespaces/basic/3.0/">
{body}
  </rdf:Description>
 </rdf:RDF>
</x:xmpmeta>
<?xpacket end="w"?>"#
        )
    }

    const XMP_BOOK: &str = r#"
   <dc:title><rdf:Alt><rdf:li xml:lang="fr">Les Dépossédés</rdf:li><rdf:li xml:lang="x-default">The Dispossessed</rdf:li></rdf:Alt></dc:title>
   <dc:creator><rdf:Seq><rdf:li>Ursula K. Le Guin</rdf:li><rdf:li>Second Author</rdf:li></rdf:Seq></dc:creator>
   <dc:subject><rdf:Bag><rdf:li>Science Fiction</rdf:li><rdf:li>Classics</rdf:li></rdf:Bag></dc:subject>
   <dc:publisher><rdf:Bag><rdf:li>Harper &amp; Row</rdf:li></rdf:Bag></dc:publisher>
   <xmp:CreateDate>1974-05-01T10:00:00Z</xmp:CreateDate>
   <xmp:CreatorTool>Adobe InDesign CS6</xmp:CreatorTool>
   <pdf:Producer>Adobe PDF Library 10.0.1</pdf:Producer>
   <prism:isbn>978-0-06-105488-4</prism:isbn>"#;

    fn authors(m: &BookMetadata) -> Vec<&str> {
        m.authors.iter().map(|a| a.name.as_str()).collect()
    }

    #[test]
    fn reads_the_info_dictionary() {
        let pdf = make_pdf(Spec {
            info: vec![
                ("Title", "The Left Hand of Darkness"),
                ("Author", "Le Guin, Ursula K."),
                ("Subject", "Science Fiction"),
                ("Keywords", "Gender; Anarchism, Winter, The Left Hand of Darkness"),
                ("Creator", "Microsoft Word"),
                ("Producer", "Acrobat Distiller 9.0"),
                ("CreationDate", "D:19690301120000+01'00'"),
            ],
            ..Default::default()
        });
        let m = parse(&pdf).unwrap();
        assert_eq!(m.title.as_deref(), Some("The Left Hand of Darkness"));
        assert_eq!(m.authors, [Author { name: "Ursula K. Le Guin".into(), sort: Some("Le Guin, Ursula K.".into()) }]);
        assert_eq!(m.tags, ["Gender", "Anarchism", "Winter", "Science Fiction"]);
        assert_eq!(m.published, NaiveDate::from_ymd_opt(1969, 3, 1));
        assert_eq!((m.publisher, m.cover), (None, None));
    }

    #[test]
    fn reads_xmp() {
        let pdf = make_pdf(Spec { xmp: Some(&xmp(XMP_BOOK)), ..Default::default() });
        let m = parse(&pdf).unwrap();
        assert_eq!(m.title.as_deref(), Some("The Dispossessed"), "x-default first");
        assert_eq!(authors(&m), ["Ursula K. Le Guin", "Second Author"]);
        assert_eq!(m.tags, ["Science Fiction", "Classics"]);
        assert_eq!(m.publisher.as_deref(), Some("Harper & Row"));
        assert_eq!(m.published, NaiveDate::from_ymd_opt(1974, 5, 1));
        assert_eq!(m.identifiers.get("isbn").map(String::as_str), Some("9780061054884"));
    }

    #[test]
    fn xmp_wins_over_info_unless_it_is_junk() {
        let x = xmp(r#"
   <dc:title><rdf:Alt><rdf:li xml:lang="x-default">Microsoft Word - draft3.docx</rdf:li></rdf:Alt></dc:title>
   <dc:creator><rdf:Seq><rdf:li>Ursula K. Le Guin</rdf:li></rdf:Seq></dc:creator>
   <xmp:CreateDate>1974-05-01</xmp:CreateDate>"#);
        let pdf = make_pdf(Spec {
            info: vec![("Title", "The Dispossessed"), ("Author", "Someone Else"), ("CreationDate", "D:20200101")],
            xmp: Some(&x),
            ..Default::default()
        });
        let m = parse(&pdf).unwrap();
        assert_eq!(m.title.as_deref(), Some("The Dispossessed"), "junk XMP title, Info's taken");
        assert_eq!(authors(&m), ["Ursula K. Le Guin"], "XMP's authors");
        assert_eq!(m.published, NaiveDate::from_ymd_opt(1974, 5, 1));
    }

    #[test]
    fn attribute_form_xmp() {
        let x = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
<rdf:Description rdf:about="" xmlns:pdf="http://ns.adobe.com/pdf/1.3/" xmlns:xmp="http://ns.adobe.com/xap/1.0/"
  pdf:Keywords="history, rome" xmp:CreateDate="2001-02-03T04:05:06"/></rdf:RDF></x:xmpmeta>"#;
        let m = parse(&make_pdf(Spec { xmp: Some(x), ..Default::default() })).unwrap();
        assert_eq!(m.tags, ["history", "rome"]);
        assert_eq!(m.published, NaiveDate::from_ymd_opt(2001, 2, 3));
    }

    #[test]
    fn junk_is_dropped() {
        let junk_titles = [
            "Microsoft Word - draft3.docx",
            "untitled",
            "Untitled-2",
            "Document1",
            "",
            "   ",
            "C:\\Users\\jo\\book.pdf",
            "thesis_final_v2",
            "chapter1.indd",
            "PowerPoint Presentation",
            "Adobe Acrobat 9.0",
            "12345",
            "Love Letter_r2",
            "LNT_iTunes:Layout 1",
            "Booklet Layout 2",
            "Remote PCB 1.vsd",
            "Building Your Business-lo-restext",
        ];
        for title in junk_titles {
            let pdf = make_pdf(Spec {
                info: vec![("Title", title), ("Author", "Adobe Acrobat 9.0"), ("Producer", "Adobe Acrobat 9.0")],
                ..Default::default()
            });
            let m = parse(&pdf).unwrap();
            assert_eq!(m.title, None, "{title:?}");
            assert!(m.authors.is_empty(), "the producer as author");
        }
        for author in [
            "jsmith",
            "user01",
            "Administrator",
            "Microsoft Office User",
            "jo@example.com",
            "CORP\\jo",
            "Unknown",
            "Example.com",
            "JSmith",
        ] {
            let m = parse(&make_pdf(Spec { info: vec![("Author", author)], ..Default::default() })).unwrap();
            assert!(m.authors.is_empty(), "{author:?}");
        }
        // A sentence isn't keywords; a long subject isn't a tag.
        let m = parse(&make_pdf(Spec {
            info: vec![
                ("Keywords", "This document describes the quarterly results of the company"),
                ("Subject", "A study of the economic history of the late Roman empire"),
            ],
            ..Default::default()
        }))
        .unwrap();
        assert!(m.tags.is_empty(), "{:?}", m.tags);
    }

    #[test]
    fn real_titles_and_names_are_kept() {
        for title in [
            "Dune",
            "Notes on Fourier.Series",
            "The C++ Programming Language",
            "Untitled Love Story",
            "Adobe Photoshop Classroom in a Book",
            "Ghostscript Documentation",
        ] {
            let m = parse(&make_pdf(Spec { info: vec![("Title", title)], ..Default::default() })).unwrap();
            assert_eq!(m.title.as_deref(), Some(title));
        }
        let m = parse(&make_pdf(Spec { info: vec![("Author", "Plato")], ..Default::default() })).unwrap();
        assert_eq!(authors(&m), ["Plato"]);
        let m = parse(&make_pdf(Spec {
            info: vec![("Author", "Ann Leckie, John Scalzi & bell hooks")],
            ..Default::default()
        }))
        .unwrap();
        assert_eq!(authors(&m), ["Ann Leckie", "John Scalzi", "bell hooks"]);
    }

    #[test]
    fn isbns_are_checked() {
        assert_eq!(valid_isbn("urn:isbn:0-06-105488-7").as_deref(), Some("0061054887"));
        assert_eq!(valid_isbn("ISBN 978-0-06-105488-4").as_deref(), Some("9780061054884"));
        assert_eq!(valid_isbn("080442957X").as_deref(), Some("080442957X"));
        assert_eq!(valid_isbn("9780061054885"), None, "bad check digit");
        assert_eq!(valid_isbn("doi:10.1000/182"), None);
        let x = xmp(r#"<dc:identifier>urn:isbn:0-06-105488-7</dc:identifier>"#);
        let m = parse(&make_pdf(Spec { xmp: Some(&x), ..Default::default() })).unwrap();
        assert_eq!(m.identifiers.get("isbn").map(String::as_str), Some("0061054887"));
    }

    #[test]
    fn owner_password_only_is_read_and_a_user_password_is_not() {
        let info = vec![("Title", "Locked Book")];
        let open = make_pdf(Spec { info: info.clone(), user_password: Some(""), ..Default::default() });
        assert!(open.windows(8).any(|w| w == b"/Encrypt"), "encrypted");
        assert!(!open.windows(11).any(|w| w == b"Locked Book"), "the title is encrypted too");
        assert_eq!(parse(&open).unwrap().title.as_deref(), Some("Locked Book"));
        let locked = make_pdf(Spec { info, user_password: Some("secret"), ..Default::default() });
        assert!(parse(&locked).is_none());
    }

    #[test]
    fn broken_files_are_none() {
        assert!(parse(b"").is_none());
        assert!(parse(b"pdf bytes").is_none());
        assert!(parse(b"%PDF-1.7\n1 0 obj << /Type /Catalog").is_none());
        let pdf = make_pdf(Spec { info: vec![("Title", "Cut Short")], ..Default::default() });
        for len in [20, pdf.len() / 3, pdf.len() / 2] {
            // Whatever lopdf recovers, it doesn't panic.
            let _ = parse(&pdf[..len]);
        }
        // A broken XMP packet is ignored.
        let m = parse(&make_pdf(Spec {
            info: vec![("Title", "Fine")],
            xmp: Some("<x:xmpmeta><broken"),
            ..Default::default()
        }))
        .unwrap();
        assert_eq!(m.title.as_deref(), Some("Fine"));
    }

    pub(crate) fn jpeg(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbImage::from_pixel(w, h, image::Rgb([200, 30, 30]));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Jpeg).unwrap();
        out.into_inner()
    }

    pub(crate) fn jpeg_xobject(w: u32, h: u32) -> Stream {
        Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Image", "Width" => w, "Height" => h,
                "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8, "Filter" => "DCTDecode",
            },
            jpeg(w, h),
        )
        .with_compression(false)
    }

    #[test]
    fn a_full_page_jpeg_is_the_cover() {
        let pdf = make_pdf(Spec { image: Some((jpeg_xobject(600, 800), 612.0, 792.0)), ..Default::default() });
        let cover = parse(&pdf).unwrap().cover.unwrap();
        assert!(cover.starts_with(&[0xFF, 0xD8]));
        assert_eq!(image::load_from_memory(&cover).unwrap().width(), 600);

        // A small picture on a text page isn't.
        let pdf = make_pdf(Spec { image: Some((jpeg_xobject(600, 800), 200.0, 260.0)), ..Default::default() });
        assert_eq!(parse(&pdf).unwrap().cover, None);
    }

    #[test]
    fn full_page_raw_pixels_become_a_png_cover() {
        let (w, h) = (300u32, 400u32);
        let mut s = Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Image", "Width" => w, "Height" => h,
                "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8,
            },
            vec![128u8; (w * h) as usize],
        );
        s.compress().unwrap();
        let pdf = make_pdf(Spec { image: Some((s, 612.0, 792.0)), ..Default::default() });
        let cover = parse(&pdf).unwrap().cover.unwrap();
        let img = image::load_from_memory(&cover).unwrap();
        assert_eq!((img.width(), img.height()), (w, h));
    }

    #[test]
    fn a_cmyk_jpeg_is_skipped() {
        let mut s = jpeg_xobject(600, 800);
        s.dict.set("ColorSpace", "DeviceCMYK");
        let pdf = make_pdf(Spec { image: Some((s, 612.0, 792.0)), ..Default::default() });
        assert_eq!(parse(&pdf).unwrap().cover, None);
    }

    /// What the reader makes of real PDFs, one line each, for eyeballing.
    /// MAGNITIDE_PDF_CORPUS=<dirs> cargo test -p magnitide-core --lib pdf_corpus -- --ignored --nocapture
    #[test]
    #[ignore = "needs MAGNITIDE_PDF_CORPUS"]
    fn pdf_corpus() {
        let dirs = std::env::var("MAGNITIDE_PDF_CORPUS").expect("MAGNITIDE_PDF_CORPUS");
        let mut files = Vec::new();
        let mut todo: Vec<std::path::PathBuf> = std::env::split_paths(&dirs).collect();
        while let Some(dir) = todo.pop() {
            for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                let p = e.path();
                if p.is_dir() {
                    todo.push(p);
                } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("pdf")) {
                    files.push(p);
                }
            }
        }
        files.sort();
        for f in files {
            let start = std::time::Instant::now();
            let m = read(&f);
            let name = f.file_name().unwrap_or_default().to_string_lossy();
            // What the file says, before junk is dropped.
            let raw = std::fs::read(&f).ok().and_then(|d| Document::load_mem(&d).ok()).map(|doc| {
                let (i, x) =
                    (Fields::from_info(&doc), xmp_packet(&doc).map(|x| Fields::from_xmp(&x)).unwrap_or_default());
                format!("info {:?} {:?}, xmp {:?} {:?}", i.title, i.authors, x.title, x.authors)
            });
            println!("  {}", raw.unwrap_or_default());
            match m {
                None => println!("{name}: unreadable"),
                Some(m) => println!(
                    "{name}: {:?} by {:?}; {:?} {:?} {:?} {:?} cover={} {:?}",
                    m.title,
                    authors(&m),
                    m.publisher,
                    m.published,
                    m.tags,
                    m.identifiers,
                    m.cover.as_ref().map_or(0, Vec::len),
                    start.elapsed()
                ),
            }
        }
    }
}
