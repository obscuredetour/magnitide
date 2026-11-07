//! A small owned document tree for turning Kindle markup into EPUB 3
//! XHTML: parsing (as XML when the markup is well formed, else with an
//! HTML5 parser, as browsers would), tidying it into valid XHTML, splitting
//! it into files, and writing it out.

use std::borrow::Cow;
use std::collections::HashSet;

use crate::convert::azw3::dom::html_entity;

pub(super) const XHTML_NS: &str = "http://www.w3.org/1999/xhtml";
pub(super) const SVG_NS: &str = "http://www.w3.org/2000/svg";
pub(super) const MATHML_NS: &str = "http://www.w3.org/1998/Math/MathML";
const XLINK_NS: &str = "http://www.w3.org/1999/xlink";
const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";
const OPS_NS: &str = "http://www.idpf.org/2007/ops";

/// Where a split goes (a page break, or a cut in a long file).
pub(super) const SPLIT: &str = "magnitide-split";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Ns {
    Html,
    Svg,
    Math,
    Other,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Node {
    El(El),
    Text(String),
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct El {
    /// Local name; HTML names lower case.
    pub name: String,
    pub ns: Ns,
    /// Names as written in XHTML ("href", "xml:lang", "xlink:href",
    /// "epub:type"); values unescaped.
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Node>,
}

const VOID: &[&str] =
    &["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source", "track", "wbr"];

impl El {
    pub fn new(name: &str) -> Self {
        Self { name: name.to_owned(), ns: Ns::Html, attrs: vec![], children: vec![] }
    }

    pub fn with(name: &str, attrs: &[(&str, &str)], children: Vec<Node>) -> Self {
        Self {
            name: name.to_owned(),
            ns: Ns::Html,
            attrs: attrs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect(),
            children,
        }
    }

    pub fn is(&self, name: &str) -> bool {
        self.ns == Ns::Html && self.name == name
    }

    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    pub fn set_attr(&mut self, name: &str, value: &str) {
        match self.attrs.iter_mut().find(|(k, _)| k == name) {
            Some(a) => a.1 = value.to_owned(),
            None => self.attrs.push((name.to_owned(), value.to_owned())),
        }
    }

    pub fn remove_attr(&mut self, name: &str) -> Option<String> {
        let i = self.attrs.iter().position(|(k, _)| k == name)?;
        Some(self.attrs.remove(i).1)
    }

    /// Add CSS declarations to the `style` attribute.
    pub fn add_style(&mut self, decls: &str) {
        let decls = decls.trim().trim_end_matches(';');
        if decls.is_empty() {
            return;
        }
        match self.attrs.iter_mut().find(|(k, _)| k == "style") {
            Some((_, s)) if !s.trim().is_empty() => {
                let t = s.trim().trim_end_matches(';').to_owned();
                *s = format!("{t}; {decls}");
            }
            Some((_, s)) => *s = decls.to_owned(),
            None => self.attrs.push(("style".into(), decls.to_owned())),
        }
    }

    /// All the text inside.
    pub fn text(&self) -> String {
        let mut out = String::new();
        fn go(el: &El, out: &mut String) {
            for c in &el.children {
                match c {
                    Node::Text(t) => out.push_str(t),
                    Node::El(e) => go(e, out),
                }
            }
        }
        go(self, &mut out);
        out
    }

    /// Every element below this one, depth first, in document order.
    pub fn walk_mut(&mut self, f: &mut impl FnMut(&mut El)) {
        for c in &mut self.children {
            if let Node::El(e) = c {
                f(e);
                e.walk_mut(f);
            }
        }
    }

    pub fn walk(&self, f: &mut impl FnMut(&El)) {
        for c in &self.children {
            if let Node::El(e) = c {
                f(e);
                e.walk(f);
            }
        }
    }

    pub fn child(&self, name: &str) -> Option<&El> {
        self.children.iter().find_map(|c| match c {
            Node::El(e) if e.is(name) => Some(e),
            _ => None,
        })
    }

    pub fn child_mut(&mut self, name: &str) -> Option<&mut El> {
        self.children.iter_mut().find_map(|c| match c {
            Node::El(e) if e.is(name) => Some(e),
            _ => None,
        })
    }

    /// Rebuild the children: each is kept, replaced, or dropped by `f`,
    /// bottom up (children are processed before their parent sees them).
    pub fn rebuild(&mut self, f: &mut impl FnMut(El) -> Vec<Node>) {
        let old = std::mem::take(&mut self.children);
        for c in old {
            match c {
                Node::El(mut e) => {
                    e.rebuild(f);
                    self.children.extend(f(e));
                }
                t => self.children.push(t),
            }
        }
    }

    /// Whether anything would show: text, or an image, rule, table…
    pub fn has_content(&self) -> bool {
        self.children.iter().any(|c| match c {
            Node::Text(t) => !t.trim().is_empty(),
            Node::El(e) => {
                e.ns != Ns::Html
                    || matches!(e.name.as_str(), "img" | "hr" | "table" | "video" | "audio" | "object" | "iframe")
                    || e.has_content()
            }
        })
    }
}

// Parsing ---------------------------------------------------------------

/// Parse a document: as XML if it's well formed, else as HTML. The result is
/// an `html` element with a `head` and a `body`.
pub(super) fn parse_document(src: &str) -> El {
    let root = parse_xml(src).filter(|r| r.is("html")).unwrap_or_else(|| parse_html(src));
    with_head_and_body(root)
}

/// Parse well-formed XML (HTML named entities allowed) into its root.
pub(super) fn parse_xml(src: &str) -> Option<El> {
    let src = xml_entities(src.trim_start_matches('\u{feff}'));
    let opts = roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() };
    let doc = roxmltree::Document::parse_with_options(&src, opts).ok()?;
    Some(from_xml(doc.root_element()))
}

fn from_xml(n: roxmltree::Node<'_, '_>) -> El {
    let ns = match n.tag_name().namespace() {
        None | Some(XHTML_NS) => Ns::Html,
        Some(SVG_NS) => Ns::Svg,
        Some(MATHML_NS) => Ns::Math,
        Some(_) => Ns::Other,
    };
    let name = if ns == Ns::Html { n.tag_name().name().to_ascii_lowercase() } else { n.tag_name().name().to_owned() };
    let attrs = n
        .attributes()
        .filter_map(|a| {
            let key = match a.namespace() {
                None => a.name().to_owned(),
                Some(XLINK_NS) => format!("xlink:{}", a.name()),
                Some(XML_NS) => format!("xml:{}", a.name()),
                Some(OPS_NS) => format!("epub:{}", a.name()),
                Some(_) => return None,
            };
            Some((key, a.value().to_owned()))
        })
        .collect();
    let children = n
        .children()
        .filter_map(|c| {
            if c.is_element() {
                Some(Node::El(from_xml(c)))
            } else if c.is_text() {
                c.text().map(|t| Node::Text(t.to_owned()))
            } else {
                None
            }
        })
        .collect();
    El { name, ns, attrs, children }
}

/// HTML named entities as numeric references, so an XML parser takes them.
fn xml_entities(src: &str) -> Cow<'_, str> {
    if !src.contains('&') {
        return Cow::Borrowed(src);
    }
    let mut out = String::with_capacity(src.len() + 64);
    let mut rest = src;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        let name_len = after.bytes().take(32).take_while(u8::is_ascii_alphanumeric).count();
        let name = &after[..name_len];
        if name_len > 0
            && after[name_len..].starts_with(';')
            && !matches!(name, "amp" | "lt" | "gt" | "quot" | "apos")
            && let Some(cp) = html_entity(name)
        {
            out.push_str(&format!("&#{cp};"));
            rest = &after[name_len + 1..];
        } else {
            out.push('&');
            rest = after;
        }
    }
    out.push_str(rest);
    Cow::Owned(out)
}

/// Parse HTML as a browser would, whatever state it's in.
pub(super) fn parse_html(src: &str) -> El {
    use html5ever::tendril::TendrilSink;
    let dom = html5ever::parse_document(markup5ever_rcdom::RcDom::default(), Default::default()).one(src);
    let doc = dom.document.clone();
    let root = doc.children.borrow().iter().find_map(|c| match from_html(c) {
        Some(Node::El(e)) if e.is("html") => Some(e),
        _ => None,
    });
    root.unwrap_or_else(|| El::new("html"))
}

fn from_html(h: &markup5ever_rcdom::Handle) -> Option<Node> {
    use markup5ever_rcdom::NodeData;
    match &h.data {
        NodeData::Text { contents } => Some(Node::Text(contents.borrow().to_string())),
        NodeData::Element { name, attrs, .. } => {
            let ns = match &*name.ns {
                XHTML_NS => Ns::Html,
                SVG_NS => Ns::Svg,
                MATHML_NS => Ns::Math,
                _ => Ns::Other,
            };
            let attrs = attrs
                .borrow()
                .iter()
                .filter_map(|a| {
                    let key = match &*a.name.ns {
                        "" => a.name.local.to_string(),
                        XLINK_NS => format!("xlink:{}", a.name.local),
                        XML_NS => format!("xml:{}", a.name.local),
                        _ => return None,
                    };
                    Some((key, a.value.to_string()))
                })
                .collect();
            let children = h.children.borrow().iter().filter_map(from_html).collect();
            Some(Node::El(El { name: name.local.to_string(), ns, attrs, children }))
        }
        _ => None,
    }
}

/// An `html` element with exactly a `head` and a `body`.
fn with_head_and_body(mut root: El) -> El {
    let mut head = El::new("head");
    let mut body: Option<El> = None;
    let mut loose = Vec::new();
    for c in std::mem::take(&mut root.children) {
        match c {
            Node::El(e) if e.is("head") => head.children.extend(e.children),
            Node::El(e) if e.is("body") => match &mut body {
                Some(b) => b.children.extend(e.children),
                None => body = Some(e),
            },
            Node::Text(t) if t.trim().is_empty() => {}
            other => loose.push(other),
        }
    }
    let mut body = body.unwrap_or_else(|| El::new("body"));
    if !loose.is_empty() {
        loose.append(&mut body.children);
        body.children = loose;
    }
    root.name = "html".into();
    root.ns = Ns::Html;
    root.children = vec![Node::El(head), Node::El(body)];
    root
}

// Tidying ---------------------------------------------------------------

/// Turn whatever the book had into valid EPUB 3 XHTML: obsolete elements
/// and attributes become CSS, scripts and other active content go, ids
/// become unique, `head` holds only what's allowed there.
pub(super) fn tidy(root: &mut El, title: &str) {
    let mut head_extra: Vec<Node> = Vec::new();
    {
        let body = root.child_mut("body").expect("with_head_and_body");
        body.attrs.retain(|(k, _)| matches!(k.as_str(), "class" | "style" | "id" | "lang" | "xml:lang" | "dir"));
        body.rebuild(&mut |e| tidy_element(e, &mut head_extra));
        let mut seen = HashSet::new();
        dedupe_ids(body, &mut seen);
    }
    let head = root.child_mut("head").expect("with_head_and_body");
    let old = std::mem::take(&mut head.children);
    let mut title_el: Option<El> = None;
    for c in old.into_iter().chain(head_extra) {
        let Node::El(e) = c else { continue };
        match e.name.as_str() {
            "title" if e.ns == Ns::Html && title_el.is_none() => title_el = Some(e),
            "style" if e.ns == Ns::Html => {
                let css = e.text();
                if !css.trim().is_empty() {
                    head.children.push(Node::El(El::with("style", &[("type", "text/css")], vec![Node::Text(css)])));
                }
            }
            "link" if e.ns == Ns::Html => {
                if e.attr("rel").is_some_and(|r| r.to_ascii_lowercase().contains("stylesheet"))
                    && let Some(href) = e.attr("href").filter(|h| !h.is_empty())
                {
                    head.children.push(Node::El(El::with(
                        "link",
                        &[("rel", "stylesheet"), ("type", "text/css"), ("href", href)],
                        vec![],
                    )));
                }
            }
            "meta" if e.ns == Ns::Html && e.attr("name") == Some("viewport") => {
                head.children.push(Node::El(El::with(
                    "meta",
                    &[("name", "viewport"), ("content", e.attr("content").unwrap_or_default())],
                    vec![],
                )));
            }
            _ => {}
        }
    }
    let text =
        title_el.map(|t| t.text().trim().to_owned()).filter(|t| !t.is_empty()).unwrap_or_else(|| title.to_owned());
    let text = if text.is_empty() { "Untitled".to_owned() } else { text };
    head.children.insert(0, Node::El(El::with("title", &[], vec![Node::Text(text)])));
    root.attrs.retain(|(k, _)| matches!(k.as_str(), "lang" | "xml:lang" | "dir"));
    fix_lang(root);
}

/// Global attributes valid on every HTML element.
fn global_attr(k: &str) -> bool {
    matches!(k, "id" | "class" | "style" | "title" | "lang" | "xml:lang" | "dir" | "epub:type" | "role" | "hidden")
        || (k.starts_with("aria-") && valid_name(k))
        || (k.starts_with("data-") && k.len() > 5 && valid_name(k) && !k.bytes().any(|b| b.is_ascii_uppercase()))
}

/// Attributes valid on particular HTML elements.
fn element_attr(el: &str, k: &str) -> bool {
    match el {
        "a" => matches!(k, "href" | "target" | "rel" | "type" | "hreflang"),
        "area" => matches!(k, "href" | "alt" | "shape" | "coords" | "target" | "rel"),
        "img" => matches!(k, "src" | "alt" | "width" | "height"),
        "ol" => matches!(k, "start" | "reversed" | "type"),
        "li" => k == "value",
        "td" => matches!(k, "colspan" | "rowspan" | "headers"),
        "th" => matches!(k, "colspan" | "rowspan" | "headers" | "scope" | "abbr"),
        "col" | "colgroup" => k == "span",
        "blockquote" | "q" => k == "cite",
        "del" | "ins" => matches!(k, "cite" | "datetime"),
        "time" => k == "datetime",
        "table" => k == "border",
        "video" => matches!(k, "src" | "poster" | "controls" | "width" | "height" | "loop" | "muted" | "preload"),
        "audio" => matches!(k, "src" | "controls" | "loop" | "muted" | "preload"),
        "source" => matches!(k, "src" | "type" | "media"),
        "object" => matches!(k, "data" | "type" | "width" | "height"),
        "param" => matches!(k, "name" | "value"),
        "meter" | "progress" => matches!(k, "value" | "min" | "max" | "low" | "high" | "optimum"),
        "details" => k == "open",
        _ => false,
    }
}

/// One element, its children already tidied: what replaces it.
fn tidy_element(mut e: El, head_extra: &mut Vec<Node>) -> Vec<Node> {
    if e.name == SPLIT {
        return vec![Node::El(e)];
    }
    match e.ns {
        Ns::Other => return e.children,
        Ns::Svg | Ns::Math => {
            e.attrs.retain(|(k, _)| {
                valid_name(k)
                    && !k.starts_with("xmlns")
                    && !k.to_ascii_lowercase().starts_with("on")
                    && (!k.contains(':') || k.starts_with("xlink:") || k.starts_with("xml:"))
            });
            if e.ns == Ns::Svg && e.name == "script" {
                return vec![];
            }
            return vec![Node::El(e)];
        }
        Ns::Html => {}
    }
    if !valid_name(&e.name) || e.name.contains(':') {
        return e.children;
    }
    // Obsolete or unwanted elements.
    match e.name.as_str() {
        "script" | "noscript" | "applet" | "frameset" | "frame" | "noframes" | "noembed" | "iframe" | "embed"
        | "basefont" | "spacer" | "base" | "meta" | "title" | "template" | "form" | "input" | "button" | "select"
        | "textarea" | "canvas" | "head" | "html" | "body" | "guide" | "reference" | "metadata" | "ncx" => {
            return match e.name.as_str() {
                // Content of these is still text worth keeping.
                "form" | "html" | "body" | "head" => e.children,
                _ => vec![],
            };
        }
        "style" => {
            head_extra.push(Node::El(e));
            return vec![];
        }
        "link" => {
            head_extra.push(Node::El(e));
            return vec![];
        }
        "font" => {
            let mut style = String::new();
            if let Some(c) = e.attr("color").filter(|c| css_value_ok(c)) {
                style.push_str(&format!("color: {c}; "));
            }
            if let Some(f) = e.attr("face").filter(|c| css_value_ok(c)) {
                style.push_str(&format!("font-family: {f}; "));
            }
            if let Some(size) = e.attr("size").and_then(font_size) {
                style.push_str(&format!("font-size: {size}; "));
            }
            e.name = "span".into();
            e.attrs.retain(|(k, _)| !matches!(k.as_str(), "color" | "face" | "size"));
            e.add_style(&style);
        }
        "center" => {
            e.name = "div".into();
            e.add_style("text-align: center");
        }
        "big" => {
            e.name = "span".into();
            e.add_style("font-size: larger");
        }
        "strike" => e.name = "s".into(),
        "tt" => e.name = "code".into(),
        "blink" | "marquee" | "nobr" | "layer" | "ilayer" | "multicol" => e.name = "span".into(),
        "acronym" => e.name = "abbr".into(),
        "dir" | "menu" => e.name = "ul".into(),
        "listing" | "xmp" | "plaintext" => e.name = "pre".into(),
        "image" => e.name = "img".into(),
        "country-region" | "place" | "placetype" | "placename" | "state" | "city" | "street" | "address"
            if e.attrs.is_empty() =>
        {
            e.name = "span".into()
        }
        // Kept only if it points at something; else its fallback content.
        "object" if e.attr("data").is_none() => return e.children,
        _ => {}
    }
    if !is_html_element(&e.name) {
        return e.children;
    }

    // Attributes: presentational ones become CSS, the rest must be valid.
    let name = e.name.clone();
    let mut style = String::new();
    let mut keep: Vec<(String, String)> = Vec::new();
    let mut anchor_name = None;
    for (k, v) in std::mem::take(&mut e.attrs) {
        let k = k.to_ascii_lowercase();
        let v = v.trim().to_owned();
        match k.as_str() {
            "align" if css_value_ok(&v) => match (name.as_str(), v.to_ascii_lowercase().as_str()) {
                ("img" | "object", "left" | "right") => style.push_str(&format!("float: {v}; ")),
                ("img" | "object", a) => style.push_str(&format!("vertical-align: {a}; ")),
                ("table", "center") => style.push_str("margin-left: auto; margin-right: auto; "),
                ("table", a) => style.push_str(&format!("float: {a}; ")),
                (_, "baseline" | "top" | "bottom" | "middle") => style.push_str(&format!("vertical-align: {v}; ")),
                (_, a) => style.push_str(&format!("text-align: {a}; ")),
            },
            "valign" if css_value_ok(&v) => style.push_str(&format!("vertical-align: {v}; ")),
            "bgcolor" if css_value_ok(&v) => style.push_str(&format!("background-color: {v}; ")),
            "color" if css_value_ok(&v) => style.push_str(&format!("color: {v}; ")),
            "nowrap" => style.push_str("white-space: nowrap; "),
            "clear" if name == "br" && css_value_ok(&v) => {
                let c = if v.eq_ignore_ascii_case("all") { "both" } else { &v };
                style.push_str(&format!("clear: {c}; "));
            }
            "type" if name == "ul" && css_value_ok(&v) => style.push_str(&format!("list-style-type: {v}; ")),
            "width" | "height" if matches!(name.as_str(), "img" | "video" | "object") => {
                if !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()) {
                    keep.push((k, v));
                } else if let Some(len) = css_length(&v) {
                    style.push_str(&format!("{k}: {len}; "));
                }
            }
            "width" | "height" => {
                if let Some(len) = css_length(&v) {
                    style.push_str(&format!("{k}: {len}; "));
                }
            }
            "border" if name == "table" => {
                if v.is_empty() || v == "1" {
                    keep.push((k, v));
                } else if v.parse::<u32>().is_ok_and(|n| n > 1) {
                    keep.push((k, "1".into()));
                }
            }
            "name" if name == "a" => anchor_name = Some(v),
            "dir" => {
                if matches!(v.as_str(), "ltr" | "rtl" | "auto") {
                    keep.push((k, v));
                }
            }
            "lang" | "xml:lang" => {
                if !v.is_empty() && v.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
                    keep.push((k, v));
                }
            }
            "id" => {
                let id = v.split_whitespace().collect::<Vec<_>>().join("_");
                if !id.is_empty() {
                    keep.push((k, id));
                }
            }
            "style" => {
                if !v.is_empty() {
                    keep.push((k, v));
                }
            }
            "colspan" | "rowspan" | "span" | "start" | "value" => {
                if v.parse::<i64>().is_ok_and(|n| n >= 0 || k == "start" || k == "value") {
                    keep.push((k, v));
                }
            }
            "type" if name == "ol" => {
                if matches!(v.as_str(), "1" | "a" | "A" | "i" | "I") {
                    keep.push((k, v));
                } else if css_value_ok(&v) {
                    style.push_str(&format!("list-style-type: {v}; "));
                }
            }
            "href" | "src" | "data" | "poster" | "cite" => {
                if !v.is_empty() {
                    keep.push((k, v));
                }
            }
            "target" => {
                if !v.is_empty() {
                    keep.push((k, v));
                }
            }
            _ if global_attr(&k) || element_attr(&name, &k) => keep.push((k, v)),
            _ => {}
        }
    }
    if let Some(n) = anchor_name.map(|n| n.split_whitespace().collect::<Vec<_>>().join("_"))
        && !n.is_empty()
        && !keep.iter().any(|(k, _)| k == "id")
    {
        keep.push(("id".into(), n));
    }
    e.attrs = keep;
    e.add_style(&style);
    // Boolean attributes in XHTML take their own name as value.
    for (k, v) in &mut e.attrs {
        if matches!(k.as_str(), "hidden" | "reversed" | "controls" | "loop" | "muted" | "open") {
            *v = k.clone();
        }
    }
    if let Some(s) = e.attr("style")
        && s.trim().is_empty()
    {
        e.remove_attr("style");
    }
    match e.name.as_str() {
        "img" => {
            if e.attr("src").is_none_or(|s| s.trim().is_empty()) {
                return vec![];
            }
            if e.attr("alt").is_none() {
                e.attrs.push(("alt".into(), String::new()));
            }
        }
        "a" if e.attr("href").is_some_and(|h| h.trim().is_empty()) => {
            e.remove_attr("href");
        }
        _ => {}
    }
    if VOID.contains(&e.name.as_str()) {
        // Anything wrongly inside a void element goes after it.
        let rest = std::mem::take(&mut e.children);
        let mut out = vec![Node::El(e)];
        out.extend(rest);
        return out;
    }
    vec![Node::El(e)]
}

/// `lang` and `xml:lang` must agree; one is copied to the other.
fn fix_lang(el: &mut El) {
    fn go(e: &mut El) {
        let lang = e.attr("xml:lang").or(e.attr("lang")).map(str::to_owned);
        if let Some(l) = lang {
            e.set_attr("lang", &l);
            e.set_attr("xml:lang", &l);
        }
        for c in &mut e.children {
            if let Node::El(c) = c
                && c.ns == Ns::Html
            {
                go(c);
            }
        }
    }
    go(el);
}

fn dedupe_ids(el: &mut El, seen: &mut HashSet<String>) {
    for c in &mut el.children {
        if let Node::El(e) = c {
            if let Some(id) = e.attr("id").map(str::to_owned)
                && !seen.insert(id)
            {
                e.remove_attr("id");
            }
            dedupe_ids(e, seen);
        }
    }
}

/// HTML `font size`: 1–7, or relative (+1, -2).
fn font_size(size: &str) -> Option<&'static str> {
    const SIZES: [&str; 7] = ["x-small", "small", "medium", "large", "x-large", "xx-large", "xxx-large"];
    let s = size.trim();
    let n: i32 = s.trim_start_matches('+').parse().ok()?;
    let abs = if s.starts_with('+') || s.starts_with('-') { 3 + n } else { n };
    Some(SIZES[(abs.clamp(1, 7) - 1) as usize])
}

/// A length for CSS: bare numbers are pixels; percentages and units stay.
pub(super) fn css_length(v: &str) -> Option<String> {
    let v = v.trim();
    let num_end = v.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-')).unwrap_or(v.len());
    if num_end == 0 || v[..num_end].parse::<f64>().is_err() {
        return None;
    }
    let unit = &v[num_end..];
    match unit {
        "" => Some(format!("{v}px")),
        "%" | "px" | "em" | "ex" | "pt" | "pc" | "cm" | "mm" | "in" | "rem" => Some(v.to_owned()),
        _ => None,
    }
}

/// A value that can't break out of a CSS declaration.
fn css_value_ok(v: &str) -> bool {
    !v.is_empty() && !v.contains([';', '{', '}', '<', '>', '"', '\\'])
}

fn is_html_element(name: &str) -> bool {
    matches!(
        name,
        "a" | "abbr"
            | "address"
            | "area"
            | "article"
            | "aside"
            | "audio"
            | "b"
            | "bdi"
            | "bdo"
            | "blockquote"
            | "br"
            | "caption"
            | "cite"
            | "code"
            | "col"
            | "colgroup"
            | "data"
            | "dd"
            | "del"
            | "details"
            | "dfn"
            | "div"
            | "dl"
            | "dt"
            | "em"
            | "figcaption"
            | "figure"
            | "footer"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "header"
            | "hgroup"
            | "hr"
            | "i"
            | "img"
            | "ins"
            | "kbd"
            | "label"
            | "li"
            | "main"
            | "map"
            | "mark"
            | "meter"
            | "nav"
            | "object"
            | "ol"
            | "p"
            | "param"
            | "picture"
            | "pre"
            | "progress"
            | "q"
            | "rp"
            | "rt"
            | "ruby"
            | "s"
            | "samp"
            | "section"
            | "small"
            | "source"
            | "span"
            | "strong"
            | "sub"
            | "summary"
            | "sup"
            | "table"
            | "tbody"
            | "td"
            | "tfoot"
            | "th"
            | "thead"
            | "time"
            | "tr"
            | "track"
            | "u"
            | "ul"
            | "var"
            | "video"
            | "wbr"
    )
}

/// A valid XML name without a prefix (or with the ones XHTML uses).
fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else { return false };
    (first.is_alphabetic() || first == '_')
        && chars.all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'))
        && name.matches(':').count() <= 1
        && !name.ends_with(':')
}

// Splitting -------------------------------------------------------------

/// Mark places to cut a body so no file is much over `limit` bytes:
/// between its blocks, going into a single long wrapper if need be.
pub(super) fn mark_long(el: &mut El, limit: usize) {
    let sizes: Vec<usize> = el
        .children
        .iter()
        .map(|c| match c {
            Node::Text(t) => t.len(),
            Node::El(e) => serialized_len(e),
        })
        .collect();
    if sizes.iter().sum::<usize>() <= limit {
        return;
    }
    let old = std::mem::take(&mut el.children);
    let mut acc = 0;
    for (c, size) in old.into_iter().zip(sizes) {
        let splittable = |e: &El| {
            e.ns == Ns::Html
                && matches!(e.name.as_str(), "div" | "section" | "article" | "blockquote" | "main" | "body")
        };
        match c {
            Node::El(mut e) if size > limit && splittable(&e) => {
                if acc > 0 {
                    el.children.push(Node::El(El::new(SPLIT)));
                }
                mark_long(&mut e, limit);
                el.children.push(Node::El(e));
                el.children.push(Node::El(El::new(SPLIT)));
                acc = 0;
            }
            c => {
                if acc > 0 && acc + size > limit {
                    el.children.push(Node::El(El::new(SPLIT)));
                    acc = 0;
                }
                acc += size;
                el.children.push(c);
            }
        }
    }
}

fn serialized_len(e: &El) -> usize {
    let mut n = e.name.len() * 2 + 5 + e.attrs.iter().map(|(k, v)| k.len() + v.len() + 4).sum::<usize>();
    for c in &e.children {
        n += match c {
            Node::Text(t) => t.len(),
            Node::El(e) => serialized_len(e),
        };
    }
    n
}

fn has_split(e: &El) -> bool {
    e.children.iter().any(|c| matches!(c, Node::El(c) if c.name == SPLIT || has_split(c)))
}

/// Cut a body at its split marks. The parts after the first get copies of
/// the elements cut through (without their ids). Parts with nothing to
/// show are merged into the next.
pub(super) fn split(body: El) -> Vec<El> {
    fn go(el: El) -> Vec<El> {
        let shell = |e: &El, first: bool| El {
            name: e.name.clone(),
            ns: e.ns,
            attrs: e.attrs.iter().filter(|(k, _)| first || k != "id").cloned().collect(),
            children: vec![],
        };
        let mut pieces = vec![shell(&el, true)];
        let template = shell(&el, false);
        for c in el.children {
            match c {
                Node::El(m) if m.name == SPLIT => {
                    pieces.push(template.clone());
                    // An anchor the break carried starts the next part.
                    if let Some(id) = m.attr("id") {
                        let a = El::with("a", &[("id", id)], vec![]);
                        pieces.last_mut().expect("pushed").children.push(Node::El(a));
                    }
                }
                Node::El(e) if has_split(&e) => {
                    let mut subs = go(e).into_iter();
                    if let Some(first) = subs.next() {
                        pieces.last_mut().expect("one").children.push(Node::El(first));
                    }
                    for s in subs {
                        let mut p = template.clone();
                        p.children.push(Node::El(s));
                        pieces.push(p);
                    }
                }
                c => pieces.last_mut().expect("one").children.push(c),
            }
        }
        pieces
    }
    if !has_split(&body) {
        return vec![body];
    }
    let pieces = go(body);
    // Empty parts (a run of page breaks) join the next, keeping their anchors.
    let mut out: Vec<El> = Vec::new();
    let mut carry: Vec<Node> = Vec::new();
    for mut p in pieces {
        if !p.has_content() {
            carry.append(&mut p.children);
            continue;
        }
        if !carry.is_empty() {
            let mut kids = std::mem::take(&mut carry);
            kids.append(&mut p.children);
            p.children = kids;
        }
        out.push(p);
    }
    match out.last_mut() {
        Some(last) => last.children.append(&mut carry),
        None => {
            let mut b = El::new("body");
            b.children = carry;
            out.push(b);
        }
    }
    out
}

// Writing ---------------------------------------------------------------

/// The document as an XHTML file.
pub(super) fn to_xhtml(root: &El) -> String {
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<!DOCTYPE html>\n");
    write_el(root, &mut out, true, Ns::Html);
    out.push('\n');
    out
}

/// An SVG (or other XML) document on its own.
pub(super) fn to_xml(root: &El) -> String {
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n");
    write_el(root, &mut out, false, Ns::Html);
    out.push('\n');
    out
}

fn write_el(e: &El, out: &mut String, root: bool, parent: Ns) {
    out.push('<');
    out.push_str(&e.name);
    if root && e.ns == Ns::Html {
        out.push_str(&format!(" xmlns=\"{XHTML_NS}\" xmlns:epub=\"{OPS_NS}\""));
    } else if e.ns != parent {
        match e.ns {
            Ns::Svg => out.push_str(&format!(" xmlns=\"{SVG_NS}\" xmlns:xlink=\"{XLINK_NS}\"")),
            Ns::Math => out.push_str(&format!(" xmlns=\"{MATHML_NS}\"")),
            Ns::Html => out.push_str(&format!(" xmlns=\"{XHTML_NS}\"")),
            Ns::Other => {}
        }
    }
    let mut written: HashSet<&str> = HashSet::new();
    for (k, v) in &e.attrs {
        if !valid_name(k) || k.starts_with("xmlns") || !written.insert(k) {
            continue;
        }
        if k.contains(':') && !(k.starts_with("xml:") || k.starts_with("xlink:") || k.starts_with("epub:")) {
            continue;
        }
        out.push(' ');
        out.push_str(k);
        out.push_str("=\"");
        escape(v, true, out);
        out.push('"');
    }
    let void = e.ns == Ns::Html && VOID.contains(&e.name.as_str());
    if e.children.is_empty() && (void || e.ns != Ns::Html) {
        out.push_str("/>");
        return;
    }
    out.push('>');
    if !void {
        for c in &e.children {
            match c {
                Node::Text(t) => escape(t, false, out),
                Node::El(c) => write_el(c, out, false, e.ns),
            }
        }
    }
    out.push_str("</");
    out.push_str(&e.name);
    out.push('>');
}

/// Escape for XML, dropping characters XML doesn't allow.
fn escape(s: &str, attr: bool, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if attr => out.push_str("&quot;"),
            '\n' if attr => out.push_str("&#10;"),
            '\t' | '\n' | '\r' => out.push(c),
            c if (c as u32) < 0x20 || c == '\u{FFFE}' || c == '\u{FFFF}' => {}
            c => out.push(c),
        }
    }
}

pub(super) fn escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    escape(s, true, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tidy_html(src: &str) -> String {
        let mut doc = parse_document(src);
        tidy(&mut doc, "T");
        to_xhtml(&doc)
    }

    #[test]
    fn soup_becomes_xhtml() {
        let x = tidy_html(
            "<html><body><p align=center>One<br>two &nbsp;<font size=+1 color=red>big</font><p>Three \
             <img recindex=1><img src='a.jpg' width=50%><a name=here></a><script>x()</script>",
        );
        assert!(x.contains(r#"<p style="text-align: center">One<br/>two "#), "{x}");
        assert!(x.contains(r#"<span style="color: red; font-size: large">big</span>"#), "{x}");
        assert!(x.contains(r#"<img src="a.jpg" style="width: 50%" alt=""/>"#), "{x}");
        assert!(x.contains(r#"<a id="here"></a>"#), "{x}");
        assert!(!x.contains("recindex") && !x.contains("script"), "{x}");
        assert!(x.contains("<title>T</title>"), "{x}");
        let opts = roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() };
        roxmltree::Document::parse_with_options(&x, opts).expect("well formed");
    }

    #[test]
    fn well_formed_markup_is_read_as_xml() {
        // An empty div written as XML stays empty (an HTML parser would nest what follows inside it).
        let x = tidy_html(
            r#"<?xml version="1.0"?><html xmlns="http://www.w3.org/1999/xhtml"><head><title>Ch</title></head><body><div class="a"/><p>After&nbsp;it</p><svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" viewBox="0 0 1 1"><image xlink:href="../images/1.jpg"/></svg></body></html>"#,
        );
        assert!(x.contains(r#"<div class="a"></div><p>After"#), "{x}");
        assert!(x.contains("\u{a0}it"), "{x}");
        assert!(x.contains(r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" viewBox="0 0 1 1"><image xlink:href="../images/1.jpg"/></svg>"#), "{x}");
        assert!(x.contains("<title>Ch</title>"));
    }

    #[test]
    fn ids_are_unique_and_lang_agrees() {
        let x = tidy_html(r#"<html lang="fr"><body><p id="a">1</p><p id="a">2</p><p id="b c">3</p></body></html>"#);
        assert_eq!(x.matches(r#"id="a""#).count(), 1, "{x}");
        assert!(x.contains(r#"id="b_c""#), "{x}");
        assert!(x.contains(r#"lang="fr" xml:lang="fr""#), "{x}");
    }

    #[test]
    fn splits_at_marks() {
        let doc = parse_html(&format!(
            "<body><div class=w id=w><p>one</p><{SPLIT} id=f9></{SPLIT}><p>two</p></div><{SPLIT}></{SPLIT}><{SPLIT}></{SPLIT}><p>three</p></body>"
        ));
        let body = doc.child("body").unwrap().clone();
        let parts = split(body);
        assert_eq!(parts.len(), 3);
        let html: Vec<String> = parts
            .iter()
            .map(|p| {
                let mut s = String::new();
                write_el(p, &mut s, false, Ns::Html);
                s
            })
            .collect();
        assert_eq!(html[0], r#"<body><div class="w" id="w"><p>one</p></div></body>"#);
        assert_eq!(html[1], r#"<body><div class="w"><a id="f9"></a><p>two</p></div></body>"#);
        assert_eq!(html[2], "<body><p>three</p></body>");
    }

    #[test]
    fn long_bodies_are_cut_between_blocks() {
        let paras: String = (0..100).map(|i| format!("<p>Paragraph {i} with some words.</p>")).collect();
        let mut body = parse_html(&format!("<body><div>{paras}</div></body>")).child("body").unwrap().clone();
        mark_long(&mut body, 1000);
        let parts = split(body);
        assert!(parts.len() >= 3, "{}", parts.len());
        assert!(parts.iter().all(|p| p.text().contains("Paragraph")));
    }
}
