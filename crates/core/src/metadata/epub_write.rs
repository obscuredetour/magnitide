//! Writing metadata (and the cover) into an EPUB.
//!
//! Only the OPF's `<metadata>` element is regenerated: the rest of the OPF
//! (manifest, spine, guide) is kept byte for byte, apart from the cover's
//! manifest entry when the cover changes. Elements Magnitide doesn't manage
//! (rights, contributors, other `<meta>`, the package's unique identifier,
//! which font obfuscation depends on) are kept as they were.
//!
//! Every other zip entry is copied without recompressing it, so `mimetype`
//! stays first and stored. The new file is written beside the original,
//! read back to check it, and only then renamed over it: a failure at any
//! point leaves the original untouched.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Write};
use std::ops::Range;
use std::path::{Path, PathBuf};

use roxmltree::{Node, NodeId};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use super::epub::{
    LocalName, MAX_XML, Res, Zip, attr, child, declared_cover, identifier, is_author, opf_path, parse_xml, read_entry,
    resolve, text,
};
use super::{BookMetadata, language_tag};

const DC: &str = "http://purl.org/dc/elements/1.1/";
const OPF: &str = "http://www.idpf.org/2007/opf";

/// Write `meta` into the EPUB at `path`. `title_sort` goes in alongside the
/// title. `meta.uuids` are added as identifiers if the file lacks them. `meta.cover`, if set, must be JPEG bytes; None leaves the file's
/// cover alone. Empty `meta.languages` keeps the file's languages (EPUB
/// requires one).
pub fn write(path: &Path, meta: &BookMetadata, title_sort: Option<&str>) -> Res<()> {
    let title = meta.title.as_deref().map(str::trim).filter(|t| !t.is_empty()).ok_or("a book needs a title")?;
    let mut zip = ZipArchive::new(BufReader::new(File::open(path)?))?;
    let opf_path = opf_path(&mut zip)?;
    let opf = String::from_utf8(read_entry(&mut zip, &opf_path, MAX_XML)?)?;
    let modified = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let edit = rewrite_opf(&opf, &opf_path, meta, title_sort, &modified)?;

    let tmp = temp_path(path);
    let result = write_zip(&mut zip, &tmp, &opf_path, &edit, meta.cover.as_deref()).and_then(|()| {
        // Read it back: it must open as an EPUB and say what we wrote.
        let back = super::epub::read(&tmp).ok_or("the rewritten file couldn't be read back")?;
        let names = |m: &BookMetadata| m.authors.iter().map(|a| a.name.clone()).collect::<Vec<_>>();
        if back.title.as_deref() != Some(title) || names(&back) != names(meta) {
            return Err("the rewritten file didn't read back as written".into());
        }
        if meta.cover.is_some() && back.cover != meta.cover {
            return Err("the rewritten file's cover didn't read back as written".into());
        }
        Ok(())
    });
    match result {
        Ok(()) => {
            drop(zip);
            fs::rename(&tmp, path)?;
            Ok(())
        }
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            Err(e)
        }
    }
}

fn temp_path(path: &Path) -> PathBuf {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    path.with_file_name(format!(".{name}.magnitide-tmp"))
}

/// The new OPF and what happens to the cover image.
struct OpfEdit {
    opf: String,
    cover: CoverPlan,
}

enum CoverPlan {
    Keep,
    /// Overwrite this zip entry.
    Replace(String),
    /// Add this zip entry (the OPF already lists it).
    Add(String),
}

fn rewrite_opf(
    opf: &str,
    opf_path: &str,
    meta: &BookMetadata,
    title_sort: Option<&str>,
    modified: &str,
) -> Res<OpfEdit> {
    let src = opf.trim_start_matches('\u{feff}');
    let doc = parse_xml(src)?;
    let package = doc.root_element();
    let epub3 = attr(package, "version").is_some_and(|v| v.trim().starts_with('3'));
    let unique_id = attr(package, "unique-identifier");
    let metadata = child(package, "metadata").ok_or("no <metadata>")?;
    let manifest = child(package, "manifest").ok_or("no <manifest>")?;

    let mut ids: HashSet<String> = doc.descendants().filter_map(|n| attr(n, "id")).map(str::to_owned).collect();
    let mut fresh = |base: &str| {
        let id =
            (1..).map(|n| if n == 1 { base.to_owned() } else { format!("{base}-{n}") }).find(|id| !ids.contains(id));
        let id = id.expect("unbounded");
        ids.insert(id.clone());
        id
    };

    // Splices into the OPF outside <metadata> (manifest changes).
    let mut splices: Vec<(Range<usize>, String)> = Vec::new();
    let mut cover_meta = None;
    let cover = match meta.cover {
        None => CoverPlan::Keep,
        Some(_) => match declared_cover(metadata, manifest).filter(|item| attr(*item, "href").is_some()) {
            Some(item) => {
                if attr(item, "media-type") != Some("image/jpeg")
                    && let Some(a) = item.attributes().find(|a| a.name() == "media-type")
                {
                    splices.push((a.range_value(), "image/jpeg".into()));
                }
                CoverPlan::Replace(resolve(opf_path, attr(item, "href").unwrap_or_default()))
            }
            None => {
                let hrefs: HashSet<&str> = manifest.children().filter_map(|n| attr(n, "href")).collect();
                let href = (1..)
                    .map(|n| if n == 1 { "magnitide-cover.jpg".to_owned() } else { format!("magnitide-cover-{n}.jpg") })
                    .find(|h| !hrefs.contains(h.as_str()))
                    .expect("unbounded");
                let id = fresh("magnitide-cover");
                let properties = if epub3 { r#" properties="cover-image""# } else { "" };
                let end = manifest.last_child().ok_or("empty <manifest>")?.range().end;
                let indent = indent_of(manifest).0;
                splices.push((
                    end..end,
                    format!(r#"{indent}<item id="{id}" href="{href}" media-type="image/jpeg"{properties}/>"#),
                ));
                cover_meta = Some(id);
                CoverPlan::Add(resolve(opf_path, &href))
            }
        },
    };

    // Which existing <metadata> children we replace.
    let refines = |n: Node<'_, '_>, property: &str| -> Option<String> {
        let id = attr(n, "id")?;
        metadata
            .children()
            .find(|m| {
                m.has_tag_name_local("meta")
                    && attr(*m, "refines").map(|r| r.trim_start_matches('#')) == Some(id)
                    && attr(*m, "property") == Some(property)
            })
            .map(text)
    };
    let manage_languages = !meta.languages.is_empty();
    let managed = |n: Node<'_, '_>| -> bool {
        let dc = n.tag_name().namespace() == Some(DC);
        match n.tag_name().name() {
            "title" | "publisher" | "subject" | "description" if dc => true,
            "language" if dc => manage_languages,
            "creator" if dc => is_author(n, metadata),
            "date" if dc => attr(n, "event").is_none_or(|e| e.contains("publication")),
            "identifier" if dc => attr(n, "id") != unique_id && identifier(attr(n, "scheme"), &text(n)).is_some(),
            "meta" if !dc => match (attr(n, "name"), attr(n, "property")) {
                (Some("calibre:series" | "calibre:series_index" | "calibre:title_sort"), _) => true,
                (Some("cover"), _) => cover_meta.is_some(),
                (_, Some("dcterms:modified")) => epub3,
                (_, Some("belongs-to-collection")) => refines(n, "collection-type").is_none_or(|t| t == "series"),
                _ => false,
            },
            _ => false,
        }
    };
    let mut dropped: HashSet<NodeId> = HashSet::new();
    let mut dropped_ids: HashSet<&str> = HashSet::new();
    for n in metadata.children().filter(Node::is_element).filter(|n| managed(*n)) {
        dropped.insert(n.id());
        dropped_ids.extend(attr(n, "id"));
    }
    // Refinements of what we dropped go too.
    for n in metadata.children().filter(|n| n.has_tag_name_local("meta")) {
        if attr(n, "refines").is_some_and(|r| dropped_ids.contains(r.trim_start_matches('#'))) {
            dropped.insert(n.id());
        }
    }

    // Prefixes in scope, declaring any that are missing on <metadata>.
    let mut declare = String::new();
    let dc = match metadata.lookup_prefix(DC) {
        Some(p) => p.to_owned(),
        None => {
            declare.push_str(&format!(r#" xmlns:dc="{DC}""#));
            "dc".to_owned()
        }
    };
    let opf_prefix = match metadata.lookup_prefix(OPF) {
        Some(p) => p.to_owned(),
        None if epub3 => String::new(), // EPUB 3 output uses no opf: attributes.
        None => {
            declare.push_str(&format!(r#" xmlns:opf="{OPF}""#));
            "opf".to_owned()
        }
    };

    // The new elements, in reading order.
    let mut lines: Vec<String> = Vec::new();
    let title = meta.title.as_deref().unwrap_or_default().trim();
    let sort = title_sort.map(str::trim).filter(|s| !s.is_empty() && *s != title);
    if epub3 {
        let id = fresh("title");
        lines.push(format!(r#"<{dc}:title id="{id}">{}</{dc}:title>"#, esc(title)));
        if let Some(sort) = sort {
            lines.push(format!(r##"<meta refines="#{id}" property="file-as">{}</meta>"##, esc(sort)));
        }
    } else {
        lines.push(format!("<{dc}:title>{}</{dc}:title>", esc(title)));
        if let Some(sort) = sort {
            lines.push(format!(r#"<meta name="calibre:title_sort" content="{}"/>"#, esc(sort)));
        }
    }
    for (i, author) in meta.authors.iter().enumerate() {
        let sort = author.sort.as_deref().map(str::trim).filter(|s| !s.is_empty());
        if epub3 {
            let id = fresh(&format!("creator{}", i + 1));
            lines.push(format!(r#"<{dc}:creator id="{id}">{}</{dc}:creator>"#, esc(&author.name)));
            lines.push(format!(r##"<meta refines="#{id}" property="role" scheme="marc:relators">aut</meta>"##));
            if let Some(sort) = sort {
                lines.push(format!(r##"<meta refines="#{id}" property="file-as">{}</meta>"##, esc(sort)));
            }
        } else {
            let file_as = sort.map(|s| format!(r#" {opf_prefix}:file-as="{}""#, esc(s))).unwrap_or_default();
            lines.push(format!(
                r#"<{dc}:creator {opf_prefix}:role="aut"{file_as}>{}</{dc}:creator>"#,
                esc(&author.name)
            ));
        }
    }
    if let Some(p) = &meta.publisher {
        lines.push(format!("<{dc}:publisher>{}</{dc}:publisher>", esc(p)));
    }
    if let Some(d) = meta.published {
        lines.push(format!("<{dc}:date>{}</{dc}:date>", d.format("%Y-%m-%d")));
    }
    for l in &meta.languages {
        lines.push(format!("<{dc}:language>{}</{dc}:language>", esc(&language_tag(l))));
    }
    let kept_ids: HashSet<String> = metadata
        .children()
        .filter(|n| n.has_tag_name_local("identifier") && !dropped.contains(&n.id()))
        .filter_map(|n| identifier(attr(n, "scheme"), &text(n)))
        .map(|(s, v)| format!("{s}:{v}"))
        .collect();
    for (scheme, value) in &meta.identifiers {
        if kept_ids.contains(&format!("{scheme}:{value}")) {
            continue;
        }
        let (scheme, value) = (esc(scheme), esc(value));
        lines.push(match (epub3, scheme.as_str()) {
            (true, "isbn") => format!("<{dc}:identifier>urn:isbn:{value}</{dc}:identifier>"),
            (true, _) => format!("<{dc}:identifier>{scheme}:{value}</{dc}:identifier>"),
            (false, "isbn") => format!(r#"<{dc}:identifier {opf_prefix}:scheme="ISBN">{value}</{dc}:identifier>"#),
            (false, _) => format!(r#"<{dc}:identifier {opf_prefix}:scheme="{scheme}">{value}</{dc}:identifier>"#),
        });
    }
    // Book ids (sending to a device), unless the file already carries them.
    let carried: Vec<String> = metadata
        .children()
        .filter(|n| n.has_tag_name_local("identifier"))
        .map(|n| text(n).to_ascii_lowercase())
        .collect();
    for uuid in &meta.uuids {
        if carried.iter().any(|c| c.ends_with(&uuid.to_ascii_lowercase())) {
            continue;
        }
        lines.push(if epub3 {
            format!("<{dc}:identifier>urn:uuid:{}</{dc}:identifier>", esc(uuid))
        } else {
            format!(r#"<{dc}:identifier {opf_prefix}:scheme="calibre">{}</{dc}:identifier>"#, esc(uuid))
        });
    }
    for tag in &meta.tags {
        lines.push(format!("<{dc}:subject>{}</{dc}:subject>", esc(tag)));
    }
    if let Some(d) = &meta.description {
        lines.push(format!("<{dc}:description>{}</{dc}:description>", esc(d)));
    }
    if let Some((name, index)) = &meta.series {
        let index = series_index(*index);
        if epub3 {
            let id = fresh("series");
            lines.push(format!(r#"<meta property="belongs-to-collection" id="{id}">{}</meta>"#, esc(name)));
            lines.push(format!(r##"<meta refines="#{id}" property="collection-type">series</meta>"##));
            lines.push(format!(r##"<meta refines="#{id}" property="group-position">{index}</meta>"##));
        }
        // calibre's convention, which most readers (KOReader included) understand.
        lines.push(format!(r#"<meta name="calibre:series" content="{}"/>"#, esc(name)));
        lines.push(format!(r#"<meta name="calibre:series_index" content="{index}"/>"#));
    }
    if let Some(id) = &cover_meta {
        lines.push(format!(r#"<meta name="cover" content="{id}"/>"#));
    }
    if epub3 {
        lines.push(format!(r#"<meta property="dcterms:modified">{modified}</meta>"#));
    }

    // Reassemble <metadata>: its start tag, our elements, then what we keep.
    let range = metadata.range();
    let (start_tag, end_tag) = match (metadata.first_child(), metadata.last_child()) {
        (Some(first), Some(last)) => (&src[range.start..first.range().start], &src[last.range().end..range.end]),
        _ => (&src[range.clone()], ""),
    };
    let (start_tag, end_tag) = if end_tag.is_empty() {
        // <metadata/>: open it up.
        let open = start_tag.trim_end_matches('>').trim_end_matches('/');
        let qname: String = open[1..].chars().take_while(|c| !c.is_whitespace()).collect();
        (format!("{open}>"), format!("</{qname}>"))
    } else {
        (start_tag.to_owned(), end_tag.to_owned())
    };
    let start_tag = match start_tag.strip_suffix('>') {
        Some(open) if !declare.is_empty() => format!("{open}{declare}>"),
        _ => start_tag,
    };
    let (indent, closing) = indent_of(metadata);
    let mut out = start_tag;
    for line in &lines {
        out.push_str(&indent);
        out.push_str(line);
    }
    for n in metadata.children().filter(|n| (n.is_element() && !dropped.contains(&n.id())) || n.is_comment()) {
        out.push_str(&indent);
        out.push_str(&src[n.range()]);
    }
    out.push_str(&closing);
    out.push_str(&end_tag);
    splices.push((range, out));

    // Apply back to front so earlier ranges stay valid.
    splices.sort_by_key(|(r, _)| std::cmp::Reverse(r.start));
    let mut opf = src.to_owned();
    for (r, with) in splices {
        opf.replace_range(r, &with);
    }
    Ok(OpfEdit { opf, cover })
}

/// The line break + indentation used for an element's children, and before
/// its end tag.
fn indent_of(node: Node<'_, '_>) -> (String, String) {
    let after_newline = |n: Node<'_, '_>| n.text().and_then(|t| t.rfind('\n').map(|i| t[i..].to_owned()));
    let inner = node.children().filter(Node::is_text).find_map(after_newline).unwrap_or_else(|| "\n    ".into());
    let closing = node.last_child().filter(Node::is_text).and_then(after_newline).unwrap_or_else(|| "\n  ".into());
    (inner, closing)
}

/// "3", "2.5".
fn series_index(i: f64) -> String {
    if i.fract() == 0.0 { format!("{i:.0}") } else { format!("{i}") }
}

/// Escape text for XML content and attribute values.
fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            // Attributes are always double-quoted, so ' needs no escape.
            '"' => out.push_str("&quot;"),
            // Control characters aren't allowed in XML 1.0.
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
            c => out.push(c),
        }
    }
    out
}

fn write_zip(src: &mut Zip, dest: &Path, opf_path: &str, edit: &OpfEdit, cover: Option<&[u8]>) -> Res<()> {
    let mut out = ZipWriter::new(BufWriter::new(File::create(dest)?));
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    // JPEGs don't compress.
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let replace = match &edit.cover {
        CoverPlan::Replace(name) => Some(name.as_str()),
        _ => None,
    };
    let mut replaced = false;
    for i in 0..src.len() {
        let entry = src.by_index_raw(i)?;
        let name = entry.name().to_owned();
        if name == opf_path {
            drop(entry);
            out.start_file(name, deflated)?;
            out.write_all(edit.opf.as_bytes())?;
        } else if replace == Some(name.as_str())
            && let Some(cover) = cover
        {
            drop(entry);
            out.start_file(name, stored)?;
            out.write_all(cover)?;
            replaced = true;
        } else {
            out.raw_copy_file(entry)?;
        }
    }
    // A new cover, or a declared one whose image was missing from the zip.
    let add = match &edit.cover {
        CoverPlan::Add(name) => Some(name),
        CoverPlan::Replace(name) if !replaced => Some(name),
        _ => None,
    };
    if let (Some(name), Some(cover)) = (add, cover) {
        out.start_file(name.as_str(), stored)?;
        out.write_all(cover)?;
    }
    let file = out.finish()?.into_inner().map_err(|e| e.into_error())?;
    file.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::io::Read;

    use chrono::NaiveDate;

    use super::super::Author;
    use super::super::epub::read;
    use super::super::epub::tests::{EPUB2_OPF, MIXED_ROLES_OPF, make_epub};
    use super::*;

    const JPEG: &[u8] = b"\xFF\xD8new-cover";

    fn wanted() -> BookMetadata {
        BookMetadata {
            title: Some("The Dispossessed: An Ambiguous Utopia".into()),
            authors: vec![
                Author { name: "Ursula K. Le Guin".into(), sort: Some("Le Guin, Ursula K.".into()) },
                Author { name: "A. N. Other".into(), sort: Some("Other, A. N.".into()) },
            ],
            series: Some(("Hainish Cycle".into(), 5.5)),
            publisher: Some("Gollancz & Sons".into()),
            published: NaiveDate::from_ymd_opt(1975, 3, 2),
            languages: vec!["eng".into(), "fra".into()],
            identifiers: BTreeMap::from([("isbn".into(), "9780575094765".into()), ("google".into(), "abc<1>".into())]),
            description: Some("<p>Revised <i>blurb</i>.</p>".into()),
            tags: vec!["Fiction".into(), "Utopias".into()],
            cover: None,
            uuids: vec![],
            comic: None,
        }
    }

    fn entries(path: &Path) -> Vec<(String, CompressionMethod, Vec<u8>)> {
        let mut zip = ZipArchive::new(File::open(path).unwrap()).unwrap();
        (0..zip.len())
            .map(|i| {
                let mut e = zip.by_index(i).unwrap();
                let mut bytes = Vec::new();
                e.read_to_end(&mut bytes).unwrap();
                (e.name().to_owned(), e.compression(), bytes)
            })
            .collect()
    }

    fn opf_text(path: &Path) -> String {
        let all = entries(path);
        String::from_utf8(all.into_iter().find(|e| e.0.ends_with(".opf")).unwrap().2).unwrap()
    }

    fn assert_reads_back(path: &Path, want: &BookMetadata) {
        let got = read(path).unwrap();
        assert_eq!(got.title, want.title);
        assert_eq!(got.authors, want.authors);
        assert_eq!(got.series, want.series);
        assert_eq!(got.publisher, want.publisher);
        assert_eq!(got.published, want.published);
        assert_eq!(got.languages, want.languages);
        assert_eq!(got.identifiers, want.identifiers);
        assert_eq!(got.description, want.description);
        assert_eq!(got.tags, want.tags);
    }

    #[test]
    fn epub2_round_trips_and_keeps_what_it_doesnt_manage() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("book.epub");
        let opf2 = EPUB2_OPF.replace(
            "<meta name=\"cover\"",
            "<dc:rights>All rights reserved</dc:rights>\n    <!-- keep me -->\n    <meta name=\"custom\" content=\"x\"/>\n    <meta name=\"cover\"",
        );
        make_epub(&path, &opf2, Some(("images/My Cover.jpg", b"\xFF\xD8old")));
        let before = entries(&path);

        write(&path, &wanted(), Some("Dispossessed, The")).unwrap();
        assert_reads_back(&path, &wanted());

        let after = entries(&path);
        assert_eq!(after[0].0, "mimetype");
        assert_eq!(after[0].1, CompressionMethod::Stored);
        // Everything but the OPF is unchanged, the cover included (none given).
        assert_eq!(after.len(), before.len());
        for (a, b) in after.iter().zip(&before) {
            if !a.0.ends_with(".opf") {
                assert_eq!(a, b);
            }
        }
        let opf = opf_text(&path);
        for kept in [
            "<dc:rights>All rights reserved</dc:rights>",
            "<!-- keep me -->",
            r#"<meta name="custom" content="x"/>"#,
            r#"<dc:creator opf:role="ill">Some Illustrator</dc:creator>"#,
            "urn:uuid:5c4b5b2c-0000-4000-8000-000000000000",
            r#"<meta name="cover" content="cover-img"/>"#,
            r#"<item id="cover-img" href="images/My%20Cover.jpg" media-type="image/jpeg"/>"#,
            r#"<meta name="calibre:title_sort" content="Dispossessed, The"/>"#,
        ] {
            assert!(opf.contains(kept), "missing {kept} in\n{opf}");
        }
        for gone in ["Science Fiction", "Harper", "978-0-06-105488-4", r#"content="5""#] {
            assert!(!opf.contains(gone), "{gone} still in\n{opf}");
        }
        assert_eq!(opf.matches("<dc:title>").count(), 1);
        assert!(!dir.path().join(".book.epub.magnitide-tmp").exists());

        // Writing again is stable.
        write(&path, &wanted(), Some("Dispossessed, The")).unwrap();
        assert_reads_back(&path, &wanted());
        assert_eq!(opf_text(&path).matches("calibre:series\"").count(), 1);
    }

    const EPUB3_OPF: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="pub-id" prefix="rendition: http://www.idpf.org/vocab/rendition/#">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="pub-id">urn:uuid:11111111-2222-4333-8444-555555555555</dc:identifier>
    <dc:title id="t">Wrong Title</dc:title>
    <meta refines="#t" property="title-type">main</meta>
    <dc:creator id="c1">Wrong Author</dc:creator>
    <meta refines="#c1" property="role" scheme="marc:relators">aut</meta>
    <dc:creator id="c2">Kept Translator</dc:creator>
    <meta refines="#c2" property="role" scheme="marc:relators">trl</meta>
    <dc:language>en</dc:language>
    <meta property="belongs-to-collection" id="col">Old Series</meta>
    <meta refines="#col" property="collection-type">series</meta>
    <meta property="belongs-to-collection" id="set">Box Set</meta>
    <meta refines="#set" property="collection-type">set</meta>
    <meta property="dcterms:modified">2020-01-01T00:00:00Z</meta>
    <meta property="rendition:layout">reflowable</meta>
  </metadata>
  <manifest>
    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
    <item id="img" href="images/cover.png" media-type="image/png" properties="cover-image"/>
  </manifest>
  <spine><itemref idref="nav"/></spine>
</package>"##;

    #[test]
    fn epub3_round_trips_with_refines_and_replaces_a_png_cover() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("book.epub");
        make_epub(&path, EPUB3_OPF, Some(("images/cover.png", b"\x89PNG")));
        let want = BookMetadata { cover: Some(JPEG.to_vec()), ..wanted() };

        write(&path, &want, None).unwrap();
        assert_reads_back(&path, &want);
        assert_eq!(read(&path).unwrap().cover.as_deref(), Some(JPEG));

        let opf = opf_text(&path);
        for kept in [
            r#"<dc:identifier id="pub-id">urn:uuid:11111111-2222-4333-8444-555555555555</dc:identifier>"#,
            r#"<dc:creator id="c2">Kept Translator</dc:creator>"#,
            r##"<meta refines="#c2" property="role" scheme="marc:relators">trl</meta>"##,
            r#"<meta property="belongs-to-collection" id="set">Box Set</meta>"#,
            r#"<meta property="rendition:layout">reflowable</meta>"#,
            r#"<item id="img" href="images/cover.png" media-type="image/jpeg" properties="cover-image"/>"#,
            "<spine><itemref idref=\"nav\"/></spine>",
            "<dc:identifier>urn:isbn:9780575094765</dc:identifier>",
            "<dc:identifier>google:abc&lt;1&gt;</dc:identifier>",
            "<dc:language>fr</dc:language>",
            r##"<meta refines="#series" property="group-position">5.5</meta>"##,
        ] {
            assert!(opf.contains(kept), "missing {kept} in\n{opf}");
        }
        for gone in ["Wrong", "Old Series", "2020-01-01", "title-type"] {
            assert!(!opf.contains(gone), "{gone} still in\n{opf}");
        }
        assert_eq!(opf.matches("dcterms:modified").count(), 1);
        // The package's identity is untouched.
        assert!(opf.contains(r#"unique-identifier="pub-id""#));
    }

    #[test]
    fn replaces_creators_with_aut_among_several_roles() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("book.epub");
        make_epub(&path, MIXED_ROLES_OPF, None);

        write(&path, &wanted(), None).unwrap();
        // Read back without duplicates: every old author was replaced.
        assert_reads_back(&path, &wanted());

        let opf = opf_text(&path);
        for kept in [
            r#"<dc:creator id="c3">Only Illustrator</dc:creator>"#,
            r##"<meta refines="#c3" property="role" scheme="marc:relators">ill</meta>"##,
        ] {
            assert!(opf.contains(kept), "missing {kept} in\n{opf}");
        }
        for gone in ["Arthur Conan Doyle", "Doyle, Arthur Conan", "Second Author", "No Role", ">ann<", ">dto<"] {
            assert!(!opf.contains(gone), "{gone} still in\n{opf}");
        }
    }

    #[test]
    fn adds_a_cover_when_there_is_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("book.epub");
        let no_cover = EPUB3_OPF.replace(r#" properties="cover-image""#, "");
        make_epub(&path, &no_cover, None);
        let want = BookMetadata { cover: Some(JPEG.to_vec()), ..wanted() };

        write(&path, &want, None).unwrap();
        assert_eq!(read(&path).unwrap().cover.as_deref(), Some(JPEG));
        let opf = opf_text(&path);
        assert!(opf.contains(
            r#"<item id="magnitide-cover" href="magnitide-cover.jpg" media-type="image/jpeg" properties="cover-image"/>"#
        ));
        let all = entries(&path);
        let added = all.iter().find(|e| e.0 == "OEBPS/magnitide-cover.jpg").unwrap();
        assert_eq!(added.1, CompressionMethod::Stored);
    }

    #[test]
    fn adds_book_uuids_once() {
        let dir = tempfile::tempdir().unwrap();
        for (name, opf) in [("2.epub", EPUB2_OPF), ("3.epub", EPUB3_OPF)] {
            let path = dir.path().join(name);
            make_epub(&path, opf, None);
            let uuid = "0f0f0f0f-1111-4222-8333-444444444444".to_owned();
            let want = BookMetadata { uuids: vec![uuid.clone()], ..wanted() };
            write(&path, &want, None).unwrap();
            write(&path, &want, None).unwrap();
            let got = read(&path).unwrap();
            assert!(got.uuids.contains(&uuid), "{name}: {:?}", got.uuids);
            assert_eq!(opf_text(&path).matches(&uuid).count(), 1, "{name}: written once");
        }
    }

    #[test]
    fn keeps_the_files_languages_when_the_library_has_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("book.epub");
        make_epub(&path, EPUB3_OPF, None);
        write(&path, &BookMetadata { languages: vec![], ..wanted() }, None).unwrap();
        assert_eq!(read(&path).unwrap().languages, ["eng"]);
    }

    #[test]
    fn declares_missing_prefixes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("book.epub");
        let bare = r#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0" unique-identifier="id">
  <metadata><dc:identifier xmlns:dc="http://purl.org/dc/elements/1.1/" id="id">x</dc:identifier></metadata>
  <manifest><item id="a" href="a.xhtml" media-type="application/xhtml+xml"/></manifest>
</package>"#;
        make_epub(&path, bare, None);
        write(&path, &wanted(), None).unwrap();
        assert_reads_back(&path, &wanted());
    }

    #[test]
    fn a_broken_file_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("book.epub");
        fs::write(&path, b"not a zip").unwrap();
        assert!(write(&path, &wanted(), None).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"not a zip");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1, "no temp file left behind");
    }
}
