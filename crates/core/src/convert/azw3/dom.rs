//! A small XHTML tree for building KF8 markup, shaped like lxml's: each
//! element has `text` (before its first child) and `tail` (after it). Text
//! and attribute values stay escaped as in the source, so serializing never
//! re-escapes, and lengths are exact (KF8 positions are byte offsets).
//! Comments, processing instructions and doctypes are dropped, as kindlegen does.

use quick_xml::Reader;
use quick_xml::events::Event;

#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct Element {
    /// As written, possibly prefixed ("svg:image").
    pub name: String,
    /// Raw (escaped) values.
    pub attrs: Vec<(String, String)>,
    pub text: String,
    pub children: Vec<Element>,
    pub tail: String,
}

/// HTML elements that never have content.
const VOID: &[&str] =
    &["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source", "track", "wbr"];

impl Element {
    pub fn new(name: &str) -> Self {
        Self { name: name.to_owned(), ..Default::default() }
    }

    /// The name without a namespace prefix, lower case.
    pub fn local(&self) -> String {
        self.name.rsplit(':').next().unwrap_or(&self.name).to_ascii_lowercase()
    }

    /// An attribute's raw value, by name without prefix.
    pub fn attr(&self, local: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k.rsplit(':').next() == Some(local)).map(|(_, v)| v.as_str())
    }

    pub fn set_attr(&mut self, name: &str, raw_value: &str) {
        match self.attrs.iter_mut().find(|(k, _)| k == name) {
            Some(a) => a.1 = raw_value.to_owned(),
            None => self.attrs.push((name.to_owned(), raw_value.to_owned())),
        }
    }

    pub fn remove_attr(&mut self, local: &str) {
        self.attrs.retain(|(k, _)| k.rsplit(':').next() != Some(local));
    }

    /// Visit every element below this one, depth first, in document order.
    pub fn walk_mut(&mut self, f: &mut impl FnMut(&mut Element)) {
        for c in &mut self.children {
            f(c);
            c.walk_mut(f);
        }
    }

    /// Serialize as KF8 HTML: unprefixed names, no namespace declarations or
    /// namespaced attributes (except `xml:lang` → `lang`), empty non-void
    /// elements written out in full.
    pub fn html(&self, with_tail: bool, out: &mut String) {
        let name = self.local();
        out.push('<');
        out.push_str(&name);
        // `lang` and `xml:lang` often sit side by side: write one, since a
        // repeated attribute is malformed and a Kindle won't index the book.
        let mut written: Vec<&str> = Vec::with_capacity(self.attrs.len());
        for (k, v) in &self.attrs {
            let key = match k.split_once(':') {
                None if k == "xmlns" => continue,
                None => k.as_str(),
                Some(("xml", "lang")) => "lang",
                Some(_) => continue,
            };
            if written.contains(&key) {
                continue;
            }
            written.push(key);
            push_attr(out, key, v);
        }
        if VOID.contains(&name.as_str()) && self.text.is_empty() && self.children.is_empty() {
            out.push_str("/>");
        } else {
            out.push('>');
            out.push_str(&self.text);
            for c in &self.children {
                c.html(true, out);
            }
            out.push_str("</");
            out.push_str(&name);
            out.push('>');
        }
        if with_tail {
            out.push_str(&self.tail);
        }
    }

    /// Serialize as standalone XML (an SVG image): names and attributes as
    /// written, with the SVG and XLink namespaces declared on the root.
    pub fn xml(&self, root: bool, out: &mut String) {
        out.push('<');
        out.push_str(&self.name);
        if root {
            let (prefix, _) = self.name.split_once(':').unwrap_or(("", ""));
            let decl = if prefix.is_empty() { "xmlns".to_owned() } else { format!("xmlns:{prefix}") };
            out.push_str(&format!(
                r#" {decl}="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink""#
            ));
        }
        for (k, v) in &self.attrs {
            if root && (k == "xmlns" || k.starts_with("xmlns:")) {
                continue;
            }
            push_attr(out, k, v);
        }
        if self.text.is_empty() && self.children.is_empty() {
            out.push_str("/>");
        } else {
            out.push('>');
            out.push_str(&self.text);
            for c in &self.children {
                c.xml(false, out);
                out.push_str(&c.tail);
            }
            out.push_str("</");
            out.push_str(&self.name);
            out.push('>');
        }
    }
}

fn push_attr(out: &mut String, key: &str, raw: &str) {
    out.push(' ');
    out.push_str(key);
    out.push_str("=\"");
    // Values may have come single-quoted; inside double quotes, " must be escaped.
    out.push_str(&raw.replace('"', "&quot;"));
    out.push('"');
}

/// Parse an XHTML document into its root element.
pub(super) fn parse(src: &str) -> Result<Element, String> {
    let mut reader = Reader::from_str(src.trim_start_matches('\u{feff}'));
    reader.config_mut().trim_text(false);
    let mut stack: Vec<Element> = vec![Element::new("#document")];
    let append_text = |stack: &mut Vec<Element>, raw: &str| {
        let parent = stack.last_mut().expect("document");
        match parent.children.last_mut() {
            Some(last) => last.tail.push_str(raw),
            None => parent.text.push_str(raw),
        }
    };
    loop {
        let event = reader.read_event().map_err(|e| e.to_string())?;
        match event {
            Event::Start(e) => stack.push(element(&e)?),
            Event::Empty(e) => {
                let el = element(&e)?;
                stack.last_mut().expect("document").children.push(el);
            }
            Event::End(_) => {
                let done = stack.pop().ok_or("unbalanced")?;
                stack.last_mut().ok_or("unbalanced")?.children.push(done);
            }
            Event::Text(t) => append_text(&mut stack, &t),
            Event::GeneralRef(r) => {
                append_text(&mut stack, &entity(&r));
            }
            Event::CData(c) => {
                append_text(&mut stack, &escape_text(&c));
            }
            Event::Eof => break,
            _ => {} // comments, PIs, declarations, doctype
        }
    }
    let doc = stack.pop().ok_or("unbalanced")?;
    if !stack.is_empty() {
        return Err("unclosed elements".into());
    }
    doc.children.into_iter().find(|c| c.local() == "html").ok_or_else(|| "no <html> element".to_owned())
}

/// Parse any XML document (an SVG image) into its root element.
pub(super) fn parse_any(src: &str) -> Result<Element, String> {
    let wrapped = format!("<html>{}</html>", strip_prologue(src));
    let mut first = parse(&wrapped)?.children.into_iter().next().ok_or_else(|| "empty document".to_owned())?;
    first.tail.clear();
    Ok(first)
}

/// A document without its XML declaration and doctype (to embed it).
fn strip_prologue(src: &str) -> &str {
    let mut s = src.trim_start_matches('\u{feff}').trim_start();
    loop {
        if s.starts_with("<?") {
            s = s.find("?>").map_or("", |i| &s[i + 2..]).trim_start();
        } else if s.starts_with("<!DOCTYPE") || s.starts_with("<!doctype") {
            s = s.find('>').map_or("", |i| &s[i + 1..]).trim_start();
        } else if s.starts_with("<!--") {
            s = s.find("-->").map_or("", |i| &s[i + 3..]).trim_start();
        } else {
            return s;
        }
    }
}

fn element(e: &quick_xml::events::BytesStart<'_>) -> Result<Element, String> {
    let name = e.name().as_ref().to_owned();
    let mut attrs = Vec::new();
    for a in e.attributes().with_checks(false) {
        let a = a.map_err(|x| x.to_string())?;
        attrs.push((a.key.as_ref().to_owned(), normalize_entities(&a.value)));
    }
    Ok(Element { name, attrs, ..Default::default() })
}

/// An entity reference as text that needs no DTD: XML's own five and
/// numeric references stay; HTML names become numeric references.
fn entity(name: &str) -> String {
    match name {
        "amp" | "lt" | "gt" | "quot" | "apos" => format!("&{name};"),
        n if n.starts_with('#') => format!("&{n};"),
        n => match html_entity(n) {
            Some(cp) => format!("&#{cp};"),
            None => format!("&amp;{n};"),
        },
    }
}

/// HTML named references inside attribute values, made DTD-free too.
fn normalize_entities(raw: &str) -> String {
    if !raw.contains('&') {
        return raw.to_owned();
    }
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        match after
            .find(';')
            .filter(|j| *j <= 32 && after[..*j].bytes().all(|b| b.is_ascii_alphanumeric() || b == b'#'))
        {
            Some(j) => {
                out.push_str(&entity(&after[..j]));
                rest = &after[j + 1..];
            }
            None => {
                out.push_str("&amp;");
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

pub(super) fn escape_text(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Undo escaping in a raw value (for reading hrefs and ids).
pub(super) fn unescape(raw: &str) -> String {
    if !raw.contains('&') {
        return raw.to_owned();
    }
    let mut out = String::new();
    let mut rest = raw;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        let Some(j) = after.find(';') else {
            out.push('&');
            rest = after;
            continue;
        };
        let name = &after[..j];
        let ch = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            n if n.starts_with("#x") || n.starts_with("#X") => {
                u32::from_str_radix(&n[2..], 16).ok().and_then(char::from_u32)
            }
            n if n.starts_with('#') => n[1..].parse().ok().and_then(char::from_u32),
            n => html_entity(n).and_then(char::from_u32),
        };
        match ch {
            Some(c) => out.push(c),
            None => out.push_str(&rest[i..i + 1 + j + 1]),
        }
        rest = &after[j + 1..];
    }
    out.push_str(rest);
    out
}

/// The HTML named references books actually use.
fn html_entity(name: &str) -> Option<u32> {
    Some(match name {
        "nbsp" => 160,
        "iexcl" => 161,
        "cent" => 162,
        "pound" => 163,
        "yen" => 165,
        "sect" => 167,
        "copy" => 169,
        "laquo" => 171,
        "reg" => 174,
        "deg" => 176,
        "plusmn" => 177,
        "sup2" => 178,
        "sup3" => 179,
        "middot" => 183,
        "raquo" => 187,
        "frac14" => 188,
        "frac12" => 189,
        "frac34" => 190,
        "iquest" => 191,
        "times" => 215,
        "divide" => 247,
        "Agrave" => 192,
        "Aacute" => 193,
        "Auml" => 196,
        "Ccedil" => 199,
        "Egrave" => 200,
        "Eacute" => 201,
        "Ouml" => 214,
        "Uuml" => 220,
        "szlig" => 223,
        "agrave" => 224,
        "aacute" => 225,
        "acirc" => 226,
        "auml" => 228,
        "ccedil" => 231,
        "egrave" => 232,
        "eacute" => 233,
        "ecirc" => 234,
        "euml" => 235,
        "iacute" => 237,
        "icirc" => 238,
        "iuml" => 239,
        "ntilde" => 241,
        "oacute" => 243,
        "ocirc" => 244,
        "ouml" => 246,
        "uacute" => 250,
        "ucirc" => 251,
        "uuml" => 252,
        "oelig" => 339,
        "ndash" => 8211,
        "mdash" => 8212,
        "lsquo" => 8216,
        "rsquo" => 8217,
        "sbquo" => 8218,
        "ldquo" => 8220,
        "rdquo" => 8221,
        "bdquo" => 8222,
        "dagger" => 8224,
        "Dagger" => 8225,
        "bull" => 8226,
        "hellip" => 8230,
        "prime" => 8242,
        "Prime" => 8243,
        "lsaquo" => 8249,
        "rsaquo" => 8250,
        "euro" => 8364,
        "trade" => 8482,
        "larr" => 8592,
        "rarr" => 8594,
        "thinsp" => 8201,
        "ensp" => 8194,
        "emsp" => 8195,
        "zwnj" => 8204,
        "zwj" => 8205,
        "shy" => 173,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_serializes() {
        let src = r#"<?xml version="1.0"?><!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops" lang="en" xml:lang="en"><head><title>T</title></head>
<body><!-- c --><p class='a"b' epub:type="x">One &amp; two&nbsp;&mdash; <em>three</em> four</p><br/><div/></body></html>"#;
        let root = parse(src).unwrap();
        let mut out = String::new();
        root.html(false, &mut out);
        assert_eq!(
            out,
            "<html lang=\"en\"><head><title>T</title></head>\n<body><p class=\"a&quot;b\">One &amp; two&#160;&#8212; <em>three</em> four</p><br/><div></div></body></html>"
        );
    }

    #[test]
    fn keeps_svg_namespaces_for_flows() {
        let root = parse(
            r#"<html><body><svg:svg xmlns:svg="http://www.w3.org/2000/svg" viewBox="0 0 1 1"><svg:image xlink:href="c.jpg"/></svg:svg></body></html>"#,
        )
        .unwrap();
        let svg = &root.children[0].children[0];
        let mut out = String::new();
        svg.xml(true, &mut out);
        assert_eq!(
            out,
            r#"<svg:svg xmlns:svg="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" viewBox="0 0 1 1"><svg:image xlink:href="c.jpg"/></svg:svg>"#
        );
    }

    #[test]
    fn unescapes_values() {
        assert_eq!(unescape("a&amp;b&#233;&#x41;&nbsp;&bogus;"), "a&bé\u{41}\u{a0}&bogus;");
        assert!(parse("<html><body><p>unclosed</body></html>").is_err());
    }
}
