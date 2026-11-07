// Derived from calibre's src/calibre/ebooks/mobi/reader/mobi6.py
// (calibre 9.18).
// Copyright: 2012, Kovid Goyal <kovid@kovidgoyal.net>. License: GPLv3.
// Ported to Rust for Magnitide, 2026.

//! MOBI 6 (the older Mobipocket format) to EPUB. Its text is one long piece
//! of HTML 3-ish markup: links are byte offsets (`filepos`), page breaks are
//! `<mbp:pagebreak/>`, images are record numbers (`recindex`), and layout
//! is in presentational attributes. Offsets become anchors, page breaks
//! become separate files, record numbers become image files, and the
//! attributes become CSS.

use std::collections::{BTreeSet, HashMap};
use std::sync::LazyLock;

use regex::Regex;
use regex::bytes::Regex as BytesRegex;

use super::book::{Header, NULL, Records};
use super::epub::{Book, Page, Resource, TocNode};
use super::html::{self, El, Node, SPLIT};
use super::kf8::image_type;

/// The page all the text is in, before it's split.
const ORIGIN: &str = "index.xhtml";
const STYLESHEET: &str = "styles/style.css";

/// How MOBI readers lay out text by default.
const BASE_CSS: &str = "body { text-align: justify }
blockquote { margin: 0em 0em 0em 2em }
p { margin: 0em; text-indent: 1.5em }
";

pub(super) fn read(records: &Records<'_>, header: &Header, warnings: &mut Vec<String>) -> Result<Book, String> {
    let mut raw = super::book::text(records, header, 0)?;
    if raw.ends_with(b"#") {
        raw.pop();
    }
    let lower_start = String::from_utf8_lossy(&raw[..raw.len().min(300)]).to_ascii_lowercase();
    let plain = header.ancient && !lower_start.contains("<html");
    if plain {
        raw = replace_bytes(&raw, b"\r ", b"\n\n ");
    }
    raw.retain(|&b| b != 0);
    if !header.utf8 {
        raw.retain(|&b| b != 0x1e && b != 0x02);
    }
    let anchored = add_anchors(&raw);
    let mut text = header.decode(&anchored);
    text = cleanup(&text, plain);

    let (images, by_index) = extract_images(records, header, warnings);
    let mut root = html::parse_document(&text);

    // The guide's table of contents and start, before the guide goes.
    let mut guide: Vec<(String, String, String)> = Vec::new();
    root.walk(&mut |e| {
        if e.is("reference")
            && let Some(kind) = e.attr("type")
            && let Some(pos) = e.attr("filepos").and_then(|p| p.trim().parse::<u64>().ok())
        {
            let title = e.attr("title").unwrap_or(kind).to_owned();
            guide.push((kind.to_ascii_lowercase(), title, format!("{ORIGIN}#filepos{pos}")));
        }
    });

    upshift(&mut root, &by_index, header.version);
    let toc_id = guide.iter().find(|g| g.0 == "toc").and_then(|g| g.2.split_once('#')).map(|(_, id)| id.to_owned());
    let mut toc = toc_id.map(|id| toc_after(&root, &id)).unwrap_or_default();
    if toc.is_empty() {
        toc = headings_toc(&mut root);
    }

    // Every page gets the base stylesheet.
    if let Some(head) = root.child_mut("head") {
        head.children.push(Node::El(El::with(
            "link",
            &[("rel", "stylesheet"), ("type", "text/css"), ("href", &format!("../{STYLESHEET}"))],
            vec![],
        )));
    }
    let mut resources = images;
    resources.push(Resource { path: STYLESHEET.into(), media_type: "text/css", data: BASE_CSS.as_bytes().to_vec() });
    let cover =
        header.exth.cover_offset.and_then(|c| by_index.get(&(c as usize + 1))).map(|name| format!("images/{name}"));
    Ok(Book {
        pages: vec![Page { origin: ORIGIN.into(), root }],
        resources,
        toc,
        guide: guide.into_iter().filter(|g| g.0 != "toc" || !g.2.is_empty()).collect(),
        cover,
        rtl: false,
    })
}

fn replace_bytes(hay: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(hay.len());
    let mut i = 0;
    while i < hay.len() {
        if hay[i..].starts_with(from) {
            out.extend_from_slice(to);
            i += from.len();
        } else {
            out.push(hay[i]);
            i += 1;
        }
    }
    out
}

/// Anchors at every byte offset a `filepos` link points to: an
/// `<a id="fileposN">` there, or, when the offset falls inside a tag, a
/// `filepos-id` attribute on that tag.
fn add_anchors(html: &[u8]) -> Vec<u8> {
    static LINK: LazyLock<BytesRegex> =
        LazyLock::new(|| BytesRegex::new(r#"(?i)<[^<>]+filepos=['"]?(\d+)[^<>]*>"#).expect("regex"));
    static END_TAG: LazyLock<BytesRegex> = LazyLock::new(|| BytesRegex::new(r"^<\s*/").expect("regex"));
    static IN_ENTITY: LazyLock<BytesRegex> =
        LazyLock::new(|| BytesRegex::new(r#"&([^;]*?)(<a id="filepos\d+"></a>)([^;]*);"#).expect("regex"));
    let positions: BTreeSet<usize> =
        LINK.captures_iter(html).filter_map(|c| std::str::from_utf8(&c[1]).ok()?.parse().ok()).collect();
    let find = |b: u8, from: usize| html.get(from..).and_then(|s| s.iter().position(|&c| c == b)).map(|p| p + from);
    let mut out = Vec::with_capacity(html.len() + positions.len() * 24);
    let mut pos = 0;
    for &oend in &positions {
        if oend == 0 {
            continue;
        }
        let mut end = oend;
        let l = find(b'<', end);
        let r = find(b'>', end);
        let mut anchor = format!("<a id=\"filepos{oend}\"></a>");
        if let Some(r) = r
            && (l.is_none_or(|l| r < l || l == end))
        {
            let p = html[..(end + 1).min(html.len())].iter().rposition(|&c| c == b'<');
            let self_closing = html.get(..=r).is_some_and(|t| t.ends_with(b"/>"));
            match p {
                Some(p) if pos < end && !END_TAG.is_match(&html[p..r]) && !self_closing => {
                    anchor = format!(" filepos-id=\"filepos{oend}\"");
                    end = r;
                }
                _ => end = r + 1,
            }
        }
        let end = end.min(html.len());
        if end > pos {
            out.extend_from_slice(&html[pos..end]);
            pos = end;
        }
        out.extend_from_slice(anchor.as_bytes());
    }
    out.extend_from_slice(&html[pos.min(html.len())..]);
    IN_ENTITY.replace_all(&out, &b"&$1$3;$2"[..]).into_owned()
}

/// calibre's clean-ups of MOBI markup, before parsing.
fn cleanup(text: &str, plain: bool) -> String {
    static RULES: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
        [
            (r"</([a-zA-Z]+)<", "</$1><"),
            // Tags like <o: p> trip up parsers.
            (r"</?[a-zA-Z]+:\s+[^>]*>", ""),
            (r"<\?xml[^>]*>", ""),
            (r#"<div height="0(pt|px|ex|em|%)?"></div>"#, ""),
            (r"<\s*(/?)\s*o:p[^>]*>", ""),
            // Page breaks become split marks.
            (
                r"(?i)<\s*/?\s*mbp:pagebreak((?:\s+[^/>]*)?)/?\s*>\s*(?:<\s*/?\s*mbp:pagebreak\s*/?\s*>)*",
                "<magnitide-split$1></magnitide-split>",
            ),
            // Inline tags around block tags, swapped so they nest.
            (r"(?i)(?P<styletags>(<(h\d+|i|b|u|em|small|big|strong|tt)>\s*){1,})(?P<para><p[^>]*>)", "$para$styletags"),
            (
                r"(?i)(?P<para></p[^>]*>)\s*(?P<styletags>(</(h\d+|i|b|u|em|small|big|strong|tt)>\s*){1,})",
                "$styletags$para",
            ),
            (r"(?i)(?P<blockquote>(</(blockquote|div)[^>]*>\s*){1,})(?P<para></p[^>]*>)", "$para$blockquote"),
            (r"(?i)(?P<para><p[^>]*>)\s*(?P<blockquote>(<(blockquote|div)[^>]*>\s*){1,})", "$blockquote$para"),
        ]
        .into_iter()
        .map(|(re, to)| (Regex::new(re).expect("regex"), to))
        .collect()
    });
    debug_assert_eq!(SPLIT, "magnitide-split");
    let mut s = text.replace("</</", "</").replace('\u{feff}', "");
    if plain {
        s = format!("<html><p>{}</html>", s.replace("\n\n", "<p>"));
    }
    s = s.replace("\r\n", "\n").replace("> <", ">\n<").replace("<mbp: ", "<mbp:");
    for (re, to) in RULES.iter() {
        if re.is_match(&s) {
            s = re.replace_all(&s, *to).into_owned();
        }
    }
    s
}

/// Image records: by MOBI's image numbering (from 1), the file each became.
fn extract_images(
    records: &Records<'_>,
    header: &Header,
    warnings: &mut Vec<String>,
) -> (Vec<Resource>, HashMap<usize, String>) {
    let mut start = header.first_image as usize;
    if header.first_image == NULL || start > records.len() {
        start = 0; // some old PRC files have broken headers
    }
    let mut processed: Vec<(usize, usize)> = vec![(0, header.text_records + 1)];
    if let Some((h, n)) = header.huff {
        processed.push((h as usize, h as usize + n as usize));
    }
    let mut resources = Vec::new();
    let mut names = HashMap::new();
    let mut index = 0;
    for i in start..records.len() {
        if processed.iter().any(|&(a, b)| i >= a && i < b) {
            continue;
        }
        index += 1;
        let data = records.get(i);
        let Some((ext, media_type)) = image_type(data) else {
            if data.starts_with(b"BM") {
                warnings.push(format!("image {index} is a BMP, which EPUB can't hold; left out"));
            }
            continue;
        };
        let name = format!("{index:05}.{ext}");
        resources.push(Resource { path: format!("images/{name}"), media_type, data: data.to_vec() });
        names.insert(index, name);
    }
    (resources, names)
}

fn ensure_unit(v: &str) -> String {
    if v.ends_with(|c: char| c.is_ascii_digit()) { format!("{v}px") } else { v.to_owned() }
}

/// MOBI's attributes as CSS and XHTML: what calibre's `upshift_markup` does.
fn upshift(root: &mut El, images: &HashMap<usize, String>, version: u32) {
    static DIGIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d").expect("regex"));
    root.walk_mut(&mut |tag| {
        tag.attrs.retain(|(k, _)| !k.contains(':') && k != "xmlns");
        let name = tag.name.clone();
        let mut styles: Vec<String> = Vec::new();
        if let Some(height) = tag.remove_attr("height") {
            let h = height.trim();
            if !h.is_empty() && !h.contains(['<', '>']) && DIGIT.is_match(h) {
                match name.as_str() {
                    "table" | "td" | "tr" => {}
                    "img" => tag.set_attr("height", h),
                    "div" if tag.children.is_empty() => {
                        // A spacer: something in it so it isn't dropped.
                        tag.children.push(Node::Text("\u{a0}".into()));
                        styles.push(format!("height: {}", ensure_unit(h)));
                    }
                    _ => styles.push(format!("margin-top: {}", ensure_unit(h))),
                }
            }
        }
        if let Some(width) = tag.remove_attr("width") {
            let w = width.trim();
            if !w.is_empty() && DIGIT.is_match(w) {
                match name.as_str() {
                    "table" | "td" | "tr" => {}
                    "img" => tag.set_attr("width", w),
                    _ => {
                        styles.push(format!("text-indent: {}", ensure_unit(w)));
                        if let Some(abs) = w.strip_prefix('-') {
                            styles.push(format!("margin-left: {}", ensure_unit(abs)));
                        }
                    }
                }
            }
        }
        if let Some(align) = tag.remove_attr("align").map(|a| a.trim().to_ascii_lowercase()).filter(|a| !a.is_empty()) {
            if name == "img" || name == "table" {
                tag.set_attr("align", &align);
            } else if align == "baseline" {
                styles.push(format!("vertical-align: {align}"));
            } else {
                styles.push(format!("text-align: {align}"));
            }
        }
        match name.as_str() {
            "hr" if version == 1 => {
                tag.name = "div".into();
                styles.push("page-break-before: always; display: block; margin: 0".into());
            }
            "font" => {
                let named = match tag.attr("size").unwrap_or_default().trim().to_ascii_lowercase().as_str() {
                    "xx-small" => Some("1"),
                    "x-small" => Some("2"),
                    "small" => Some("3"),
                    "medium" => Some("4"),
                    "large" => Some("5"),
                    "x-large" => Some("6"),
                    "xx-large" => Some("7"),
                    _ => None,
                };
                if let Some(n) = named {
                    tag.set_attr("size", n);
                }
            }
            "img" => {
                let mut recindex = None;
                for key in ["lowrecindex", "recindex", "hirecindex"] {
                    if let Some(v) = tag.remove_attr(key) {
                        recindex = Some(v);
                    }
                }
                if let Some(n) = recindex.and_then(|r| r.trim().parse::<usize>().ok()) {
                    let file = images.get(&n).cloned().unwrap_or_else(|| format!("{n:05}.jpg"));
                    tag.set_attr("src", &format!("../images/{file}"));
                }
                for key in ["width", "height"] {
                    if let Some(v) = tag.attr(key).map(str::to_ascii_lowercase) {
                        if let Some(em) = v.strip_suffix("em") {
                            // Set for the Kindle's screen: 16px a em at 168 dpi.
                            match em.trim().parse::<f64>() {
                                Ok(n) => tag.set_attr(key, &format!("{}", (n * 16.0 * (168.451 / 72.0)) as i64)),
                                Err(_) => {
                                    tag.remove_attr(key);
                                }
                            }
                        } else if v.ends_with('%') {
                            tag.remove_attr(key);
                        }
                    }
                }
            }
            "pre" if tag.children.is_empty() => tag.name = "div".into(),
            _ => {}
        }
        if let Some(id) = tag.remove_attr("filepos-id") {
            tag.set_attr("id", &id);
            if tag.attr("name").is_some() {
                tag.set_attr("name", &id);
            }
        }
        if let Some(fp) = tag.remove_attr("filepos")
            && let Ok(n) = fp.trim().parse::<u64>()
        {
            tag.set_attr("href", &format!("#filepos{n}"));
        }
        if !styles.is_empty() {
            tag.add_style(&styles.join("; "));
        }
    });

    // SVG in MOBI 6 is unusable; images inside it are kept.
    root.rebuild(&mut |e| {
        if e.name == "svg" {
            let mut imgs = Vec::new();
            e.walk(&mut |i| {
                if i.is("img") && i.attr("src").is_some() {
                    imgs.push(Node::El(El { children: vec![], ..i.clone() }));
                }
            });
            return imgs;
        }
        vec![Node::El(e)]
    });
    forward_anchors(root);
}

/// An empty `<a id="fileposN">` just before a block moves onto the block
/// (or into it, if it has an id already).
fn forward_anchors(el: &mut El) {
    let is_block = |n: &Node| matches!(n, Node::El(e) if matches!(e.name.as_str(), "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "div" | "p"));
    let mut i = 0;
    while i < el.children.len() {
        let movable = matches!(&el.children[i], Node::El(a)
            if a.is("a") && a.children.is_empty() && a.attr("id").is_some_and(|id| id.starts_with("filepos")) && a.attr("href").is_none());
        if movable {
            let mut j = i + 1;
            while matches!(el.children.get(j), Some(Node::Text(t)) if t.trim().is_empty()) {
                j += 1;
            }
            if el.children.get(j).is_some_and(is_block) {
                let Node::El(anchor) = el.children.remove(i) else { unreachable!() };
                let Node::El(block) = &mut el.children[j - 1] else { unreachable!() };
                let id = anchor.attr("id").unwrap_or_default().to_owned();
                if block.attr("id").is_some() {
                    block.children.insert(0, Node::El(anchor));
                } else {
                    block.set_attr("id", &id);
                }
                continue;
            }
        }
        if let Node::El(c) = &mut el.children[i] {
            forward_anchors(c);
        }
        i += 1;
    }
}

/// The table of contents as the book's contents page lists it: the links
/// after the element the guide points to, up to the next page break.
fn toc_after(root: &El, id: &str) -> Vec<TocNode> {
    let mut out = Vec::new();
    let mut reached = false;
    let mut done = false;
    root.walk(&mut |e| {
        if done {
            return;
        }
        if !reached {
            reached = e.attr("id") == Some(id);
            return;
        }
        if e.name == SPLIT && !out.is_empty() {
            done = true;
            return;
        }
        if e.is("a")
            && let Some(href) = e.attr("href").filter(|h| h.starts_with('#'))
        {
            let label = e.text().split_whitespace().collect::<Vec<_>>().join(" ");
            if !label.is_empty() {
                out.push(TocNode { label, href: format!("{ORIGIN}{href}"), children: vec![] });
            }
        }
    });
    out
}

/// A table of contents from the headings, for books without one.
fn headings_toc(root: &mut El) -> Vec<TocNode> {
    let mut out = Vec::new();
    let mut n = 0;
    root.walk_mut(&mut |e| {
        if !matches!(e.name.as_str(), "h1" | "h2") {
            return;
        }
        let label = e.text().split_whitespace().collect::<Vec<_>>().join(" ");
        if label.is_empty() {
            return;
        }
        let id = match e.attr("id") {
            Some(id) => id.to_owned(),
            None => {
                n += 1;
                let id = format!("heading{n}");
                e.set_attr("id", &id);
                id
            }
        };
        out.push(TocNode { label, href: format!("{ORIGIN}#{id}"), children: vec![] });
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchors_go_where_links_point() {
        let src = b"<p>One</p><p>Two <a filepos=0000000010>x</a></p>";
        let out = String::from_utf8(add_anchors(src)).unwrap();
        assert_eq!(out, r#"<p>One</p><p filepos-id="filepos10">Two <a filepos=0000000010>x</a></p>"#);
        // Between tags: an anchor of its own.
        let out = String::from_utf8(add_anchors(b"<p>One</p> <p>Two <a filepos=10>x</a></p>")).unwrap();
        assert!(out.starts_with(r#"<p>One</p><a id="filepos10"></a> <p>"#), "{out}");
        // An offset inside a tag puts the id on the tag.
        let src = b"<p>One</p><p class=x>Two <a filepos=13>x</a></p>";
        let out = String::from_utf8(add_anchors(src)).unwrap();
        assert!(out.contains(r#"<p class=x filepos-id="filepos13">"#), "{out}");
    }

    #[test]
    fn page_breaks_become_split_marks() {
        let s = cleanup("<p>a</p><mbp:pagebreak/><mbp:pagebreak /><p>b</p>", false);
        assert_eq!(s, "<p>a</p><magnitide-split></magnitide-split><p>b</p>");
    }
}
