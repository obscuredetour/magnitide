// Derived from calibre's src/calibre/ebooks/mobi/writer8/skeleton.py (calibre 9.15).
// Copyright: 2012, Kovid Goyal <kovid@kovidgoyal.net>. License: GPLv3.
// Ported to Rust for Magnitide, 2026.

//! Splitting each HTML file into a skeleton and fragments ("chunks"), the
//! way kindlegen does (ported from calibre's writer8/skeleton.py).
//!
//! The KF8 text holds, per file, the skeleton (the document with most of its
//! body taken out) followed by its chunks; the chunk table says where each
//! chunk goes back in. Positions inside the book ("kindle:pos:fid:…") are a
//! chunk number and an offset within it.

use std::collections::HashMap;

use super::dom::Element;

/// Chunks are kept to about this size.
pub(super) const CHUNK_SIZE: usize = 8192;

/// Tags kindlegen gives an `aid` attribute (so they can be pointed at).
pub(super) const AID_ABLE: &[&str] = &[
    "a",
    "abbr",
    "address",
    "article",
    "aside",
    "audio",
    "b",
    "bdo",
    "blockquote",
    "body",
    "button",
    "cite",
    "code",
    "dd",
    "del",
    "details",
    "dfn",
    "div",
    "dl",
    "dt",
    "em",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hgroup",
    "i",
    "ins",
    "kbd",
    "label",
    "legend",
    "li",
    "map",
    "mark",
    "meter",
    "nav",
    "ol",
    "output",
    "p",
    "pre",
    "progress",
    "q",
    "rp",
    "rt",
    "samp",
    "section",
    "select",
    "small",
    "span",
    "strong",
    "sub",
    "summary",
    "sup",
    "textarea",
    "time",
    "ul",
    "var",
    "video",
];

/// Digits for base 32 numbers in KF8 references.
pub(super) fn base32(mut n: u64, min_digits: usize) -> String {
    const DIGITS: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUV";
    let mut out = Vec::new();
    while n > 0 {
        out.push(DIGITS[(n % 32) as usize]);
        n /= 32;
    }
    while out.len() < min_digits.max(1) {
        out.push(b'0');
    }
    out.reverse();
    String::from_utf8(out).expect("ascii")
}

#[derive(Debug, Clone)]
struct Chunk {
    raw: String,
    starts_tags: Vec<String>,
    ends_tags: Vec<String>,
    insert_pos: usize,
    selector: String,
}

/// A skeleton table entry.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Skel {
    pub file_number: usize,
    pub chunk_count: usize,
    pub start_pos: usize,
    pub length: usize,
}

/// A chunk table entry.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct ChunkEntry {
    pub insert_pos: usize,
    pub selector: String,
    pub file_number: usize,
    pub sequence_number: usize,
    pub start_pos: usize,
    pub length: usize,
}

/// Where an aid ended up: (chunk number, offset in that chunk, offset in the text).
pub(super) type AidPos = (usize, usize, usize);

pub(super) struct Chunked {
    /// The text flow: each file's skeleton followed by its chunks.
    pub text: String,
    pub skels: Vec<Skel>,
    pub chunks: Vec<ChunkEntry>,
    pub aid_offsets: HashMap<String, AidPos>,
}

/// Chunk `files` (each an `<html>` root, aids already assigned). Internal
/// links are `placeholders` (placeholder → target aid), filled in here.
pub(super) fn chunk_files(files: Vec<Element>, placeholders: &HashMap<String, String>) -> Result<Chunked, String> {
    struct File {
        skeleton: String,
        chunks: Vec<Chunk>,
    }
    let mut out_files = Vec::new();
    for mut root in files {
        root.tail = "\n".into();
        let body = root.children.iter_mut().find(|c| c.local() == "body").ok_or("a file has no <body>")?;
        body.tail = "\n".into();
        let mut chunks = Vec::new();
        let mut selector = String::new();
        step_into(body, &mut chunks, &mut selector)?;
        let chunks = merge_small(chunks);

        let mut skeleton = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        let mut html = String::new();
        root.html(true, &mut html);
        skeleton.push_str(&html.replacen("<html", "<html xmlns=\"http://www.w3.org/1999/xhtml\"", 1));

        // Where each stepped-into tag's start and end sit in the skeleton.
        let mut metrics: HashMap<String, (usize, usize)> = HashMap::new();
        collect_metrics(&root, &mut metrics);
        let body_offset = skeleton.find("<body").ok_or("no <body> in the skeleton")?;
        let mut chunks = chunks;
        let mut pos = body_offset;
        for c in &mut chunks {
            for t in &c.starts_tags {
                pos += metrics.get(t).ok_or("missing metrics")?.0;
            }
            c.insert_pos = pos;
            pos += c.raw.len();
            for t in &c.ends_tags {
                pos += metrics.get(t).ok_or("missing metrics")?.1;
            }
        }
        out_files.push(File { skeleton, chunks });
    }

    // Tables.
    let mut skels = Vec::new();
    let mut table = Vec::new();
    let mut start = 0;
    let mut seq = 0;
    for (n, f) in out_files.iter().enumerate() {
        let total = f.skeleton.len() + f.chunks.iter().map(|c| c.raw.len()).sum::<usize>();
        skels.push(Skel { file_number: n, chunk_count: f.chunks.len(), start_pos: start, length: f.skeleton.len() });
        let mut cp = 0;
        for c in &f.chunks {
            table.push(ChunkEntry {
                insert_pos: c.insert_pos + start,
                selector: c.selector.clone(),
                file_number: n,
                sequence_number: seq,
                start_pos: cp,
                length: c.raw.len(),
            });
            cp += c.raw.len();
            seq += 1;
        }
        start += total;
    }

    // The documents as a reader rebuilds them: offsets of every aid.
    let mut rebuilt = String::new();
    let mut text = String::new();
    for (n, f) in out_files.iter().enumerate() {
        let mut doc = f.skeleton.clone();
        for c in &f.chunks {
            doc.insert_str(c.insert_pos, &c.raw);
        }
        // A Kindle parses pages as XML and silently skips a book with a
        // malformed one, so fail here instead.
        if let Err(e) = roxmltree::Document::parse(&doc) {
            return Err(format!("page {} isn't well-formed XML: {e}", n + 1));
        }
        rebuilt.push_str(&doc);
        text.push_str(&f.skeleton);
        for c in &f.chunks {
            text.push_str(&c.raw);
        }
    }
    let mut aid_offsets = HashMap::new();
    for (offset, aid) in aid_positions(&rebuilt) {
        let mut found = None;
        for (i, c) in table.iter().enumerate() {
            if c.insert_pos <= offset && offset < c.insert_pos + c.length {
                found = Some((c.sequence_number, offset - c.insert_pos, offset));
                break;
            }
            if c.insert_pos > offset {
                // In the skeleton, not a chunk: the chunk that follows.
                found = Some((c.sequence_number, 0, offset));
                break;
            }
            if i + 1 == table.len() {
                found = Some((c.sequence_number, offset.saturating_sub(c.insert_pos), offset));
            }
        }
        if let Some(f) = found {
            aid_offsets.insert(aid, f);
        }
    }

    // Internal links: same length placeholders, so nothing shifts.
    for (placeholder, aid) in placeholders {
        if let Some((pos, fid, _)) = aid_offsets.get(aid) {
            let target = format!("kindle:pos:fid:{}:off:{}", base32(*pos as u64, 4), base32(*fid as u64, 10));
            debug_assert_eq!(target.len(), placeholder.len());
            text = text.replace(placeholder.as_str(), &target);
        }
    }

    Ok(Chunked { text, skels, chunks: table, aid_offsets })
}

/// Split a tag's content into chunks, stepping into children too big to be
/// one chunk (if they have an aid). Children taken into chunks leave the tree.
fn step_into(tag: &mut Element, chunks: &mut Vec<Chunk>, selector: &mut String) -> Result<(), String> {
    let aid = tag.attr("aid").ok_or("stepped into a tag without an aid")?.to_owned();
    *selector = format!("P-//*[@aid='{aid}']");
    let first = chunks.len();
    if !tag.text.trim().is_empty() {
        chunks.extend(chunk_text(&tag.text, selector));
        tag.text.clear();
    }
    let mut kept = Vec::new();
    for mut child in std::mem::take(&mut tag.children) {
        let mut raw = String::new();
        child.html(false, &mut raw);
        if raw.len() > CHUNK_SIZE && child.attr("aid").is_some() {
            step_into(&mut child, chunks, selector)?;
            if !child.tail.trim().is_empty() {
                chunks.extend(chunk_text(&child.tail, selector));
                child.tail.clear();
            }
            kept.push(child);
        } else {
            chunks.push(Chunk {
                raw,
                starts_tags: vec![],
                ends_tags: vec![],
                insert_pos: 0,
                selector: selector.clone(),
            });
            if !child.tail.is_empty() {
                chunks.extend(chunk_text(&child.tail, selector));
            }
        }
    }
    tag.children = kept;
    if chunks.len() > first {
        chunks[first].starts_tags.push(aid.clone());
        chunks.last_mut().expect("non-empty").ends_tags.push(aid.clone());
    } else if !chunks.is_empty() {
        return Err("stepped into a tag that made no chunks".into());
    }
    *selector = format!("S-//*[@aid='{aid}']");
    Ok(())
}

/// Text as chunks of at most CHUNK_SIZE bytes; pieces after the first are
/// wrapped as kindlegen does. Never splits a character or an entity.
fn chunk_text(text: &str, selector: &str) -> Vec<Chunk> {
    let mut pieces = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let mut end = rest.len().min(CHUNK_SIZE);
        while !rest.is_char_boundary(end) {
            end -= 1;
        }
        if end < rest.len()
            && let Some(amp) = rest[..end].rfind('&')
            && !rest[amp..end].contains(';')
        {
            end = amp.max(1);
        }
        pieces.push(&rest[..end]);
        rest = &rest[end..];
    }
    pieces
        .into_iter()
        .enumerate()
        .map(|(i, p)| Chunk {
            raw: if i == 0 { p.to_owned() } else { format!("<span class=\"AmznBigTextBlock\">{p}</span>") },
            starts_tags: vec![],
            ends_tags: vec![],
            insert_pos: 0,
            selector: selector.to_owned(),
        })
        .collect()
}

/// Join neighbouring small chunks within the same skeleton tag.
fn merge_small(chunks: Vec<Chunk>) -> Vec<Chunk> {
    let mut out: Vec<Chunk> = Vec::new();
    for c in chunks {
        match out.last_mut() {
            Some(prev)
                if c.starts_tags.is_empty()
                    && c.raw.len() + prev.raw.len() <= CHUNK_SIZE
                    && prev.ends_tags.is_empty() =>
            {
                prev.raw.push_str(&c.raw);
                prev.ends_tags = c.ends_tags;
            }
            _ => out.push(c),
        }
    }
    out
}

/// For each aid'd tag left in the skeleton: the length of its start tag plus
/// text, and of its end tag plus tail.
fn collect_metrics(el: &Element, out: &mut HashMap<String, (usize, usize)>) {
    if let Some(aid) = el.attr("aid") {
        let mut raw = String::new();
        el.html(true, &mut raw);
        let start = raw.find('>').map_or(0, |i| i + 1) + el.text.len();
        let end = raw.len() - raw.rfind('<').unwrap_or(0);
        out.insert(aid.to_owned(), (start, end));
    }
    for c in &el.children {
        collect_metrics(c, out);
    }
}

/// (offset of the tag, aid) for every tag carrying an aid or cid.
fn aid_positions(html: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let bytes = html.as_bytes();
    let mut i = 0;
    while let Some(lt) = html[i..].find('<').map(|p| p + i) {
        let Some(gt) = html[lt..].find('>').map(|p| p + lt) else { break };
        let tag = &html[lt..gt];
        for marker in [" aid=\"", " cid=\""] {
            if let Some(p) = tag.find(marker) {
                let v = &tag[p + marker.len()..];
                if let Some(q) = v.find('"') {
                    out.push((lt, v[..q].to_owned()));
                }
                break;
            }
        }
        i = gt + 1;
        if i >= bytes.len() {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::dom::parse;
    use super::*;

    fn with_aids(mut root: Element) -> Element {
        let mut n = 0;
        let mut f = |e: &mut Element| {
            if AID_ABLE.contains(&e.local().as_str()) {
                e.set_attr("aid", &base32(n, 1));
                n += 1;
            }
        };
        f(&mut root);
        root.walk_mut(&mut f);
        root
    }

    fn rebuild(c: &Chunked, file: usize) -> String {
        let s = &c.skels[file];
        let mut doc = c.text[s.start_pos..s.start_pos + s.length].to_owned();
        for ch in c.chunks.iter().filter(|ch| ch.file_number == file) {
            let raw = &c.text[s.start_pos + s.length + ch.start_pos..][..ch.length];
            doc.insert_str(ch.insert_pos - s.start_pos, raw);
        }
        doc
    }

    #[test]
    fn chunks_rebuild_to_the_original() {
        let big: String =
            (0..400).map(|i| format!("<p>Paragraph {i} &amp; some text that goes on for a while.</p>\n")).collect();
        let doc = format!(
            "<html><head><title>T</title></head><body>\n<h1 id=\"top\">Title</h1>\n<div class=\"big\">{big}</div>\n<p>End <a href=\"#top\">top</a></p>\n</body></html>"
        );
        let root = with_aids(parse(&doc).unwrap());
        let mut expected = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        let mut html = String::new();
        let mut r = root.clone();
        r.tail = "\n".into();
        r.children[1].tail = "\n".into();
        r.html(true, &mut html);
        expected.push_str(&html.replacen("<html", "<html xmlns=\"http://www.w3.org/1999/xhtml\"", 1));

        let c = chunk_files(vec![root], &HashMap::new()).unwrap();
        assert!(c.chunks.len() > 3, "the big div was stepped into: {}", c.chunks.len());
        assert!(c.chunks.iter().all(|ch| ch.length <= CHUNK_SIZE));
        assert_eq!(rebuild(&c, 0), expected, "skeleton + chunks give back the document");
        // The h1 is found where it really is.
        let h1_aid = "1";
        let (_, _, offset) = c.aid_offsets[h1_aid];
        assert!(expected[offset..].starts_with("<h1"), "{}", &expected[offset..offset + 20]);
    }

    #[test]
    fn base32_references() {
        assert_eq!(base32(0, 4), "0000");
        assert_eq!(base32(33, 4), "0011");
        assert_eq!(base32(1_000_000, 1), "UGI0");
    }
}
