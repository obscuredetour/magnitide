//! EPUB → KEPUB, natively (what kepubify does): Kobo's own format is an
//! EPUB whose text is wrapped in numbered `koboSpan`s, which Kobo's reader
//! uses for page turns, reading stats and highlights.
//!
//! Each content document is passed through byte for byte apart from the
//! inserted markup: body text split into sentences, each in
//! `<span class="koboSpan" id="kobo.P.S">`, images likewise, and the body
//! wrapped in Kobo's `book-columns`/`book-inner` divs. A document that isn't
//! well-formed XHTML is left as it was rather than risk breaking it.

use std::fs::File;
use std::io::{BufReader, BufWriter, Write};
use std::path::Path;

use quick_xml::Reader;
use quick_xml::events::Event;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::metadata::epub::{LocalName, MAX_XML, Res, attr, child, opf_path, parse_xml, read_entry, resolve};

/// Elements whose text is never wrapped.
const SKIP: &[&str] = &["script", "style", "pre", "svg", "math", "textarea", "code"];
/// Elements that start a new "paragraph" in span ids.
const BLOCKS: &[&str] = &[
    "p",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "li",
    "blockquote",
    "dd",
    "dt",
    "td",
    "th",
    "figcaption",
    "div",
    "section",
    "aside",
];

/// What converting did.
#[derive(Debug, Default, PartialEq)]
pub struct KepubReport {
    pub documents: usize,
    /// Documents left unchanged because they couldn't be parsed.
    pub skipped: Vec<String>,
}

/// Convert the EPUB at `src` into a KEPUB at `dest` (which must not exist).
pub fn convert(src: &Path, dest: &Path) -> Res<KepubReport> {
    let mut zip = ZipArchive::new(BufReader::new(File::open(src)?))?;
    let opf_name = opf_path(&mut zip)?;
    let opf = String::from_utf8(read_entry(&mut zip, &opf_name, MAX_XML)?)?;
    let doc = parse_xml(&opf)?;
    let manifest = child(doc.root_element(), "manifest").ok_or("no <manifest>")?;
    let content: Vec<String> = manifest
        .children()
        .filter(|n| n.has_tag_name_local("item"))
        .filter(|n| attr(*n, "media-type").is_some_and(|t| t == "application/xhtml+xml"))
        .filter_map(|n| attr(n, "href"))
        .map(|h| resolve(&opf_name, h))
        .collect();

    let mut report = KepubReport::default();
    let mut out = ZipWriter::new(BufWriter::new(File::create_new(dest)?));
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let result = (|| -> Res<()> {
        for i in 0..zip.len() {
            let name = zip.by_index_raw(i)?.name().to_owned();
            if !content.contains(&name) {
                out.raw_copy_file(zip.by_index_raw(i)?)?;
                continue;
            }
            let bytes = read_entry(&mut zip, &name, MAX_XML * 4)?;
            let converted = std::str::from_utf8(&bytes).ok().and_then(|s| spans(s).ok());
            match converted {
                Some(xhtml) => {
                    out.start_file(name, deflated)?;
                    out.write_all(xhtml.as_bytes())?;
                    report.documents += 1;
                }
                None => {
                    report.skipped.push(name);
                    out.raw_copy_file(zip.by_index_raw(i)?)?;
                }
            }
        }
        out.finish()?.into_inner().map_err(|e| e.into_error())?.sync_all()?;
        Ok(())
    })();
    if let Err(e) = result {
        let _ = std::fs::remove_file(dest);
        return Err(e);
    }
    Ok(report)
}

/// One XHTML document with Kobo spans added. Already-converted documents
/// come back unchanged.
pub(crate) fn spans(src: &str) -> Res<String> {
    if src.contains("koboSpan") {
        return Ok(src.to_owned());
    }
    let mut reader = Reader::from_str(src);
    reader.config_mut().trim_text(false);
    let mut out = String::with_capacity(src.len() + src.len() / 4);
    let mut ids = Ids::default();
    let (mut in_body, mut skip) = (false, 0usize);
    let mut text_from: Option<usize> = None;

    loop {
        let start = reader.buffer_position() as usize;
        let event = reader.read_event()?;
        let end = reader.buffer_position() as usize;
        // Text and entity references arrive in pieces; wrap them together.
        if matches!(event, Event::Text(_) | Event::GeneralRef(_)) {
            text_from.get_or_insert(start);
            continue;
        }
        if let Some(from) = text_from.take() {
            let text = &src[from..start];
            if in_body && skip == 0 { ids.wrap_text(text, &mut out) } else { out.push_str(text) }
        }
        let raw = &src[start..end];
        match event {
            Event::Start(e) => {
                let name = e.local_name().into_inner().to_ascii_lowercase();
                out.push_str(raw);
                if name == "body" {
                    in_body = true;
                    out.push_str(r#"<div class="book-columns"><div class="book-inner">"#);
                } else if in_body && (skip > 0 || SKIP.contains(&name.as_str())) {
                    skip += 1;
                } else if in_body && BLOCKS.contains(&name.as_str()) {
                    ids.paragraph();
                }
            }
            Event::End(e) => {
                let name = e.local_name().into_inner().to_ascii_lowercase();
                if name == "body" {
                    in_body = false;
                    out.push_str("</div></div>");
                } else {
                    skip = skip.saturating_sub(1);
                }
                out.push_str(raw);
            }
            Event::Empty(e) if in_body && skip == 0 && e.local_name().into_inner().eq_ignore_ascii_case("img") => {
                ids.wrap(raw, &mut out);
            }
            Event::Eof => break,
            _ => out.push_str(raw),
        }
    }
    Ok(out)
}

/// Kobo span ids: `kobo.<paragraph>.<segment>`, from 1.
#[derive(Default)]
struct Ids {
    para: usize,
    seg: usize,
}

impl Ids {
    fn paragraph(&mut self) {
        self.para += 1;
        self.seg = 0;
    }

    fn wrap(&mut self, inner: &str, out: &mut String) {
        if self.para == 0 {
            self.paragraph();
        }
        self.seg += 1;
        out.push_str(&format!(r#"<span class="koboSpan" id="kobo.{}.{}">"#, self.para, self.seg));
        out.push_str(inner);
        out.push_str("</span>");
    }

    /// Each sentence in its own span; whitespace-only text is left alone.
    fn wrap_text(&mut self, text: &str, out: &mut String) {
        if text.trim().is_empty() {
            out.push_str(text);
            return;
        }
        // Leading whitespace stays outside, so indentation isn't inside a span.
        let body = text.trim_start();
        out.push_str(&text[..text.len() - body.len()]);
        for sentence in sentences(body) {
            self.wrap(sentence, out);
        }
    }
}

/// Split after `.`, `!`, `?` or `…` (and any closing quotes or brackets)
/// followed by whitespace; each piece keeps its trailing whitespace.
fn sentences(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((_, c)) = chars.next() {
        if !matches!(c, '.' | '!' | '?' | '…') {
            continue;
        }
        while let Some(&(_, q)) = chars.peek() {
            if matches!(q, '"' | '\'' | '”' | '’' | ')' | ']' | '»' | '.' | '!' | '?' | '…') {
                chars.next();
            } else {
                break;
            }
        }
        let mut saw_space = false;
        while let Some(&(_, w)) = chars.peek() {
            if w.is_whitespace() {
                saw_space = true;
                chars.next();
            } else {
                break;
            }
        }
        if saw_space {
            let end = chars.peek().map_or(text.len(), |&(i, _)| i);
            if end < text.len() {
                out.push(&text[start..end]);
                start = end;
            }
        }
    }
    out.push(&text[start..]);
    // Pieces with no letters or digits (". . .", a lone quote) join the one before.
    let mut merged: Vec<&str> = Vec::with_capacity(out.len());
    let mut from = 0;
    for piece in out {
        let end = from + piece.len();
        match merged.last_mut() {
            Some(last) if !piece.chars().any(char::is_alphanumeric) => {
                *last = &text[end - piece.len() - last.len()..end]
            }
            _ => merged.push(piece),
        }
        from = end;
    }
    merged
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use super::*;

    #[test]
    fn splits_sentences() {
        assert_eq!(sentences("One. Two! “Three?” Four"), ["One. ", "Two! ", "“Three?” ", "Four"]);
        assert_eq!(sentences("Mr. X"), ["Mr. ", "X"], "no abbreviation smarts, like kepubify");
        assert_eq!(sentences("3.14 is pi."), ["3.14 is pi."]);
        assert_eq!(sentences("End. "), ["End. "]);
        assert_eq!(sentences(". . . ”"), [". . . ”"], "a spaced ellipsis stays whole");
        assert_eq!(sentences("Wait . . . then go."), ["Wait . . . ", "then go."]);
    }

    #[test]
    fn wraps_body_text_and_images() {
        let xhtml = r#"<?xml version="1.0" encoding="utf-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml"><head><title>T. Title</title><style>p { x: y. z }</style></head>
<body class="c">
  <h1>Chapter One</h1>
  <p>It was dark. Tom &amp; Jerry <em>ran</em> home!</p>
  <p><img src="a.jpg" alt=""/></p>
  <pre>keep. this</pre>
</body></html>"#;
        let out = spans(xhtml).unwrap();
        assert!(out.starts_with(r#"<?xml version="1.0" encoding="utf-8"?>"#));
        assert!(out.contains(r#"<body class="c"><div class="book-columns"><div class="book-inner">"#), "{out}");
        assert!(out.contains(r#"</div></div></body>"#));
        assert!(out.contains(r#"<h1><span class="koboSpan" id="kobo.1.1">Chapter One</span></h1>"#), "{out}");
        assert!(out.contains(r#"<span class="koboSpan" id="kobo.2.1">It was dark. </span><span class="koboSpan" id="kobo.2.2">Tom &amp; Jerry </span><em><span class="koboSpan" id="kobo.2.3">ran</span></em>"#), "{out}");
        assert!(out.contains(r#"<span class="koboSpan" id="kobo.3.1"><img src="a.jpg" alt=""/></span>"#), "{out}");
        assert!(out.contains("<pre>keep. this</pre>"), "pre untouched");
        assert!(out.contains("<title>T. Title</title><style>p { x: y. z }</style>"), "head untouched");
        // Still well-formed, and converting again changes nothing.
        roxmltree::Document::parse_with_options(
            &out,
            roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() },
        )
        .unwrap();
        assert_eq!(spans(&out).unwrap(), out);
    }

    #[test]
    fn broken_documents_are_errors() {
        assert!(spans("<html><body><p>unclosed</body></html>").is_err());
    }

    #[test]
    fn converts_an_epub() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("book.epub");
        let opf = r#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="2.0">
<metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>T</dc:title></metadata>
<manifest><item id="c1" href="ch1.xhtml" media-type="application/xhtml+xml"/>
<item id="c2" href="bad.xhtml" media-type="application/xhtml+xml"/>
<item id="css" href="s.css" media-type="text/css"/></manifest></package>"#;
        {
            let mut z = ZipWriter::new(File::create(&src).unwrap());
            let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            for (name, body) in [
                ("mimetype", "application/epub+zip"),
                (
                    "META-INF/container.xml",
                    r#"<container><rootfiles><rootfile full-path="OEBPS/content.opf"/></rootfiles></container>"#,
                ),
                ("OEBPS/content.opf", opf),
                ("OEBPS/ch1.xhtml", r#"<html xmlns="http://www.w3.org/1999/xhtml"><body><p>Hi.</p></body></html>"#),
                ("OEBPS/bad.xhtml", "<html><body><p>oops</body></html>"),
                ("OEBPS/s.css", "p{}"),
            ] {
                z.start_file(name, stored).unwrap();
                z.write_all(body.as_bytes()).unwrap();
            }
            z.finish().unwrap();
        }
        let dest = dir.path().join("book.kepub");
        let report = convert(&src, &dest).unwrap();
        assert_eq!(report, KepubReport { documents: 1, skipped: vec!["OEBPS/bad.xhtml".into()] });

        let mut z = ZipArchive::new(File::open(&dest).unwrap()).unwrap();
        assert_eq!(z.by_index(0).unwrap().name(), "mimetype");
        let mut ch1 = String::new();
        z.by_name("OEBPS/ch1.xhtml").unwrap().read_to_string(&mut ch1).unwrap();
        assert!(ch1.contains(r#"<span class="koboSpan" id="kobo.1.1">Hi.</span>"#), "{ch1}");
        let mut bad = String::new();
        z.by_name("OEBPS/bad.xhtml").unwrap().read_to_string(&mut bad).unwrap();
        assert_eq!(bad, "<html><body><p>oops</body></html>");
        // The result still reads as a book.
        assert_eq!(crate::metadata::epub::read(&dest).unwrap().title.as_deref(), Some("T"));
        assert!(convert(&src, &dest).is_err(), "never overwrites");
    }
}
