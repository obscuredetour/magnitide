// Derived from calibre's src/calibre/ebooks/mobi/reader/mobi8.py, markup.py,
// ncx.py and containers.py, and read_font_record in mobi/utils.py
// (calibre 9.18).
// Copyright: 2012-2014, Kovid Goyal <kovid@kovidgoyal.net>. License: GPLv3.
// Ported to Rust for Magnitide, 2026.

//! KF8 (AZW3, and the KF8 half of joint MOBI files) back to EPUB: the text
//! is cut into files again from the skeleton and fragment indexes, the
//! extra flows become stylesheets and SVG images, `kindle:` references
//! become paths, and the NCX and guide indexes become the table of contents
//! and landmarks.

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::sync::LazyLock;

use regex::Regex;
use regex::bytes::Regex as BytesRegex;

use super::book::{Header, NULL, Records, u32_at};
use super::epub::{Book, Page, Resource, TocNode};
use super::html::{self, El, Node, Ns};
use super::index::{self, TagMap};

/// A skeleton: one XHTML file's frame.
struct Skel {
    frags: usize,
    start: usize,
    len: usize,
}

/// A fragment inserted into a skeleton.
struct Frag {
    insert_pos: usize,
    selector: String,
    start: usize,
    len: usize,
}

/// A rebuilt file and where it was in the text.
struct Part {
    name: String,
    start: usize,
    end: usize,
    data: Vec<u8>,
}

#[derive(Clone, PartialEq)]
enum FlowKind {
    /// A file at this path (from the book's root).
    File(String),
    /// Put into the markup where it's referenced.
    Inline,
}

struct Kf8<'a> {
    frags: Vec<Frag>,
    parts: Vec<Part>,
    flows: Vec<(FlowKind, String)>,
    resources: Vec<Option<String>>,
    linked_aids: HashSet<String>,
    header: &'a Header,
}

/// Read the KF8 book whose header is record `base`. `resource_ranges` are
/// the record ranges holding its images and fonts.
pub(super) fn read(
    records: &Records<'_>,
    base: usize,
    header: &Header,
    resource_ranges: &[(usize, usize)],
    warnings: &mut Vec<String>,
) -> Result<Book, String> {
    let text = super::book::text(records, header, base)?;
    let rec = |n: u32| base + n as usize;

    // The flow table: the text, then stylesheets and SVG images.
    let mut flow_table = vec![(0, text.len())];
    if header.fdst != NULL {
        let fdst = records.get(rec(header.fdst));
        if !fdst.starts_with(b"FDST") {
            return Err("the book's flow table is missing".into());
        }
        let start = u32_at(fdst, 4).unwrap_or(12) as usize;
        let count = u32_at(fdst, 8).unwrap_or(0) as usize;
        flow_table = (0..count)
            .filter_map(|i| Some((u32_at(fdst, start + 8 * i)? as usize, u32_at(fdst, start + 8 * i + 4)? as usize)))
            .collect();
    }
    let slice =
        |a: usize, b: usize| text.get(a.min(text.len())..b.min(text.len()).max(a.min(text.len()))).unwrap_or(&[]);
    let raw_flows: Vec<&[u8]> = flow_table.iter().map(|&(a, b)| slice(a, b)).collect();
    let main = *raw_flows.first().ok_or("the book has no text")?;

    let skels: Vec<Skel> = if header.skel == NULL {
        vec![]
    } else {
        index::read(records, rec(header.skel), header.utf8)?
            .entries
            .into_iter()
            .map(|(_, t)| Skel {
                frags: first(&t, 1).unwrap_or(0) as usize,
                start: first(&t, 6).unwrap_or(0) as usize,
                len: t.get(&6).and_then(|v| v.get(1)).copied().unwrap_or(0) as usize,
            })
            .collect()
    };
    let frags: Vec<Frag> = if header.frag == NULL {
        vec![]
    } else {
        let idx = index::read(records, rec(header.frag), header.utf8)?;
        idx.entries
            .iter()
            .map(|(name, t)| Frag {
                insert_pos: name.trim().parse().unwrap_or(0),
                selector: idx.string(first(t, 2)).unwrap_or_default().to_owned(),
                start: first(t, 6).unwrap_or(0) as usize,
                len: t.get(&6).and_then(|v| v.get(1)).copied().unwrap_or(0) as usize,
            })
            .collect()
    };
    if skels.is_empty() {
        return Err("the book's file index is missing".into());
    }

    let parts = build_parts(main, &skels, &frags, warnings);
    let flows = classify_flows(&raw_flows, header);
    let (resources, files) = extract_resources(records, resource_ranges, warnings);
    let mut kf8 = Kf8 { frags, parts, flows, resources, linked_aids: HashSet::new(), header };

    // Positions must be read before anything in the parts changes.
    let guide = if header.guide == NULL { vec![] } else { kf8.guide(records, rec(header.guide))? };
    let mut toc = if header.ncx == NULL { vec![] } else { kf8.ncx(records, rec(header.ncx), warnings)? };
    let start = kf8.start(&guide);
    let raw_parts: Vec<Vec<u8>> =
        (0..kf8.parts.len()).map(|i| kf8.resolve_positions(&kf8.parts[i].data.clone(), warnings)).collect();

    let mut pages: Vec<Page> = Vec::new();
    for (part, raw) in kf8.parts.iter().zip(raw_parts) {
        let mut root = html::parse_document(&header.decode(&raw));
        kf8.inline_flows(&mut root);
        kf8.rewrite(&mut root);
        pages.push(Page { origin: part.name.clone(), root });
    }

    let mut all_resources = files;
    for (i, (kind, flow)) in kf8.flows.iter().enumerate().skip(1) {
        let FlowKind::File(path) = kind else { continue };
        if path.ends_with(".css") {
            let css = kf8.rewrite_css(flow);
            all_resources.push(Resource { path: path.clone(), media_type: "text/css", data: css.into_bytes() });
        } else if let Some(mut svg) = html::parse_xml(flow) {
            kf8.rewrite_el(&mut svg);
            all_resources.push(Resource {
                path: path.clone(),
                media_type: "image/svg+xml",
                data: html::to_xml(&svg).into_bytes(),
            });
        } else {
            warnings.push(format!("flow {i}: an SVG image couldn't be read"));
        }
    }

    if toc.len() < 2
        && let Some((_, _, href)) = guide.iter().find(|g| g.0 == "toc")
        && let Some(inline) = inline_toc(&pages, href)
        && inline.len() > toc.len()
    {
        toc = inline;
    }

    let mut landmarks: Vec<(String, String, String)> = guide;
    if let Some(start) = start
        && !landmarks.iter().any(|g| g.0 == "text" || g.1 == "start")
    {
        landmarks.push(("text".into(), "Start".into(), start));
    }
    let cover = header
        .exth
        .cover_offset
        .and_then(|c| kf8.resources.get(c as usize).cloned().flatten())
        .filter(|p| p.starts_with("images/"));
    Ok(Book {
        pages,
        resources: all_resources,
        toc,
        guide: landmarks,
        cover,
        rtl: header.exth.page_progression.as_deref() == Some("rtl"),
    })
}

fn first(t: &TagMap, tag: u8) -> Option<u64> {
    t.get(&tag).and_then(|v| v.first()).copied()
}

/// Python's `find`: -1 when absent.
fn find(hay: &[u8], needle: u8) -> i64 {
    hay.iter().position(|&b| b == needle).map_or(-1, |p| p as i64)
}

fn rfind(hay: &[u8], needle: u8) -> i64 {
    hay.iter().rposition(|&b| b == needle).map_or(-1, |p| p as i64)
}

/// Rebuild each file: its skeleton, with its fragments put back in.
fn build_parts(text: &[u8], skels: &[Skel], frags: &[Frag], warnings: &mut Vec<String>) -> Vec<Part> {
    let get = |a: usize, b: usize| text.get(a.min(text.len())..b.min(text.len())).unwrap_or(&[]).to_vec();
    let mut parts = Vec::new();
    let mut fragptr = 0;
    for (n, s) in skels.iter().enumerate() {
        let mut baseptr = s.start + s.len;
        let mut skeleton = get(s.start, baseptr);
        let mut aid = String::new();
        let mut warned = false;
        for i in 0..s.frags {
            let Some(f) = frags.get(fragptr) else { break };
            if i == 0 {
                // "P-//*[@aid='0']" names where the first fragment goes.
                aid = f.selector.get(12..f.selector.len().saturating_sub(2)).unwrap_or_default().to_owned();
            }
            let piece = get(baseptr, baseptr + f.len);
            let mut insert = f.insert_pos.saturating_sub(s.start).min(skeleton.len());
            let (head, tail) = skeleton.split_at(insert);
            if find(tail, b'>') < find(tail, b'<') || rfind(head, b'>') < rfind(head, b'<') {
                // A broken insert position: find the tag by its aid instead.
                if !warned {
                    warnings.push(format!("file {n}: fragment positions are off; placed by their anchors"));
                    warned = true;
                }
                if let Some((bp, ep)) = tag_with_aid(&skeleton, &aid)
                    && bp != ep
                {
                    insert = (ep + 1 + f.start).min(skeleton.len());
                }
            }
            skeleton.splice(insert..insert, piece);
            baseptr += f.len;
            fragptr += 1;
        }
        parts.push(Part { name: format!("part{n:04}.xhtml"), start: s.start, end: baseptr, data: skeleton });
    }
    parts
}

/// Where the tag with this aid starts and ends.
fn tag_with_aid(ml: &[u8], aid: &str) -> Option<(usize, usize)> {
    let re = BytesRegex::new(&format!(r#"(?i)<[^>]*\said\s*=\s*['"]{}['"][^>]*>"#, regex::escape(aid))).ok()?;
    let m = re.find(ml)?;
    let gt = ml[m.start() + 1..].iter().position(|&b| b == b'>')? + m.start() + 1;
    Some((m.start(), gt))
}

/// The flows after the text: stylesheets, SVG images, and pieces that were
/// inline (a `<style>` with CDATA, an SVG with raster images inside).
fn classify_flows(raw: &[&[u8]], header: &Header) -> Vec<(FlowKind, String)> {
    static SVG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)<svg[^>]*>").expect("regex"));
    static IMAGE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)<(?:svg:)?image[^>]*>").expect("regex"));
    let mut out = vec![(FlowKind::Inline, String::new())];
    for (j, flow) in raw.iter().enumerate().skip(1) {
        let text = header.decode(flow);
        let entry = if let Some(m) = SVG.find(&text) {
            let svg = text[m.start()..].to_owned();
            if IMAGE.is_match(&svg) {
                (FlowKind::Inline, svg)
            } else {
                (FlowKind::File(format!("images/svgimg{j:04}.svg")), text)
            }
        } else if text.contains("[CDATA[") {
            let css = text.replace("<![CDATA[", "").replace("]]>", "");
            (FlowKind::Inline, css)
        } else {
            (FlowKind::File(format!("styles/{j:04}.css")), text)
        };
        out.push(entry);
    }
    out
}

/// The 1x1 GIF calibre puts where an image is missing.
const PLACEHOLDER_GIF: &[u8] = b"GIF89a\x01\x00\x01\x00\xf0\x00\x00\x00\x00\x00\xff\xff\xff!\xf9\x04\x01\x00\x00\x00\x00!\xfe calibre-placeholder-gif-for-azw3\x00,\x00\x00\x00\x00\x01\x00\x01\x00\x00\x02\x02D\x01\x00;";

/// Images and fonts. The list is by resource number (from 0; `kindle:embed`
/// counts from 1), with the path each became.
fn extract_resources(
    records: &Records<'_>,
    ranges: &[(usize, usize)],
    warnings: &mut Vec<String>,
) -> (Vec<Option<String>>, Vec<Resource>) {
    let mut map = Vec::new();
    let mut files = Vec::new();
    for &(start, end) in ranges {
        for i in start..end.min(records.len()) {
            let data = records.get(i);
            let n = map.len() + 1;
            let href = match data.get(..4).unwrap_or(&[]) {
                b"FLIS" | b"FCIS" | b"SRCS" | b"\xe9\x8e\r\n" | b"BOUN" | b"FDST" | b"DATP" | b"AUDI" | b"VIDE"
                | b"RESC" | b"CMET" | b"PAGE" | b"CONT" | b"CRES" | b"\xa0\xa0\xa0\xa0" => None,
                b"FONT" => match read_font(data) {
                    Ok((font, ext, media_type)) => {
                        let path = format!("fonts/{n:05}.{ext}");
                        files.push(Resource { path: path.clone(), media_type, data: font });
                        Some(path)
                    }
                    Err(e) => {
                        warnings.push(format!("font {n}: {e}"));
                        None
                    }
                },
                _ if data == PLACEHOLDER_GIF => None,
                _ => image_type(data).map(|(ext, media_type)| {
                    let path = format!("images/{n:05}.{ext}");
                    files.push(Resource { path: path.clone(), media_type, data: data.to_vec() });
                    path
                }),
            };
            map.push(href);
        }
    }
    (map, files)
}

pub(super) fn image_type(data: &[u8]) -> Option<(&'static str, &'static str)> {
    if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(("jpg", "image/jpeg"))
    } else if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(("png", "image/png"))
    } else if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        Some(("gif", "image/gif"))
    } else {
        None
    }
}

/// A FONT record: maybe obfuscated (XOR over its first 1040 bytes), maybe
/// zlib-compressed.
fn read_font(data: &[u8]) -> Result<(Vec<u8>, &'static str, &'static str), String> {
    let field = |at| u32_at(data, at).map(|v| v as usize).ok_or("its header is cut short");
    let (size, flags, start, xor_len, xor_start) = (field(4)?, field(8)?, field(12)?, field(16)?, field(20)?);
    let mut font = data.get(start..).ok_or("its data is missing")?.to_vec();
    if flags & 0b10 != 0 {
        let key = data.get(xor_start..xor_start + xor_len).filter(|k| !k.is_empty()).ok_or("its key is missing")?;
        let n = font.len().min(1040);
        for (i, b) in font[..n].iter_mut().enumerate() {
            *b ^= key[i % key.len()];
        }
    }
    if flags & 1 != 0 {
        let mut out = Vec::with_capacity(size);
        flate2::read::ZlibDecoder::new(&font[..])
            .take(64 << 20)
            .read_to_end(&mut out)
            .map_err(|e| format!("it couldn't be decompressed ({e})"))?;
        if out.len() != size {
            return Err("its size is wrong".into());
        }
        font = out;
    }
    match font.get(..4) {
        Some(b"\0\x01\0\0" | b"true" | b"ttcf") => Ok((font, "ttf", "font/ttf")),
        Some(b"OTTO") => Ok((font, "otf", "font/otf")),
        _ => Err("it isn't a font Magnitide knows".into()),
    }
}

static ID_RE: LazyLock<BytesRegex> =
    LazyLock::new(|| BytesRegex::new(r#"^<[^>]+\s(?:id|ID)\s*=\s*['"]([^'"]+)['"]"#).expect("regex"));
static NAME_RE: LazyLock<BytesRegex> =
    LazyLock::new(|| BytesRegex::new(r#"^<\s*a\s*\s(?:name|NAME)\s*=\s*['"]([^'"]+)['"]"#).expect("regex"));
static AID_RE: LazyLock<BytesRegex> =
    LazyLock::new(|| BytesRegex::new(r#"^<[^>]+\s(?:aid|AID)\s*=\s*['"]([^'"]+)['"]"#).expect("regex"));

/// The id a link to an aid becomes.
fn aid_id(aid: &str) -> String {
    format!("aid-{aid}")
}

impl Kf8<'_> {
    fn part_at(&self, pos: usize) -> Option<usize> {
        self.parts.iter().position(|p| pos >= p.start && pos < p.end)
    }

    /// A `kindle:pos:fid` position as (file, id).
    fn pos_fid(&mut self, fid: u64, off: u64) -> Option<(String, String)> {
        let f = self.frags.get(fid as usize)?;
        let pos = f.insert_pos + off as usize;
        let part = self.part_at(pos)?;
        Some((self.parts[part].name.clone(), self.id_before(pos)?))
    }

    /// The id (or name, or aid) of the nearest tag at or before `pos`; empty
    /// for the start of the file.
    fn id_before(&mut self, pos: usize) -> Option<String> {
        let p = self.part_at(pos)?;
        let block = &self.parts[p].data;
        let mut npos = (pos - self.parts[p].start).min(block.len());
        let rest = &block[npos..];
        let pgt = rest.iter().position(|&b| b == b'>').map_or(-1, |p| (p + npos) as i64);
        let plt = rest.iter().position(|&b| b == b'<').map_or(-1, |p| (p + npos) as i64);
        // Inside a tag: search up to its end; else from the tag before.
        if plt == npos as i64 || pgt < plt {
            npos = usize::try_from(pgt + 1).unwrap_or(0);
        }
        let mut end = npos.min(block.len());
        while let Some(gt) = block[..end].iter().rposition(|&b| b == b'>') {
            let Some(lt) = block[..gt].iter().rposition(|&b| b == b'<') else { break };
            let tag = &block[lt..=gt];
            if let Some(m) = ID_RE.captures(tag).or_else(|| NAME_RE.captures(tag)) {
                return Some(String::from_utf8_lossy(&m[1]).into_owned());
            }
            // Some books link to tags by aid alone.
            if let Some(m) = AID_RE.captures(tag) {
                let aid = String::from_utf8_lossy(&m[1]).into_owned();
                self.linked_aids.insert(aid.clone());
                return Some(aid_id(&aid));
            }
            end = lt;
        }
        Some(String::new())
    }

    fn href(&mut self, fid: u64, off: u64) -> Option<String> {
        let (file, id) = self.pos_fid(fid, off)?;
        Some(if id.is_empty() { file } else { format!("{file}#{id}") })
    }

    /// `kindle:pos:fid` links in a part as file#id.
    fn resolve_positions(&mut self, part: &[u8], warnings: &mut Vec<String>) -> Vec<u8> {
        static A: LazyLock<BytesRegex> = LazyLock::new(|| BytesRegex::new(r"(?i)<a\s[^>]*>").expect("regex"));
        static POS: LazyLock<BytesRegex> = LazyLock::new(|| {
            BytesRegex::new(r#"['"]kindle:pos:fid:([0-9A-Va-v]+):off:([0-9A-Va-v]+)[^'"]*['"]"#).expect("regex")
        });
        let mut out = Vec::with_capacity(part.len());
        let mut last = 0;
        for m in A.find_iter(part) {
            out.extend_from_slice(&part[last..m.start()]);
            let tag = m.as_bytes();
            let mut new = Vec::with_capacity(tag.len());
            let mut l = 0;
            for c in POS.captures_iter(tag) {
                let whole = c.get(0).expect("match");
                new.extend_from_slice(&tag[l..whole.start()]);
                let parse = |g: usize| u64::from_str_radix(std::str::from_utf8(&c[g]).unwrap_or("!"), 32).ok();
                let href = match (parse(1), parse(2)) {
                    (Some(fid), Some(off)) => self.href(fid, off),
                    _ => None,
                };
                let href = href.unwrap_or_else(|| {
                    warnings.push("a link points nowhere".into());
                    String::new()
                });
                new.push(b'"');
                new.extend_from_slice(html::escape_text(&href).as_bytes());
                new.push(b'"');
                l = whole.end();
            }
            new.extend_from_slice(&tag[l..]);
            out.extend(new);
            last = m.end();
        }
        out.extend_from_slice(&part[last..]);
        out
    }

    /// The guide: (type, title, href).
    fn guide(&mut self, records: &Records<'_>, at: usize) -> Result<Vec<(String, String, String)>, String> {
        let idx = index::read(records, at, self.header.utf8)?;
        let mut out = Vec::new();
        for (kind, t) in &idx.entries {
            let title = idx.string(first(t, 1)).unwrap_or(kind).to_owned();
            let Some([fid, off]) = t.get(&6).and_then(|v| <[u64; 2]>::try_from(v.as_slice()).ok()) else { continue };
            if let Some(href) = self.href(fid, off) {
                out.push((kind.clone(), title, href));
            }
        }
        Ok(out)
    }

    /// Where reading starts, from the header, when the guide doesn't say.
    fn start(&mut self, guide: &[(String, String, String)]) -> Option<String> {
        if guide.iter().any(|g| g.0 == "text" || g.1 == "start") {
            return None;
        }
        let pos = self.header.exth.start_offset? as usize;
        let part = self.part_at(pos)?;
        let file = self.parts[part].name.clone();
        let id = self.id_before(pos)?;
        Some(if id.is_empty() { file } else { format!("{file}#{id}") })
    }

    /// The table of contents from the NCX index.
    fn ncx(&mut self, records: &Records<'_>, at: usize, warnings: &mut Vec<String>) -> Result<Vec<TocNode>, String> {
        let idx = index::read(records, at, self.header.utf8)?;
        struct Entry {
            num: usize,
            level: u64,
            parent: Option<u64>,
            label: String,
            href: String,
        }
        let mut entries = Vec::new();
        for (num, (_, t)) in idx.entries.iter().enumerate() {
            let label =
                crate::convert::azw3::dom::unescape(idx.string(first(t, 3)).unwrap_or("Unknown")).trim().to_owned();
            let href = match t.get(&6).and_then(|v| <[u64; 2]>::try_from(v.as_slice()).ok()) {
                Some([fid, off]) => self.href(fid, off),
                None => first(t, 1).and_then(|pos| {
                    let pos = pos as usize;
                    let part = self.part_at(pos)?;
                    let file = self.parts[part].name.clone();
                    let id = self.id_before(pos)?;
                    Some(if id.is_empty() { file } else { format!("{file}#{id}") })
                }),
            };
            let Some(href) = href else {
                warnings.push(format!("table of contents entry “{label}” points nowhere; left out"));
                continue;
            };
            entries.push(Entry { num, level: first(t, 4).unwrap_or(0), parent: first(t, 21), label, href });
        }
        // Built level by level, each entry under its parent.
        let mut levels: Vec<u64> = entries.iter().map(|e| e.level).collect();
        levels.sort_unstable();
        levels.dedup();
        // Paths from the root to each entry, by number.
        let mut paths: HashMap<usize, Vec<usize>> = HashMap::new();
        let mut toc: Vec<TocNode> = Vec::new();
        for level in levels {
            for e in entries.iter().filter(|e| e.level == level) {
                let node = TocNode { label: e.label.clone(), href: e.href.clone(), children: vec![] };
                let parent_path = e.parent.and_then(|p| paths.get(&(p as usize))).cloned();
                let path = match parent_path {
                    Some(pp) => {
                        let mut list = &mut toc;
                        for &i in &pp {
                            list = &mut list[i].children;
                        }
                        list.push(node);
                        let mut p = pp;
                        p.push(list.len() - 1);
                        p
                    }
                    None => {
                        toc.push(node);
                        vec![toc.len() - 1]
                    }
                };
                paths.insert(e.num, path);
            }
        }
        Ok(toc)
    }

    /// Elements that pull in an inline flow (an SVG, a `<style>`) are
    /// replaced by it.
    fn inline_flows(&self, root: &mut El) {
        root.rebuild(&mut |e| {
            let flow = ["src", "href", "xlink:href", "data"]
                .iter()
                .filter_map(|k| e.attr(k))
                .find_map(|v| kindle_ref(v).filter(|r| r.0 == "flow").map(|r| r.1));
            let Some((FlowKind::Inline, text)) = flow.and_then(|n| self.flows.get(n as usize)) else {
                return vec![Node::El(e)];
            };
            if text.trim_start().to_ascii_lowercase().starts_with("<svg") {
                html::parse_xml(text).map(|svg| vec![Node::El(svg)]).unwrap_or_default()
            } else {
                vec![Node::El(El::with("style", &[("type", "text/css")], vec![Node::Text(text.clone())]))]
            }
        });
    }

    /// `kindle:` references as paths, aids gone (or ids, if linked to).
    fn rewrite(&self, root: &mut El) {
        root.walk_mut(&mut |e| self.rewrite_one(e));
    }

    fn rewrite_el(&self, el: &mut El) {
        self.rewrite_one(el);
        el.walk_mut(&mut |e| self.rewrite_one(e));
    }

    fn rewrite_one(&self, e: &mut El) {
        if e.ns == Ns::Html {
            for key in ["aid", "cid"] {
                if let Some(aid) = e.remove_attr(key)
                    && self.linked_aids.contains(&aid)
                    && e.attr("id").is_none()
                {
                    e.set_attr("id", &aid_id(&aid));
                }
            }
            let page_break = e.attrs.iter().position(|(k, _)| k.eq_ignore_ascii_case("data-amznpagebreak"));
            if let Some(i) = page_break {
                let (_, v) = e.attrs.remove(i);
                e.add_style(&format!("page-break-after: {v}"));
            }
            e.attrs.retain(|(k, _)| !k.to_ascii_lowercase().starts_with("data-amzn"));
        }
        for (k, v) in &mut e.attrs {
            if k == "style" {
                if v.contains("kindle:") {
                    *v = self.rewrite_css(v);
                }
            } else if v.starts_with("kindle:")
                && let Some(path) = self.path_of(v)
            {
                *v = format!("../{path}");
            }
        }
        if e.is("style") {
            for c in &mut e.children {
                if let Node::Text(t) = c {
                    *t = self.rewrite_css(t);
                }
            }
        }
    }

    /// Where a `kindle:embed` or `kindle:flow` reference now is.
    fn path_of(&self, url: &str) -> Option<String> {
        let (kind, n) = kindle_ref(url)?;
        if kind == "embed" {
            n.checked_sub(1).and_then(|i| self.resources.get(i as usize)).cloned().flatten()
        } else {
            match self.flows.get(n as usize) {
                Some((FlowKind::File(p), _)) if n > 0 => Some(p.clone()),
                _ => None,
            }
        }
    }

    /// `url(kindle:…)` in CSS as paths (from a folder beside `images`).
    fn rewrite_css(&self, css: &str) -> String {
        static URL: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r#"(?i)url\(\s*(?:"(kindle:[^"]*)"|'(kindle:[^']*)'|(kindle:[^'"()\s]*))\s*\)"#).expect("regex")
        });
        URL.replace_all(css, |c: &regex::Captures<'_>| {
            let (quote, url) = match (c.get(1), c.get(2), c.get(3)) {
                (Some(u), _, _) => ("\"", u.as_str()),
                (_, Some(u), _) => ("'", u.as_str()),
                (_, _, Some(u)) => ("", u.as_str()),
                _ => return c[0].to_owned(),
            };
            match self.path_of(url) {
                Some(p) => format!("url({quote}../{p}{quote})"),
                None => c[0].to_owned(),
            }
        })
        .into_owned()
    }
}

/// ("embed" or "flow", number) from a `kindle:` reference.
fn kindle_ref(url: &str) -> Option<(&'static str, u64)> {
    let lower = url.trim().to_ascii_lowercase();
    let (kind, rest) = match lower.strip_prefix("kindle:embed:") {
        Some(r) => ("embed", r),
        None => ("flow", lower.strip_prefix("kindle:flow:")?),
    };
    let digits: String = rest.chars().take_while(char::is_ascii_alphanumeric).collect();
    Some((kind, u64::from_str_radix(&digits, 32).ok()?))
}

/// A table of contents read from the book's own contents page, for books
/// whose NCX index is missing: its links, nested as the page nests them.
fn inline_toc(pages: &[Page], href: &str) -> Option<Vec<TocNode>> {
    let (file, frag) = href.split_once('#').unwrap_or((href, ""));
    let page = pages.iter().find(|p| p.origin == file)?;
    let body = page.root.child("body")?;
    let mut links: Vec<(String, String, usize)> = Vec::new();
    let mut reached = frag.is_empty();
    fn walk(el: &El, depth: usize, frag: &str, reached: &mut bool, file: &str, out: &mut Vec<(String, String, usize)>) {
        for c in &el.children {
            let Node::El(e) = c else { continue };
            if !*reached && e.attr("id") == Some(frag) {
                *reached = true;
            }
            if *reached
                && e.is("a")
                && let Some(h) = e.attr("href").filter(|h| !h.contains("://") && !h.starts_with("mailto:"))
            {
                let target = if h.starts_with('#') { format!("{file}{h}") } else { h.to_owned() };
                let label = e.text().split_whitespace().collect::<Vec<_>>().join(" ");
                if !label.is_empty() && !out.iter().any(|(l, t, _)| *l == label && *t == target) {
                    out.push((label, target, depth));
                }
            }
            walk(e, depth + 1, frag, reached, file, out);
        }
    }
    walk(body, 0, frag, &mut reached, file, &mut links);
    if links.is_empty() {
        return None;
    }
    let mut depths: Vec<usize> = links.iter().map(|l| l.2).collect();
    depths.sort_unstable();
    depths.dedup();
    // Nest by depth rank, as the page nests the links.
    let mut toc: Vec<TocNode> = Vec::new();
    let mut stack: Vec<usize> = Vec::new(); // depth ranks of the open path
    for (label, href, depth) in links {
        let rank = depths.iter().position(|d| *d == depth).unwrap_or(0);
        while stack.last().is_some_and(|r| *r >= rank) {
            stack.pop();
        }
        let mut list = &mut toc;
        for _ in 0..stack.len() {
            list = &mut list.last_mut().expect("open").children;
        }
        list.push(TocNode { label, href, children: vec![] });
        stack.push(rank);
    }
    Some(toc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_kindle_references() {
        assert_eq!(kindle_ref("kindle:embed:0001?mime=image/jpeg"), Some(("embed", 1)));
        assert_eq!(kindle_ref("kindle:flow:000A?mime=text/css"), Some(("flow", 10)));
        assert_eq!(kindle_ref("KINDLE:EMBED:00ZZ"), None);
        assert_eq!(kindle_ref("images/x.jpg"), None);
    }

    #[test]
    fn rebuilds_files_from_skeletons() {
        let text = b"<html><body></body></html><p>One</p><html><body></body></html><p>Two</p>";
        let skels = [Skel { frags: 1, start: 0, len: 26 }, Skel { frags: 1, start: 36, len: 26 }];
        let frags = [
            Frag { insert_pos: 12, selector: "P-//*[@aid='0']".into(), start: 0, len: 10 },
            Frag { insert_pos: 48, selector: "P-//*[@aid='1']".into(), start: 0, len: 10 },
        ];
        let parts = build_parts(text, &skels, &frags, &mut vec![]);
        assert_eq!(parts[0].data, b"<html><body><p>One</p></body></html>");
        assert_eq!(parts[1].data, b"<html><body><p>Two</p></body></html>");
        assert_eq!((parts[1].start, parts[1].end), (36, 72));
    }
}
