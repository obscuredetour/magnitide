// Derived from calibre's src/calibre/ebooks/mobi/writer8/ (main.py, exth.py) (calibre 9.15).
// Copyright: 2012, Kovid Goyal <kovid@kovidgoyal.net>. License: GPLv3.
// Ported to Rust for Magnitide, 2026.

//! EPUB → AZW3 (KF8), natively: the Kindle format, without calibre.
//!
//! Follows calibre's KF8 writer (writer8/, GPL-3.0 like Magnitide):
//! * resources (images, fonts) become records, referenced as `kindle:embed:…`;
//! * stylesheets and SVG become extra "flows" (`kindle:flow:…`) after the text;
//! * each HTML file is split into a skeleton and chunks ([`chunker`]), with
//!   `aid` attributes marking places links and the table of contents point at;
//! * the text is cut into 4096-byte PalmDOC-compressed records;
//! * skeleton, chunk, NCX (table of contents) and guide indexes ([`index`]);
//! * record 0: PalmDOC + MOBI headers, EXTH metadata, the full title.
//!
//! * trailing byte sequences ([`tbs`]) on text records, for chapter jumps.
//!
//! Not yet: obfuscated fonts (fonts are stored plainly).

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufReader, Cursor, Write};
use std::path::Path;

use roxmltree::Node;
use zip::ZipArchive;

use self::chunker::{AID_ABLE, base32};
use self::dom::Element;
use self::index::{CHUNK_TAGS, Cncx, Entry, GUIDE_TAGS, NCX_TAGS, SKEL_TAGS};
use crate::metadata::epub::{
    LocalName, MAX_XML, Res, Zip, attr, child, declared_cover, opf_path, parse_xml, read_entry, resolve,
};
use crate::metadata::{BookMetadata, epub, language_tag};

mod chunker;
mod dom;
mod index;
pub mod palmdoc;
mod tbs;

const RECORD_SIZE: usize = 4096;
const NULL: u32 = 0xFFFF_FFFF;
const MAX_RESOURCE: u64 = 30 << 20;
const MAX_DOC: u64 = 32 << 20;

/// What converting noticed but didn't stop for.
#[derive(Debug, Default)]
pub struct Azw3Report {
    pub warnings: Vec<String>,
}

/// A table of contents entry, as the EPUB has it.
struct TocNode {
    label: String,
    /// Zip path and fragment.
    target: (String, String),
    children: Vec<TocNode>,
}

/// Convert the EPUB at `src` to AZW3 at `dest` (which must not exist).
pub fn convert(src: &Path, dest: &Path) -> Res<Azw3Report> {
    let meta = epub::read(src).ok_or("not a readable EPUB")?;
    let mut zip: Zip = ZipArchive::new(BufReader::new(File::open(src)?))?;
    let opf_path = opf_path(&mut zip)?;
    let opf_text = String::from_utf8(read_entry(&mut zip, &opf_path, MAX_XML)?)?;
    let opf = parse_xml(&opf_text)?;
    let package = opf.root_element();
    let manifest = child(package, "manifest").ok_or("no <manifest>")?;
    let metadata = child(package, "metadata").ok_or("no <metadata>")?;
    let spine_node = child(package, "spine").ok_or("no <spine>")?;
    let mut report = Azw3Report::default();

    // Manifest: id → (zip path, media type, properties).
    let items: Vec<(String, String, String, String)> = manifest
        .children()
        .filter(|n| n.has_tag_name_local("item"))
        .filter_map(|n| {
            Some((
                attr(n, "id")?.to_owned(),
                resolve(&opf_path, attr(n, "href")?),
                attr(n, "media-type").unwrap_or_default().to_ascii_lowercase(),
                attr(n, "properties").unwrap_or_default().to_owned(),
            ))
        })
        .collect();
    let by_id: HashMap<&str, &(String, String, String, String)> = items.iter().map(|i| (i.0.as_str(), i)).collect();
    let spine: Vec<String> = spine_node
        .children()
        .filter(|n| n.has_tag_name_local("itemref"))
        .filter_map(|n| by_id.get(attr(n, "idref")?))
        .filter(|i| i.2.contains("html"))
        .map(|i| i.1.clone())
        .collect();
    if spine.is_empty() {
        return Err("the EPUB has no pages".into());
    }
    let spine_set: HashSet<&str> = spine.iter().map(String::as_str).collect();
    let cover_path = declared_cover(metadata, manifest).and_then(|n| attr(n, "href")).map(|h| resolve(&opf_path, h));

    // Resources: images, a cover thumbnail, fonts. 1-based record numbers.
    let mut resources: Vec<Vec<u8>> = Vec::new();
    let mut embeds: HashMap<String, (usize, Option<&'static str>)> = HashMap::new();
    let (mut cover_offset, mut thumb_offset) = (None, None);
    let mut has_fonts = false;
    for (_, path, media, _) in &items {
        let is_font =
            media.contains("font") || media.contains("opentype") || path.ends_with(".ttf") || path.ends_with(".otf");
        if media.starts_with("image/") && media != "image/svg+xml" {
            let Ok(data) = read_entry(&mut zip, path, MAX_RESOURCE) else {
                report.warnings.push(format!("{path}: missing image"));
                continue;
            };
            let Some((data, mime)) = kindle_image(&data) else {
                report.warnings.push(format!("{path}: unreadable image left out"));
                continue;
            };
            resources.push(data);
            embeds.insert(path.clone(), (resources.len(), Some(mime)));
            if cover_path.as_deref() == Some(path.as_str()) {
                cover_offset = Some(resources.len() as u32 - 1);
                if let Some(thumb) = thumbnail(&resources[resources.len() - 1]) {
                    resources.push(thumb);
                    thumb_offset = Some(resources.len() as u32 - 1);
                }
            }
        } else if is_font && !path.ends_with(".woff") && !path.ends_with(".woff2") {
            let Ok(data) = read_entry(&mut zip, path, MAX_RESOURCE) else { continue };
            resources.push(font_record(&data));
            embeds.insert(path.clone(), (resources.len(), None));
            has_fonts = true;
        }
    }

    // Flows after the text: stylesheets and SVG images.
    let mut flows: Vec<String> = vec![String::new()];
    let mut flow_of: HashMap<String, usize> = HashMap::new();
    let css_items: Vec<&String> = items.iter().filter(|i| i.2 == "text/css").map(|i| &i.1).collect();
    for path in &css_items {
        flow_of.insert((*path).clone(), flows.len());
        flows.push(String::new());
    }
    for path in &css_items {
        let css =
            read_entry(&mut zip, path, MAX_XML).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
        flows[flow_of[*path]] = rewrite_css(&css, path, &embeds, &flow_of);
    }
    for (_, path, media, _) in &items {
        if media == "image/svg+xml"
            && let Ok(raw) = read_entry(&mut zip, path, MAX_XML)
            && let Ok(mut svg) = dom::parse_any(&String::from_utf8_lossy(&raw))
        {
            embed_links(&mut svg, path, &embeds);
            let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
            svg.xml(true, &mut out);
            flow_of.insert(path.clone(), flows.len());
            flows.push(out);
        }
    }

    // The pages.
    let mut roots = Vec::new();
    let mut id_map: HashMap<(String, String), String> = HashMap::new();
    let mut links: Vec<(String, (String, String))> = Vec::new();
    let mut inline_css: HashMap<String, usize> = HashMap::new();
    for (i, path) in spine.iter().enumerate() {
        let raw = read_entry(&mut zip, path, MAX_DOC)?;
        let mut root =
            dom::parse(&String::from_utf8_lossy(&raw)).map_err(|e| format!("{path} couldn't be read as XHTML: {e}"))?;
        root.attrs.retain(|(k, _)| matches!(k.as_str(), "lang" | "xml:lang" | "class" | "style"));
        clean(&mut root);
        embed_links(&mut root, path, &embeds);
        styles_to_flows(&mut root, path, &embeds, &mut flows, &mut flow_of, &mut inline_css);
        svgs_to_flows(&mut root, &mut flows, &flow_of, path);
        internal_links(&mut root, path, &spine_set, &mut links);
        assign_aids(&mut root, path, i, &mut id_map);
        roots.push(root);
    }
    let lookup =
        |target: &(String, String)| id_map.get(target).or_else(|| id_map.get(&(target.0.clone(), String::new())));
    let placeholders: HashMap<String, String> =
        links.iter().filter_map(|(p, target)| Some((p.clone(), lookup(target)?.clone()))).collect();
    let chunked = chunker::chunk_files(roots, &placeholders)?;
    let text_len_flow0 = chunked.text.len();
    flows[0] = chunked.text;

    // Text records.
    let all: Vec<u8> = flows.iter().flat_map(|f| f.as_bytes().iter().copied()).collect();
    let mut records: Vec<Vec<u8>> = vec![Vec::new()]; // record 0, built last
    let mut text_size = 0;
    let mut record_lengths = Vec::new();
    let mut pos = 0;
    while pos < all.len() {
        let end = (pos + RECORD_SIZE).min(all.len());
        let data = &all[pos..end];
        let overlap = &all[end..end + utf8_overlap(&all, end)];
        record_lengths.push(data.len());
        let mut rec = palmdoc::compress(data);
        rec.extend_from_slice(overlap);
        rec.push(overlap.len() as u8);
        text_size += rec.len();
        records.push(rec);
        pos = end;
    }
    let last_text_record = records.len() - 1;
    let mut first_non_text = records.len();
    if text_size % 4 != 0 {
        records.push(vec![0; text_size % 4]);
        first_non_text += 1;
    }

    // Indexes.
    let cncx = Cncx::new(chunked.chunks.iter().map(|c| c.selector.as_str()));
    let chunk_entries: Vec<Entry> = chunked
        .chunks
        .iter()
        .map(|c| Entry {
            name: format!("{:010}", c.insert_pos).into_bytes(),
            values: vec![
                (2, vec![cncx.offset(&c.selector)]),
                (3, vec![c.file_number as u64]),
                (4, vec![c.sequence_number as u64]),
                (6, vec![c.start_pos as u64, c.length as u64]),
            ],
        })
        .collect();
    let chunk_index = records.len() as u32;
    records.extend(index::build(CHUNK_TAGS, &chunk_entries, &cncx));
    let skel_entries: Vec<Entry> = chunked
        .skels
        .iter()
        .map(|s| Entry {
            name: format!("SKEL{:010}", s.file_number).into_bytes(),
            values: vec![
                (1, vec![s.chunk_count as u64; 2]),
                (6, vec![s.start_pos as u64, s.length as u64, s.start_pos as u64, s.length as u64]),
            ],
        })
        .collect();
    let skel_index = records.len() as u32;
    records.extend(index::build(SKEL_TAGS, &skel_entries, &Cncx::empty()));

    let position = |target: &(String, String)| -> (u64, u64, u64) {
        match lookup(target).and_then(|aid| chunked.aid_offsets.get(aid)) {
            Some(&(pos, fid, offset)) => (pos as u64, fid as u64, offset as u64),
            None => (0, 0, chunked.chunks.first().map_or(0, |c| c.insert_pos as u64)),
        }
    };

    // Guide (EPUB 2 guide, else EPUB 3 landmarks) and where reading starts.
    let mut start_offset = None;
    let mut guide = read_guide(package, &opf_path);
    if guide.is_empty() {
        guide = landmarks(&mut zip, &items);
    }
    guide.sort_by(|a, b| a.0.cmp(&b.0));
    guide.dedup_by(|a, b| a.0 == b.0);
    let guide_cncx = Cncx::new(guide.iter().map(|g| g.1.as_str()));
    let mut guide_entries = Vec::new();
    for (kind, title, target) in &guide {
        let (pos, fid, offset) = position(target);
        if matches!(kind.as_str(), "text" | "start" | "other.start") || title.eq_ignore_ascii_case("start") {
            start_offset = Some(offset as u32);
        }
        guide_entries.push(Entry {
            name: kind.clone().into_bytes(),
            values: vec![(1, vec![guide_cncx.offset(title)]), (6, vec![pos, fid])],
        });
    }
    let guide_index = if guide_entries.is_empty() {
        NULL
    } else {
        let at = records.len() as u32;
        records.extend(index::build(GUIDE_TAGS, &guide_entries, &guide_cncx));
        at
    };

    // Table of contents: the EPUB's, else one entry per page.
    let mut toc = read_toc(&mut zip, &items, spine_node, &by_id);
    if toc.is_empty() {
        toc = spine
            .iter()
            .enumerate()
            .map(|(i, p)| TocNode {
                label: format!("Part {}", i + 1),
                target: (p.clone(), String::new()),
                children: vec![],
            })
            .collect();
        report.warnings.push("no table of contents; made one per page".into());
    }
    let ncx_index = {
        let entries = ncx_entries(&toc, &position, text_len_flow0 as u64);
        let ncx_cncx = Cncx::new(entries.iter().map(|e| e.label.as_str()));
        let width = format!("{:X}", entries.len().saturating_sub(1)).len().max(2);
        let index_entries: Vec<Entry> = entries
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let mut values = vec![
                    (1, vec![e.offset]),
                    (2, vec![e.length]),
                    (3, vec![ncx_cncx.offset(&e.label)]),
                    (4, vec![e.depth]),
                ];
                if let Some(p) = e.parent {
                    values.push((21, vec![p as u64]));
                }
                if let Some((first, last)) = e.children {
                    values.push((22, vec![first as u64]));
                    values.push((23, vec![last as u64]));
                }
                values.push((6, vec![e.pos, e.fid]));
                Entry { name: format!("{i:0width$X}").into_bytes(), values }
            })
            .collect();
        // Which entries each text record touches.
        let toc_entries: Vec<tbs::TocEntry> = entries
            .iter()
            .enumerate()
            .map(|(i, e)| tbs::TocEntry {
                index: i,
                start: e.offset,
                length: e.length,
                depth: e.depth,
                parent: e.parent,
            })
            .collect();
        for (i, trailing) in tbs::all(&toc_entries, &record_lengths).into_iter().enumerate() {
            records[i + 1].extend(trailing);
        }
        let at = records.len() as u32;
        records.extend(index::build(NCX_TAGS, &index_entries, &ncx_cncx));
        at
    };

    // Resources, then the flow table, FLIS, FCIS, end of file.
    let num_resources = resources.len() as u32;
    let first_resource = if resources.is_empty() { NULL } else { records.len() as u32 };
    records.extend(resources);
    let fdst_record = records.len() as u32;
    let mut fdst = b"FDST".to_vec();
    fdst.extend_from_slice(&12u32.to_be_bytes());
    fdst.extend_from_slice(&(flows.len() as u32).to_be_bytes());
    let mut start = 0u32;
    for f in &flows {
        fdst.extend_from_slice(&start.to_be_bytes());
        start += f.len() as u32;
        fdst.extend_from_slice(&start.to_be_bytes());
    }
    records.push(fdst);
    let flis_record = records.len() as u32;
    records.push(FLIS.to_vec());
    let fcis_record = records.len() as u32;
    records.push(fcis(all.len() as u32));
    records.push(b"\xe9\x8e\r\n".to_vec());

    let uuid = meta.uuids.first().cloned().unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let exth = build_exth(&meta, &uuid, cover_offset, thumb_offset, start_offset, num_resources, has_fonts);
    let title = meta.title.clone().unwrap_or_else(|| "Unknown".into());
    records[0] = record0(Record0 {
        text_length: all.len() as u32,
        last_text_record: last_text_record as u16,
        first_non_text: first_non_text as u32,
        language: mobi_language(meta.languages.first().map(|l| language_tag(l)).as_deref().unwrap_or("en")),
        first_resource,
        exth_flags: 0b101_0000 | if has_fonts { 0b1_0000_0000_0000 } else { 0 },
        fdst_record,
        fdst_count: flows.len() as u32,
        flis_record,
        fcis_record,
        ncx_index,
        chunk_index,
        skel_index,
        guide_index,
        exth,
        title: title.trim().as_bytes().to_vec(),
    });

    let mut out = File::create_new(dest)?;
    let result = write_palmdb(&mut out, &title, &records);
    if result.is_err() {
        let _ = std::fs::remove_file(dest);
    }
    result?;
    Ok(report)
}

/// An href (relative to `base`) as (zip path, fragment); "#x" stays in `base`.
fn target(base: &str, href: &str) -> (String, String) {
    let (file, frag) = href.split_once('#').unwrap_or((href, ""));
    let path = if file.is_empty() { base.to_owned() } else { resolve(base, file) };
    (path, frag.to_owned())
}

/// Bytes from the next record needed to finish the character cut at `end`.
fn utf8_overlap(all: &[u8], end: usize) -> usize {
    if end >= all.len() {
        return 0;
    }
    // Find the start of the character that contains byte end-1.
    let mut start = end - 1;
    while start > 0 && end - start < 4 && all[start] & 0xC0 == 0x80 {
        start -= 1;
    }
    let len = match all[start] {
        b if b < 0x80 => 1,
        b if b >> 5 == 0b110 => 2,
        b if b >> 4 == 0b1110 => 3,
        b if b >> 3 == 0b11110 => 4,
        _ => 1,
    };
    (start + len).saturating_sub(end).min(all.len() - end)
}

/// An image as Kindles take it: JPEG, PNG and GIF as they are, anything else
/// as JPEG.
fn kindle_image(data: &[u8]) -> Option<(Vec<u8>, &'static str)> {
    if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some((data.to_vec(), "image/jpeg"));
    }
    if data.starts_with(b"\x89PNG") {
        return Some((data.to_vec(), "image/png"));
    }
    if data.starts_with(b"GIF8") {
        return Some((data.to_vec(), "image/gif"));
    }
    crate::images::as_jpeg(data).ok().map(|j| (j, "image/jpeg"))
}

/// A small JPEG of the cover (at most 330×470), for the Kindle's library view.
fn thumbnail(data: &[u8]) -> Option<Vec<u8>> {
    let img = image::load_from_memory(data).ok()?.thumbnail(330, 470);
    let mut out = Cursor::new(Vec::new());
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85).encode_image(&img.to_rgb8()).ok()?;
    Some(out.into_inner())
}

/// A FONT record: zlib-compressed, not obfuscated.
fn font_record(data: &[u8]) -> Vec<u8> {
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    let _ = z.write_all(data);
    let packed = z.finish().unwrap_or_default();
    let mut rec = b"FONT".to_vec();
    rec.extend_from_slice(&(data.len() as u32).to_be_bytes());
    rec.extend_from_slice(&1u32.to_be_bytes()); // zlib
    rec.extend_from_slice(&24u32.to_be_bytes()); // data offset
    rec.extend_from_slice(&0u32.to_be_bytes()); // no XOR key
    rec.extend_from_slice(&24u32.to_be_bytes());
    rec.extend_from_slice(&packed);
    rec
}

fn embed_ref(index: usize, mime: Option<&str>) -> String {
    match mime {
        Some(m) => format!("kindle:embed:{}?mime={m}", base32(index as u64, 4)),
        None => format!("kindle:embed:{}", base32(index as u64, 4)),
    }
}

fn flow_ref(index: usize, mime: &str) -> String {
    format!("kindle:flow:{}?mime={mime}", base32(index as u64, 4))
}

/// `url(...)` and `@import` in CSS pointed at the book's records and flows.
fn rewrite_css(
    css: &str,
    css_path: &str,
    embeds: &HashMap<String, (usize, Option<&str>)>,
    flows: &HashMap<String, usize>,
) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(i) = rest.find("url(") {
        out.push_str(&rest[..i + 4]);
        let after = &rest[i + 4..];
        let Some(close) = after.find(')') else {
            rest = after;
            continue;
        };
        let raw = after[..close].trim().trim_matches(|c| c == '"' || c == '\'');
        let target = resolve(css_path, raw);
        match (embeds.get(&target), flows.get(&target)) {
            // Unquoted, as kindlegen writes them (and as readers expect).
            (Some((idx, mime)), _) => out.push_str(&embed_ref(*idx, *mime)),
            (None, Some(idx)) => out.push_str(&flow_ref(*idx, "text/css")),
            _ => out.push_str(&after[..close]),
        }
        out.push(')');
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    out
}

/// Drop empty scripts and any aid/cid attributes already present.
fn clean(el: &mut Element) {
    el.children.retain(|c| {
        !(c.local() == "script" && c.text.trim().is_empty() && c.children.is_empty() && c.attr("src").is_none())
    });
    el.remove_attr("aid");
    el.remove_attr("cid");
    for c in &mut el.children {
        clean(c);
    }
}

/// Images (and SVG `<image>`s) pointed at their resource records.
fn embed_links(el: &mut Element, doc: &str, embeds: &HashMap<String, (usize, Option<&str>)>) {
    let mut f = |e: &mut Element| {
        let local = e.local();
        if local == "img" || local == "image" {
            let names: Vec<String> = e
                .attrs
                .iter()
                .filter(|(k, _)| matches!(k.rsplit(':').next(), Some("src" | "href")))
                .map(|(k, _)| k.clone())
                .collect();
            for name in names {
                let raw = e.attrs.iter().find(|(k, _)| *k == name).map(|(_, v)| dom::unescape(v)).unwrap_or_default();
                if let Some((idx, mime)) = embeds.get(&resolve(doc, &raw)) {
                    e.set_attr(&name, &embed_ref(*idx, *mime));
                }
            }
        }
    };
    f(el);
    el.walk_mut(&mut f);
}

/// `<style>`s become flows behind `<link>`s; stylesheet links point at their flows.
fn styles_to_flows(
    el: &mut Element,
    doc: &str,
    embeds: &HashMap<String, (usize, Option<&str>)>,
    flows: &mut Vec<String>,
    flow_of: &mut HashMap<String, usize>,
    inline: &mut HashMap<String, usize>,
) {
    for c in &mut el.children {
        match c.local().as_str() {
            "link" => {
                if let Some(href) = c.attr("href").map(dom::unescape)
                    && let Some(idx) = flow_of.get(&resolve(doc, &href))
                {
                    c.set_attr("href", &flow_ref(*idx, "text/css"));
                }
            }
            "style" => {
                let css = dom::unescape(&c.text);
                if css.trim().is_empty() {
                    c.name = "#drop".into();
                    continue;
                }
                let css = rewrite_css(&css, doc, embeds, flow_of);
                let idx = *inline.entry(css.clone()).or_insert_with(|| {
                    flows.push(css);
                    flows.len() - 1
                });
                let tail = std::mem::take(&mut c.tail);
                *c = Element::new("link");
                c.attrs = vec![
                    ("type".into(), "text/css".into()),
                    ("rel".into(), "stylesheet".into()),
                    ("href".into(), flow_ref(idx, "text/css")),
                ];
                c.tail = format!("\n{tail}");
            }
            _ => styles_to_flows(c, doc, embeds, flows, flow_of, inline),
        }
    }
    let mut kept = Vec::with_capacity(el.children.len());
    for c in std::mem::take(&mut el.children) {
        if c.name == "#drop" {
            match kept.last_mut() {
                Some(prev) => {
                    let prev: &mut Element = prev;
                    prev.tail.push_str(&c.tail);
                }
                None => el.text.push_str(&c.tail),
            }
        } else {
            kept.push(c);
        }
    }
    el.children = kept;
}

/// Inline `<svg>` become flows shown by `<img>`; `<img>`s of SVG files point at theirs.
fn svgs_to_flows(el: &mut Element, flows: &mut Vec<String>, flow_of: &HashMap<String, usize>, doc: &str) {
    for c in &mut el.children {
        match c.local().as_str() {
            "svg" => {
                let mut xml = String::new();
                c.xml(true, &mut xml);
                flows.push(xml);
                let tail = std::mem::take(&mut c.tail);
                *c = Element::new("img");
                c.attrs = vec![("src".into(), flow_ref(flows.len() - 1, "image/svg+xml"))];
                c.tail = tail;
            }
            "img" => {
                if let Some(src) = c.attr("src").map(dom::unescape)
                    && let Some(idx) = flow_of.get(&resolve(doc, &src))
                {
                    c.set_attr("src", &flow_ref(*idx, "image/svg+xml"));
                }
            }
            _ => svgs_to_flows(c, flows, flow_of, doc),
        }
    }
}

/// Links to the book's own pages become placeholders, resolved after chunking.
fn internal_links(el: &mut Element, doc: &str, spine: &HashSet<&str>, out: &mut Vec<(String, (String, String))>) {
    let mut f = |e: &mut Element| {
        if e.local() != "a" {
            return;
        }
        let Some(href) = e.attr("href").map(dom::unescape) else { return };
        if href.contains("://") || href.starts_with("mailto:") {
            return;
        }
        let (file, frag) = href.split_once('#').unwrap_or((href.as_str(), ""));
        let target = if file.is_empty() { doc.to_owned() } else { resolve(doc, file) };
        if spine.contains(target.as_str()) {
            let placeholder = format!("kindle:pos:fid:0000:off:{}", base32(out.len() as u64 + 1, 10));
            e.set_attr("href", &placeholder);
            out.push((placeholder, (target, frag.to_owned())));
        }
    };
    el.walk_mut(&mut f);
}

/// `aid`s on the tags kindlegen marks and on everything with an id (`cid`
/// inside tables, which Kindles mishandle with aids).
fn assign_aids(root: &mut Element, doc: &str, file: usize, ids: &mut HashMap<(String, String), String>) {
    fn visit(
        e: &mut Element,
        in_table: bool,
        doc: &str,
        base: u64,
        n: &mut u64,
        cid: &mut u64,
        ids: &mut HashMap<(String, String), String>,
    ) {
        for c in &mut e.children {
            let local = c.local();
            let mut id = c.attr("id").map(dom::unescape);
            if id.is_none()
                && local == "a"
                && let Some(name) = c.attr("name").map(str::to_owned)
            {
                c.set_attr("id", &name);
                id = Some(dom::unescape(&name));
            }
            let tabled = in_table || local == "table";
            if id.is_some() || AID_ABLE.contains(&local.as_str()) {
                if tabled {
                    if let Some(id) = &id {
                        *cid += 1;
                        let v = format!("c{cid}");
                        c.set_attr("cid", &v);
                        ids.insert((doc.to_owned(), id.clone()), v);
                    }
                } else {
                    let aid = base32(base + *n, 1);
                    c.set_attr("aid", &aid);
                    if local == "body" {
                        ids.insert((doc.to_owned(), String::new()), aid.clone());
                    }
                    if let Some(id) = &id {
                        ids.insert((doc.to_owned(), id.clone()), aid);
                    }
                    *n += 1;
                }
            }
            visit(c, tabled, doc, base, n, cid, ids);
        }
    }
    let (mut n, mut cid) = (0, 0);
    visit(root, false, doc, file as u64 * 1_000_000, &mut n, &mut cid, ids);
}

/// The EPUB 2 guide: (type, title, target).
fn read_guide(package: Node<'_, '_>, opf_path: &str) -> Vec<(String, String, (String, String))> {
    let Some(guide) = child(package, "guide") else { return Vec::new() };
    guide
        .children()
        .filter(|n| n.has_tag_name_local("reference"))
        .filter_map(|n| {
            Some((
                attr(n, "type")?.to_owned(),
                attr(n, "title").filter(|t| !t.trim().is_empty()).unwrap_or("Unknown").to_owned(),
                target(opf_path, attr(n, "href")?),
            ))
        })
        .collect()
}

/// EPUB 3 landmarks, as guide entries.
fn landmarks(zip: &mut Zip, items: &[(String, String, String, String)]) -> Vec<(String, String, (String, String))> {
    let Some(nav) = items.iter().find(|i| i.3.split_whitespace().any(|p| p == "nav")) else { return Vec::new() };
    let Ok(raw) = read_entry(zip, &nav.1, MAX_XML) else { return Vec::new() };
    let raw = String::from_utf8_lossy(&raw);
    let Ok(doc) = parse_xml(&raw) else { return Vec::new() };
    let Some(lm) = doc.descendants().find(|n| n.has_tag_name_local("nav") && attr(*n, "type") == Some("landmarks"))
    else {
        return Vec::new();
    };
    lm.descendants()
        .filter(|n| n.has_tag_name_local("a"))
        .filter_map(|a| {
            let kind = match attr(a, "type")? {
                "bodymatter" => "text",
                "toc" => "toc",
                "cover" => "cover",
                _ => return None,
            };
            Some((kind.to_owned(), crate::metadata::epub::text(a), target(&nav.1, attr(a, "href")?)))
        })
        .collect()
}

/// The table of contents: EPUB 3 nav, else the EPUB 2 NCX.
fn read_toc(
    zip: &mut Zip,
    items: &[(String, String, String, String)],
    spine: Node<'_, '_>,
    by_id: &HashMap<&str, &(String, String, String, String)>,
) -> Vec<TocNode> {
    if let Some(nav) = items.iter().find(|i| i.3.split_whitespace().any(|p| p == "nav"))
        && let Ok(raw) = read_entry(zip, &nav.1, MAX_XML)
    {
        let raw = String::from_utf8_lossy(&raw);
        if let Ok(doc) = parse_xml(&raw)
            && let Some(nav_el) =
                doc.descendants().find(|n| n.has_tag_name_local("nav") && attr(*n, "type") == Some("toc"))
            && let Some(ol) = nav_el.children().find(|n| n.has_tag_name_local("ol"))
        {
            let toc = nav_list(ol, &nav.1);
            if !toc.is_empty() {
                return toc;
            }
        }
    }
    let ncx = attr(spine, "toc")
        .and_then(|id| by_id.get(id).copied())
        .or_else(|| items.iter().find(|i| i.2 == "application/x-dtbncx+xml"));
    let Some(ncx) = ncx else { return Vec::new() };
    let Ok(raw) = read_entry(zip, &ncx.1, MAX_XML) else { return Vec::new() };
    let raw = String::from_utf8_lossy(&raw);
    let Ok(doc) = parse_xml(&raw) else { return Vec::new() };
    let Some(map) = doc.descendants().find(|n| n.has_tag_name_local("navMap")) else { return Vec::new() };
    ncx_points(map, &ncx.1)
}

fn nav_list(ol: Node<'_, '_>, base: &str) -> Vec<TocNode> {
    ol.children()
        .filter(|n| n.has_tag_name_local("li"))
        .filter_map(|li| {
            let a = li.children().find(|n| n.has_tag_name_local("a") || n.has_tag_name_local("span"))?;
            let children =
                li.children().find(|n| n.has_tag_name_local("ol")).map(|o| nav_list(o, base)).unwrap_or_default();
            Some(TocNode { label: crate::metadata::epub::text(a), target: target(base, attr(a, "href")?), children })
        })
        .collect()
}

fn ncx_points(parent: Node<'_, '_>, base: &str) -> Vec<TocNode> {
    parent
        .children()
        .filter(|n| n.has_tag_name_local("navPoint"))
        .filter_map(|p| {
            let label = p
                .descendants()
                .find(|n| n.has_tag_name_local("text"))
                .map(crate::metadata::epub::text)
                .unwrap_or_default();
            let src = p.children().find(|n| n.has_tag_name_local("content")).and_then(|c| attr(c, "src"))?;
            Some(TocNode { label, target: target(base, src), children: ncx_points(p, base) })
        })
        .collect()
}

/// A flattened NCX entry.
struct NcxEntry {
    label: String,
    depth: u64,
    offset: u64,
    length: u64,
    pos: u64,
    fid: u64,
    parent: Option<usize>,
    children: Option<(usize, usize)>,
}

/// Flatten the table of contents depth first, then order it by (depth,
/// offset) as Kindles need, with parent/child links and lengths.
fn ncx_entries(
    toc: &[TocNode],
    position: &impl Fn(&(String, String)) -> (u64, u64, u64),
    text_len: u64,
) -> Vec<NcxEntry> {
    struct Flat {
        label: String,
        depth: u64,
        pos: (u64, u64, u64),
        parent: Option<usize>,
        children: Vec<usize>,
    }
    fn flatten(
        nodes: &[TocNode],
        depth: u64,
        parent: Option<usize>,
        out: &mut Vec<Flat>,
        position: &impl Fn(&(String, String)) -> (u64, u64, u64),
    ) {
        for n in nodes {
            let me = out.len();
            let label = if n.label.trim().is_empty() { "Unknown".to_owned() } else { n.label.trim().to_owned() };
            out.push(Flat { label, depth, pos: position(&n.target), parent, children: vec![] });
            if let Some(p) = parent {
                out[p].children.push(me);
            }
            flatten(&n.children, depth + 1, Some(me), out, position);
        }
    }
    let mut flat = Vec::new();
    flatten(toc, 0, None, &mut flat, position);
    let mut order: Vec<usize> = (0..flat.len()).collect();
    order.sort_by_key(|&i| (flat[i].depth, flat[i].pos.2, i));
    let new_index: HashMap<usize, usize> = order.iter().enumerate().map(|(new, &old)| (old, new)).collect();
    order
        .iter()
        .map(|&i| {
            let f = &flat[i];
            let next = flat
                .iter()
                .filter(|e| e.depth <= f.depth && e.pos.2 > f.pos.2)
                .map(|e| e.pos.2)
                .min()
                .unwrap_or(text_len);
            let children: Vec<usize> = f.children.iter().map(|c| new_index[c]).collect();
            NcxEntry {
                label: f.label.clone(),
                depth: f.depth,
                offset: f.pos.2,
                length: next.saturating_sub(f.pos.2),
                pos: f.pos.0,
                fid: f.pos.1,
                parent: f.parent.map(|p| new_index[&p]),
                children: children.first().zip(children.last()).map(|(a, b)| (*a, *b)),
            }
        })
        .collect()
}

/// EXTH metadata, padded to a multiple of four (at least one byte).
fn build_exth(
    meta: &BookMetadata,
    uuid: &str,
    cover: Option<u32>,
    thumb: Option<u32>,
    start: Option<u32>,
    resources: u32,
    _fonts: bool,
) -> Vec<u8> {
    let mut recs: Vec<(u32, Vec<u8>)> = Vec::new();
    let text = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ").into_bytes();
    if let Some(t) = &meta.title {
        recs.push((503, text(t)));
    }
    for a in &meta.authors {
        recs.push((100, text(&a.name)));
    }
    if let Some(p) = &meta.publisher {
        recs.push((101, text(p)));
    }
    if let Some(d) = &meta.description {
        recs.push((103, d.as_bytes().to_vec()));
    }
    if let Some(isbn) = meta.identifiers.get("isbn") {
        recs.push((104, isbn.as_bytes().to_vec()));
    }
    for t in &meta.tags {
        recs.push((105, text(t)));
    }
    if let Some(d) = meta.published {
        recs.push((106, d.format("%Y-%m-%dT00:00:00+00:00").to_string().into_bytes()));
    }
    for l in meta.languages.iter().take(1) {
        recs.push((524, language_tag(l).into_bytes()));
    }
    // The book's uuid as its ASIN and source, as calibre does; EBOK makes
    // Kindles list it as a book.
    recs.push((113, uuid.as_bytes().to_vec()));
    recs.push((112, format!("calibre:{uuid}").into_bytes()));
    recs.push((501, b"EBOK".to_vec()));
    for (code, v) in [(204u32, 202u32), (205, 2), (206, 9), (207, 0)] {
        recs.push((code, v.to_be_bytes().to_vec()));
    }
    recs.push((535, b"0730-890adc2".to_vec()));
    if let Some(c) = cover {
        recs.push((201, c.to_be_bytes().to_vec()));
        recs.push((203, 0u32.to_be_bytes().to_vec()));
    }
    if let Some(t) = thumb {
        recs.push((202, t.to_be_bytes().to_vec()));
        recs.push((129, format!("kindle:embed:{}", base32(u64::from(t), 4)).into_bytes()));
    }
    if let Some(s) = start {
        recs.push((116, s.to_be_bytes().to_vec()));
    }
    recs.push((125, resources.to_be_bytes().to_vec()));
    if resources > 0 {
        recs.push((131, 0u32.to_be_bytes().to_vec()));
    }
    recs.push((528, b"true".to_vec()));

    let mut body = Vec::new();
    for (code, v) in &recs {
        body.extend_from_slice(&code.to_be_bytes());
        body.extend_from_slice(&(v.len() as u32 + 8).to_be_bytes());
        body.extend_from_slice(v);
    }
    let pad = 4 - body.len() % 4;
    let mut exth = b"EXTH".to_vec();
    exth.extend_from_slice(&(body.len() as u32 + 12).to_be_bytes());
    exth.extend_from_slice(&(recs.len() as u32).to_be_bytes());
    exth.extend_from_slice(&body);
    exth.extend(std::iter::repeat_n(0u8, pad));
    exth
}

/// MOBI's language code for an ISO 639-1 tag.
fn mobi_language(tag: &str) -> u32 {
    const CODES: &[(&str, u32)] = &[
        ("af", 54),
        ("ar", 1),
        ("bg", 2),
        ("ca", 3),
        ("cs", 5),
        ("da", 6),
        ("de", 7),
        ("el", 8),
        ("en", 9),
        ("es", 10),
        ("et", 37),
        ("eu", 45),
        ("fa", 41),
        ("fi", 11),
        ("fr", 12),
        ("he", 13),
        ("hi", 57),
        ("hr", 26),
        ("hu", 14),
        ("id", 33),
        ("is", 15),
        ("it", 16),
        ("ja", 17),
        ("ko", 18),
        ("lt", 39),
        ("lv", 38),
        ("nl", 19),
        ("no", 20),
        ("pl", 21),
        ("pt", 22),
        ("ro", 24),
        ("ru", 25),
        ("sk", 27),
        ("sl", 36),
        ("sr", 26),
        ("sv", 29),
        ("th", 30),
        ("tr", 31),
        ("uk", 34),
        ("vi", 42),
        ("zh", 4),
    ];
    let base = tag.split(['-', '_']).next().unwrap_or("").to_ascii_lowercase();
    CODES.iter().find(|(t, _)| *t == base).map_or(0, |(_, c)| *c)
}

struct Record0 {
    text_length: u32,
    last_text_record: u16,
    first_non_text: u32,
    language: u32,
    first_resource: u32,
    exth_flags: u32,
    fdst_record: u32,
    fdst_count: u32,
    flis_record: u32,
    fcis_record: u32,
    ncx_index: u32,
    chunk_index: u32,
    skel_index: u32,
    guide_index: u32,
    exth: Vec<u8>,
    title: Vec<u8>,
}

/// Record 0: the PalmDOC and MOBI headers (KF8, header length 264), EXTH,
/// the full title, and room for Amazon's tools to add data.
fn record0(r: Record0) -> Vec<u8> {
    let mut h: Vec<u8> = Vec::with_capacity(9000);
    let u16b = |h: &mut Vec<u8>, v: u16| h.extend_from_slice(&v.to_be_bytes());
    let u32b = |h: &mut Vec<u8>, v: u32| h.extend_from_slice(&v.to_be_bytes());
    u16b(&mut h, 2); // PalmDOC compression
    u16b(&mut h, 0);
    u32b(&mut h, r.text_length);
    u16b(&mut h, r.last_text_record);
    u16b(&mut h, RECORD_SIZE as u16);
    u16b(&mut h, 0); // not encrypted
    u16b(&mut h, 0);
    h.extend_from_slice(b"MOBI");
    u32b(&mut h, 264);
    u32b(&mut h, 2); // a book
    u32b(&mut h, 65001); // UTF-8
    u32b(&mut h, rand_u32());
    u32b(&mut h, 8); // file version
    for _ in 0..10 {
        u32b(&mut h, NULL); // orth, infl, extra indexes
    }
    u32b(&mut h, r.first_non_text);
    let title_offset_at = h.len();
    u32b(&mut h, 0);
    u32b(&mut h, r.title.len() as u32);
    u32b(&mut h, r.language);
    u32b(&mut h, 0);
    u32b(&mut h, 0);
    u32b(&mut h, 8); // min version
    u32b(&mut h, r.first_resource);
    h.extend_from_slice(&[0; 16]); // huffman
    u32b(&mut h, r.exth_flags);
    h.extend_from_slice(&[0; 32]);
    u32b(&mut h, NULL);
    u32b(&mut h, NULL); // DRM offset
    h.extend_from_slice(&[0; 12]); // DRM count, size, flags
    h.extend_from_slice(&[0; 8]);
    u32b(&mut h, r.fdst_record);
    u32b(&mut h, r.fdst_count);
    u32b(&mut h, r.fcis_record);
    u32b(&mut h, 1);
    u32b(&mut h, r.flis_record);
    u32b(&mut h, 1);
    h.extend_from_slice(&[0; 8]);
    u32b(&mut h, NULL); // SRCS
    u32b(&mut h, 0);
    h.extend_from_slice(&[0xFF; 8]);
    u32b(&mut h, 0b11); // extra data: multibyte overlap, then TBS
    u32b(&mut h, r.ncx_index);
    u32b(&mut h, r.chunk_index);
    u32b(&mut h, r.skel_index);
    u32b(&mut h, NULL); // DATP
    u32b(&mut h, r.guide_index);
    h.extend_from_slice(&[0xFF; 4]);
    h.extend_from_slice(&[0; 4]);
    h.extend_from_slice(&[0xFF; 4]);
    h.extend_from_slice(&[0; 4]);
    debug_assert_eq!(h.len(), 280);
    h.extend_from_slice(&r.exth);
    let title_offset = h.len() as u32;
    h[title_offset_at..title_offset_at + 4].copy_from_slice(&title_offset.to_be_bytes());
    h.extend_from_slice(&r.title);
    h.extend_from_slice(&[0; 8192]);
    h
}

fn rand_u32() -> u32 {
    let b = uuid::Uuid::new_v4().into_bytes();
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

const FLIS: &[u8] = b"FLIS\0\0\0\x08\0\x41\0\0\0\0\0\0\xff\xff\xff\xff\0\x01\0\x03\0\0\0\x03\0\0\0\x01\xff\xff\xff\xff";

fn fcis(text_length: u32) -> Vec<u8> {
    let mut f = b"FCIS\x00\x00\x00\x14\x00\x00\x00\x10\x00\x00\x00\x02\x00\x00\x00\x00".to_vec();
    f.extend_from_slice(&text_length.to_be_bytes());
    f.extend_from_slice(b"\x00\x00\x00\x00\x00\x00\x00\x28\x00\x00\x00\x00\x00\x00\x00");
    f.extend_from_slice(b"\x28\x00\x00\x00\x08\x00\x01\x00\x01\x00\x00\x00\x00");
    f
}

/// The PalmDB container: name, header, record list, records.
fn write_palmdb(out: &mut File, title: &str, records: &[Vec<u8>]) -> Res<()> {
    let mut name: Vec<u8> = title
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == ' ')
        .map(|c| if c == ' ' { b'_' } else { c as u8 })
        .take(31)
        .collect();
    name.resize(32, 0);
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as u32);
    let mut head = name;
    for v in [0u16, 0] {
        head.extend_from_slice(&v.to_be_bytes());
    }
    for v in [now, now, 0, 0, 0, 0] {
        head.extend_from_slice(&v.to_be_bytes());
    }
    head.extend_from_slice(b"BOOKMOBI");
    let n = records.len() as u32;
    head.extend_from_slice(&(2 * n - 1).to_be_bytes());
    head.extend_from_slice(&0u32.to_be_bytes());
    head.extend_from_slice(&(n as u16).to_be_bytes());
    let mut offset = head.len() as u32 + 8 * n + 2;
    for (i, r) in records.iter().enumerate() {
        head.extend_from_slice(&offset.to_be_bytes());
        head.extend_from_slice(&(2 * i as u32).to_be_bytes()); // attributes 0, unique id
        offset += r.len() as u32;
    }
    head.extend_from_slice(&[0, 0]);
    let mut w = std::io::BufWriter::new(out);
    w.write_all(&head)?;
    for r in records {
        w.write_all(r)?;
    }
    w.flush()?;
    w.into_inner().map_err(|e| e.into_error())?.sync_all()?;
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use std::io::Write;

    use zip::write::SimpleFileOptions;

    use super::*;

    /// A small but realistic EPUB 3: two chapters (one long enough to be
    /// split into chunks), CSS with an image, a cover, an inline SVG, a nav
    /// table of contents and links between the chapters.
    pub(crate) fn sample_epub(path: &Path) {
        let mut png = Cursor::new(Vec::new());
        image::RgbImage::from_pixel(60, 90, image::Rgb([200, 30, 30]))
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let png = png.into_inner();
        let long: String = (0..300)
            .map(|i| format!("<p>Paragraph {i}: it was a bright cold day &amp; the clocks struck&nbsp;thirteen.</p>\n"))
            .collect();
        let files: Vec<(&str, String)> = vec![
            ("mimetype", "application/epub+zip".into()),
            ("META-INF/container.xml", r#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#.into()),
            ("OEBPS/content.opf", r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="id">urn:uuid:0f0f0f0f-1111-4222-8333-444444444444</dc:identifier>
    <dc:title>Sample Book</dc:title><dc:creator>Ann Author</dc:creator><dc:language>en</dc:language>
    <meta property="dcterms:modified">2026-01-01T00:00:00Z</meta>
  </metadata>
  <manifest>
    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
    <item id="c1" href="text/ch1.xhtml" media-type="application/xhtml+xml"/>
    <item id="c2" href="text/ch2.xhtml" media-type="application/xhtml+xml"/>
    <item id="css" href="styles/style.css" media-type="text/css"/>
    <item id="cover" href="images/cover.png" media-type="image/png" properties="cover-image"/>
  </manifest>
  <spine><itemref idref="c1"/><itemref idref="c2"/></spine>
</package>"#.into()),
            ("OEBPS/nav.xhtml", r##"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><head><title>Contents</title></head><body>
<nav epub:type="toc"><ol><li><a href="text/ch1.xhtml">Chapter One</a><ol><li><a href="text/ch1.xhtml#middle">The Middle</a></li></ol></li><li><a href="text/ch2.xhtml">Chapter Two</a></li></ol></nav>
<nav epub:type="landmarks"><ol><li><a epub:type="bodymatter" href="text/ch1.xhtml">Start</a></li></ol></nav>
</body></html>"##.into()),
            ("OEBPS/text/ch1.xhtml", format!(r#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" lang="en" xml:lang="en"><head><title>One</title><link rel="stylesheet" href="../styles/style.css"/><style>h1 {{ color: red; }}</style></head>
<body><h1>Chapter One</h1>
<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" viewBox="0 0 60 90"><image width="60" height="90" xlink:href="../images/cover.png"/></svg>
<div class="long">{long}<p id="middle">The middle. <a href="ch2.xhtml#end">Skip to the end</a>.</p>{long}</div></body></html>"#)),
            ("OEBPS/text/ch2.xhtml", r#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml"><head><title>Two</title><link rel="stylesheet" href="../styles/style.css"/></head>
<body><h1>Chapter Two</h1><p>Short. <img src="../images/cover.png" alt=""/></p><p id="end">The end. <a href="ch1.xhtml#middle">Back</a></p></body></html>"#.into()),
            ("OEBPS/styles/style.css", "body { background: url('../images/cover.png'); }\np { margin: 0; }".into()),
        ];
        let mut z = zip::ZipWriter::new(File::create(path).unwrap());
        let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (name, body) in files {
            z.start_file(name, stored).unwrap();
            z.write_all(body.as_bytes()).unwrap();
        }
        z.start_file("OEBPS/images/cover.png", stored).unwrap();
        z.write_all(&png).unwrap();
        z.finish().unwrap();
    }

    /// The text of a converted book, decompressed and with trailing entries removed.
    fn text_of(azw3: &[u8]) -> String {
        let count = u16::from_be_bytes([azw3[76], azw3[77]]) as usize;
        let offsets: Vec<usize> =
            (0..count).map(|i| u32::from_be_bytes(azw3[78 + 8 * i..82 + 8 * i].try_into().unwrap()) as usize).collect();
        let rec = |i: usize| &azw3[offsets[i]..offsets.get(i + 1).copied().unwrap_or(azw3.len())];
        let r0 = rec(0);
        let last = u16::from_be_bytes([r0[8], r0[9]]) as usize;
        let mut text = Vec::new();
        for i in 1..=last {
            let mut r = rec(i);
            // TBS: a backward-encoded size at the end.
            let mut size = 0usize;
            for (k, b) in r.iter().rev().take(4).enumerate() {
                size |= usize::from(b & 0x7F) << (7 * k);
                if b & 0x80 != 0 {
                    break;
                }
            }
            r = &r[..r.len() - size];
            // Multibyte overlap: its count in the last byte.
            let overlap = usize::from(r[r.len() - 1] & 3);
            r = &r[..r.len() - 1 - overlap];
            text.extend(palmdoc::decompress(r));
        }
        String::from_utf8(text).unwrap()
    }

    #[test]
    fn converts_an_epub() {
        let dir = tempfile::tempdir().unwrap();
        let (src, dest) = (dir.path().join("book.epub"), dir.path().join("book.azw3"));
        sample_epub(&src);
        let report = convert(&src, &dest).unwrap();
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        let bytes = std::fs::read(&dest).unwrap();
        assert_eq!(&bytes[60..68], b"BOOKMOBI");

        // Our own MOBI reader sees the metadata and cover.
        let m = crate::metadata::mobi::read(&dest).unwrap();
        assert_eq!(m.title.as_deref(), Some("Sample Book"));
        assert_eq!(m.authors[0].name, "Ann Author");
        assert_eq!(m.uuids, ["0f0f0f0f-1111-4222-8333-444444444444"]);
        assert!(m.cover.is_some_and(|c| c.starts_with(b"\x89PNG")));

        // The text: both chapters, resources and flows referenced, links resolved.
        let text = text_of(&bytes);
        assert!(text.contains("Chapter One") && text.contains("Chapter Two") && text.contains("Paragraph 299"));
        assert!(text.contains("kindle:embed:0001?mime=image/png"), "the image");
        assert!(text.contains("kindle:flow:0001?mime=text/css"), "the stylesheet");
        assert!(text.contains("?mime=image/svg+xml"), "the inline SVG as a flow");
        assert!(text.contains("url(kindle:embed:0001?mime=image/png)"), "the CSS image");
        assert!(text.contains("&#160;"), "HTML entities made safe");
        assert!(
            !text.contains("kindle:pos:fid:0000:off:000000000"),
            "links resolved: {}",
            text.matches("kindle:pos").count()
        );
        assert!(
            text.contains("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<html xmlns=\"http://www.w3.org/1999/xhtml\"")
        );
    }

    #[test]
    fn refuses_what_isnt_an_epub() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("x.epub"), "nope").unwrap();
        assert!(convert(&dir.path().join("x.epub"), &dir.path().join("x.azw3")).is_err());
        assert!(!dir.path().join("x.azw3").exists());
    }
}
