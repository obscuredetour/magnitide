//! Copying books to a device and removing them from it.
//!
//! Sending picks the format the device reads best, copies it into a folder
//! per author (`documents/` on a Kindle), and, for EPUBs, writes the
//! library's metadata, cover and book id into the copy (never into the
//! library's file), so the device shows what the library shows and the copy
//! is always recognized. Removing deletes the book file and KOReader's
//! `.sdr` folder next to it, nothing else.

use std::fs;
use std::path::{Component, Path, PathBuf};

use magnitide_catalog::{Book, Format};
use serde::Serialize;

use super::{Device, DeviceKind};
use crate::convert::{Converter, TempDir};
use crate::metadata::{Author, BookMetadata, epub_write};
use crate::paths;

/// What a device reads, best first. Kindles don't take EPUB over USB.
pub fn readable(kind: DeviceKind) -> &'static [&'static str] {
    match kind {
        DeviceKind::Kobo => &["KEPUB", "EPUB", "PDF", "MOBI", "CBZ", "CBR", "TXT", "HTML", "RTF"],
        DeviceKind::Kindle => &["AZW3", "MOBI", "AZW", "PDF", "TXT"],
        DeviceKind::Generic => &["EPUB", "PDF", "MOBI", "AZW3", "FB2", "CBZ", "CBR", "TXT", "KEPUB"],
    }
}

/// The format to send to this kind of device: `preferred` if the book has
/// it and the device reads it, else the best the book has.
pub fn pick_format<'b>(
    kind: DeviceKind,
    book: &'b Book,
    library_root: &Path,
    preferred: Option<&str>,
) -> Option<&'b Format> {
    let present = |p: &str| book.formats.iter().find(|f| f.format == p && book.format_path(library_root, f).is_file());
    preferred
        .filter(|p| readable(kind).contains(p))
        .and_then(present)
        .or_else(|| readable(kind).iter().find_map(|p| present(p)))
}

#[derive(Debug, Default, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct SendReport {
    /// Books copied.
    pub sent: usize,
    pub bytes: u64,
    /// Of those, converted on the way (the library keeps its formats).
    pub converted: usize,
    /// Books with no format this device reads ("Title (EPUB only)").
    pub unsupported: Vec<String>,
    /// Some unsupported books could be converted if calibre were installed.
    pub needs_calibre: bool,
    /// Books that couldn't be sent, with the reason; nothing left behind.
    pub failed: Vec<String>,
    /// Sent, but with something worth knowing (metadata couldn't be written).
    pub warnings: Vec<String>,
}

/// What to put on the device for one book.
struct Plan<'b> {
    book: &'b Book,
    /// The library file to start from.
    source: &'b Format,
    /// The format sent: `source`'s, or one it's converted to.
    format: String,
}

/// Copy `books` to the device, as `preferred` where they have it (converting
/// to it where they don't and `converter` can). Books with no format the
/// device reads are converted to the best one it does, where possible.
/// `progress(done, total, title)`.
pub fn send(
    device: &Device,
    books: &[&Book],
    library_root: &Path,
    free_bytes: Option<u64>,
    preferred: Option<&str>,
    converter: &Converter,
    mut progress: impl FnMut(usize, usize, &str),
) -> SendReport {
    let mut report = SendReport::default();
    let can_calibre = Converter::with_calibre(Some("ebook-convert".into()));
    let mut plans: Vec<Plan<'_>> = Vec::new();
    for b in books {
        let present: Vec<String> =
            b.formats.iter().filter(|f| b.format_path(library_root, f).is_file()).map(|f| f.format.clone()).collect();
        let format_named = |name: &str| b.formats.iter().find(|f| f.format == name);
        let convert_to = |to: &str, conv: &Converter| conv.source_for(&present, to).and_then(format_named);
        let wanted = preferred.filter(|p| readable(device.kind).contains(p) && !present.iter().any(|h| h == p));
        let plan = match wanted.and_then(|w| convert_to(w, converter).map(|src| (src, w))) {
            // The preferred format, made from another.
            Some((source, to)) => Some(Plan { book: b, source, format: to.to_owned() }),
            None => match pick_format(device.kind, b, library_root, preferred) {
                Some(f) => Some(Plan { book: b, source: f, format: f.format.clone() }),
                // Nothing it reads: the best format it reads that we can make.
                None => readable(device.kind)
                    .iter()
                    .find_map(|to| convert_to(to, converter).map(|src| (src, *to)))
                    .map(|(source, to)| Plan { book: b, source, format: to.to_owned() }),
            },
        };
        match plan {
            Some(p) => plans.push(p),
            None => {
                let have: Vec<&str> = b.formats.iter().map(|f| f.format.as_str()).collect();
                report.unsupported.push(format!("“{}” ({})", b.title, have.join(", ")));
                report.needs_calibre |= converter.calibre().is_none()
                    && readable(device.kind).iter().any(|to| can_calibre.source_for(&present, to).is_some());
            }
        }
    }

    // Converted books are estimated at their source's size.
    let needed: u64 = plans.iter().map(|p| p.source.size).sum();
    if let Some(free) = free_bytes
        && needed > free.saturating_sub(5 << 20)
    {
        report.failed.push(format!(
            "Not enough space on {}: {} MB needed, {} MB free",
            device.name,
            needed.div_ceil(1 << 20),
            free >> 20
        ));
        return report;
    }

    let base = match device.kind {
        DeviceKind::Kindle => device.root.join("documents"),
        DeviceKind::Kobo | DeviceKind::Generic => device.root.clone(),
    };
    for (done, plan) in plans.iter().enumerate() {
        progress(done, plans.len(), &plan.book.title);
        let src = plan.book.format_path(library_root, plan.source);
        let result = if plan.format == plan.source.format {
            send_one(device.kind, plan.book, &plan.format, &src, library_root, &base)
        } else {
            TempDir::new().and_then(|work| {
                let converted = work.0.join(format!("converted.{}", plan.format.to_ascii_lowercase()));
                converter.convert(&src, &plan.source.format, &plan.format, &converted)?;
                send_one(device.kind, plan.book, &plan.format, &converted, library_root, &base)
            })
        };
        match result {
            Ok((bytes, warning)) => {
                report.sent += 1;
                report.converted += usize::from(plan.format != plan.source.format);
                report.bytes += bytes;
                report.warnings.extend(warning);
            }
            Err(e) => report.failed.push(format!("“{}”: {e}", plan.book.title)),
        }
    }
    progress(plans.len(), plans.len(), "");
    report
}

/// Copy one book file (`src`, in `format`) to the device.
fn send_one(
    kind: DeviceKind,
    book: &Book,
    format: &str,
    src: &Path,
    library_root: &Path,
    base: &Path,
) -> Result<(u64, Option<String>), String> {
    let author = book.authors.first().map_or("Unknown", String::as_str);
    let dir = base.join(paths::sanitize(author, 64));
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    // Kobo only treats a KEPUB as one with this double extension.
    let ext = match (kind, format) {
        (DeviceKind::Kobo, "KEPUB") => "kepub.epub".to_owned(),
        (_, f) => f.to_ascii_lowercase(),
    };
    let stem = paths::sanitize(&format!("{} - {author}", book.title), 120);
    let dest = (1..)
        .map(|n| dir.join(if n == 1 { format!("{stem}.{ext}") } else { format!("{stem} ({n}).{ext}") }))
        .find(|p| !p.exists())
        .expect("unbounded");

    // Copy beside, finish, then rename: an unplugged device never holds half a book.
    let part = dir.join(format!(".{}.part", dest.file_name().unwrap_or_default().to_string_lossy()));
    let result = (|| {
        fs::copy(src, &part).map_err(|e| e.to_string())?;
        let mut warning = None;
        if matches!(format, "EPUB" | "KEPUB")
            && let Err(e) = epub_write::write(&part, &metadata_for(book, library_root), book.sort.as_deref())
        {
            // A plain copy still reads fine; say so rather than fail.
            fs::copy(src, &part).map_err(|e| e.to_string())?;
            warning = Some(format!("“{}”: sent as is; its metadata couldn't be written ({e})", book.title));
        }
        let bytes = fs::metadata(&part).map_err(|e| e.to_string())?.len();
        fs::rename(&part, &dest).map_err(|e| e.to_string())?;
        Ok((bytes, warning))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&part);
        let _ = fs::remove_dir(&dir); // only if empty
    }
    result
}

/// The library's view of a book, for writing into a copy.
fn metadata_for(book: &Book, library_root: &Path) -> BookMetadata {
    // calibre-style "Sort One & Sort Two", one per author when they line up.
    let sorts: Vec<&str> = book.author_sort.as_deref().map(|s| s.split(" & ").collect()).unwrap_or_default();
    let authors = book
        .authors
        .iter()
        .enumerate()
        .map(|(i, name)| Author {
            name: name.clone(),
            sort: (sorts.len() == book.authors.len()).then(|| sorts[i].trim().to_owned()),
        })
        .collect();
    // Only JPEG covers can be embedded (some calibre "cover.jpg" files aren't).
    let cover = book.cover_path(library_root).and_then(|p| fs::read(p).ok()).filter(|b| b.starts_with(&[0xFF, 0xD8]));
    BookMetadata {
        title: Some(book.title.clone()),
        authors,
        series: book.series.clone().map(|s| (s, book.series_index)),
        publisher: book.publisher.clone(),
        published: book.pubdate.map(|d| d.date_naive()),
        languages: book.languages.clone(),
        identifiers: book.identifiers.clone(),
        description: book.comments.clone(),
        tags: book.tags.clone(),
        cover,
        uuids: book.uuid.clone().into_iter().collect(),
    }
}

#[derive(Debug, Default, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct RemoveReport {
    pub removed: usize,
    pub failed: Vec<String>,
}

/// Delete files (paths relative to the device root, as scanned) and their
/// KOReader `.sdr` folders, then any folders left empty.
pub fn remove(device: &Device, rel_paths: &[String]) -> RemoveReport {
    let mut report = RemoveReport::default();
    for rel in rel_paths {
        match remove_one(&device.root, rel) {
            Ok(()) => report.removed += 1,
            Err(e) => report.failed.push(format!("{rel}: {e}")),
        }
    }
    report
}

fn remove_one(root: &Path, rel: &str) -> Result<(), String> {
    // Paths come from our own scan, but are checked anyway: plain, relative, inside.
    let rel_path = PathBuf::from(rel);
    if rel.is_empty() || !rel_path.components().all(|c| matches!(c, Component::Normal(_))) {
        return Err("not a path on the device".into());
    }
    let path = root.join(&rel_path);
    if !path.is_file() {
        return Err("not on the device any more".into());
    }
    fs::remove_file(&path).map_err(|e| e.to_string())?;
    // KOReader keeps progress and highlights in "<name minus extension>.sdr".
    let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
    if let Some((stem, _)) = name.rsplit_once('.') {
        let sdr = path.with_file_name(format!("{stem}.sdr"));
        if sdr.is_dir() {
            let _ = fs::remove_dir_all(sdr);
        }
    }
    // Folders left empty, up to (not including) the root and Kindle's documents.
    let mut dir = path.parent();
    while let Some(d) = dir.filter(|d| *d != root && !d.ends_with("documents") && d.starts_with(root)) {
        if fs::remove_dir(d).is_err() {
            break;
        }
        dir = d.parent();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Library;
    use crate::devices::{MatchedBy, detect, match_books, scan};
    use crate::import::add_files;
    use crate::metadata::epub;
    use crate::metadata::epub::tests::{EPUB2_OPF, make_epub};

    fn setup(tmp: &Path) -> Library {
        let src = tmp.join("src");
        fs::create_dir_all(&src).unwrap();
        make_epub(&src.join("The Dispossessed - Ursula K. Le Guin.epub"), EPUB2_OPF, None);
        fs::write(src.join("Dune - Frank Herbert.pdf"), "%PDF").unwrap();
        fs::write(src.join("Dune - Frank Herbert.azw3"), "azw3").unwrap();
        fs::write(src.join("Notes - Someone.docx"), "docx").unwrap();
        let mut lib = Library::create(tmp.join("lib"), "Test").unwrap();
        add_files(&mut lib, &[src], |_| {}).unwrap();
        lib
    }

    #[test]
    fn sends_the_best_format_with_library_metadata_then_removes() {
        let tmp = tempfile::tempdir().unwrap();
        let mut lib = setup(tmp.path());
        // A library-only edit: the device copy should carry it.
        let id = lib.load_catalog().unwrap().books.iter().find(|b| b.title == "The Dispossessed").unwrap().id;
        let edit = crate::BookEdit { title: "The Dispossessed (edited)".into(), ..lib.book_edit(id).unwrap() };
        lib.update_book(id, &edit).unwrap();
        let cat = lib.load_catalog().unwrap();
        let books: Vec<&Book> = cat.books.iter().collect();

        let kobo_root = tmp.path().join("KOBOeReader");
        fs::create_dir_all(kobo_root.join(".kobo")).unwrap();
        let kobo = detect(&kobo_root).unwrap();
        let report = send(&kobo, &books, lib.root(), Some(1 << 30), None, &Converter::default(), |_, _, _| {});
        assert_eq!((report.sent, report.unsupported.len()), (2, 1), "{report:?}");
        assert!(report.unsupported[0].starts_with("“Notes” (DOCX)"));

        let sent = kobo_root.join("Ursula K. Le Guin/The Dispossessed (edited) - Ursula K. Le Guin.epub");
        let copy = epub::read(&sent).unwrap();
        assert_eq!(copy.title.as_deref(), Some("The Dispossessed (edited)"));
        assert!(kobo_root.join("Frank Herbert/Dune - Frank Herbert.pdf").is_file(), "Kobo prefers PDF to AZW3");
        // The library's file is untouched.
        let b = cat.get(id).unwrap();
        assert_eq!(
            epub::read(&b.format_path(lib.root(), &b.formats[0])).unwrap().title.as_deref(),
            Some("The Dispossessed")
        );

        // Recognized by its embedded id.
        let s = scan(&kobo);
        let m = match_books(&s, &cat, lib.root());
        let hit = s.files.iter().zip(&m).find(|(f, _)| f.format == "EPUB").unwrap();
        assert_eq!(*hit.1, Some((id, MatchedBy::Uuid)));

        // Remove it, with a KOReader sidecar.
        let sdr = sent.with_file_name("The Dispossessed (edited) - Ursula K. Le Guin.sdr");
        fs::create_dir_all(&sdr).unwrap();
        fs::write(sdr.join("metadata.epub.lua"), "progress").unwrap();
        let r = remove(&kobo, &[hit.0.path.clone(), "../escape.pdf".into()]);
        assert_eq!(r.removed, 1);
        assert_eq!(r.failed.len(), 1);
        assert!(!sent.exists() && !sdr.exists() && !kobo_root.join("Ursula K. Le Guin").exists());
        assert!(kobo_root.join(".kobo").is_dir(), "the device is otherwise untouched");
    }

    #[test]
    fn kindles_get_kindle_formats_in_documents() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = setup(tmp.path());
        let cat = lib.load_catalog().unwrap();
        let root = tmp.path().join("Kindle");
        fs::create_dir_all(root.join("system")).unwrap();
        fs::create_dir_all(root.join("documents")).unwrap();
        let kindle = detect(&root).unwrap();

        let books: Vec<&Book> = cat.books.iter().collect();
        let report = send(&kindle, &books, lib.root(), None, None, &Converter::default(), |_, _, _| {});
        assert_eq!(report.sent, 1, "{report:?}");
        assert_eq!(report.unsupported.len(), 1, "DOCX can't be made into anything a Kindle reads");
        // The EPUB is converted to AZW3 on the way; this fixture has no pages, so that fails, cleanly.
        assert_eq!(report.failed.len(), 1, "{report:?}");
        assert!(root.join("documents/Frank Herbert/Dune - Frank Herbert.azw3").is_file());

        // Sending again doesn't overwrite.
        send(&kindle, &books, lib.root(), None, None, &Converter::default(), |_, _, _| {});
        assert!(root.join("documents/Frank Herbert/Dune - Frank Herbert (2).azw3").is_file());
    }

    #[test]
    fn sends_the_preferred_format_where_there_is_one() {
        let tmp = tempfile::tempdir().unwrap();
        let mut lib = setup(tmp.path());
        let kepub = tmp.path().join("The Dispossessed - Ursula K. Le Guin.kepub.epub");
        make_epub(&kepub, EPUB2_OPF, None);
        add_files(&mut lib, &[kepub], |_| {}).unwrap();
        let cat = lib.load_catalog().unwrap();
        let root = tmp.path().join("KOBOeReader");
        fs::create_dir_all(root.join(".kobo")).unwrap();
        let kobo = detect(&root).unwrap();
        let books: Vec<&Book> = cat.books.iter().collect();

        let r = send(&kobo, &books, lib.root(), None, Some("EPUB"), &Converter::default(), |_, _, _| {});
        assert_eq!(r.sent, 2, "{r:?}");
        assert!(root.join("Ursula K. Le Guin/The Dispossessed - Ursula K. Le Guin.epub").is_file(), "EPUB as asked");
        assert!(root.join("Frank Herbert/Dune - Frank Herbert.pdf").is_file(), "no EPUB: its best other format");
        // A format the device can't read is ignored.
        let dune: Vec<&Book> = cat.books.iter().filter(|b| b.title == "Dune").collect();
        assert_eq!(pick_format(kobo.kind, dune[0], lib.root(), Some("AZW3")).unwrap().format, "PDF");
    }

    #[test]
    fn converts_on_the_way_when_needed() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = setup(tmp.path());
        let cat = lib.load_catalog().unwrap();
        let dispossessed: Vec<&Book> = cat.books.iter().filter(|b| b.title == "The Dispossessed").collect();
        let root = tmp.path().join("KOBOeReader");
        fs::create_dir_all(root.join(".kobo")).unwrap();
        let kobo = detect(&root).unwrap();

        // Preferred KEPUB, the book has only EPUB: converted natively for the device.
        let r = send(&kobo, &dispossessed, lib.root(), None, Some("KEPUB"), &Converter::default(), |_, _, _| {});
        assert_eq!((r.sent, r.converted), (1, 1), "{r:?}");
        let sent = root.join("Ursula K. Le Guin/The Dispossessed - Ursula K. Le Guin.kepub.epub");
        assert!(epub::read(&sent).is_some());
        assert_eq!(cat.get(dispossessed[0].id).unwrap().formats.len(), 1, "the library is unchanged");

        // A Kindle and an EPUB-only book: converted to AZW3 natively, no calibre needed.
        let kindle_root = tmp.path().join("Kindle");
        fs::create_dir_all(kindle_root.join("system")).unwrap();
        fs::create_dir_all(kindle_root.join("documents")).unwrap();
        let sample = tmp.path().join("Sample Book - Ann Author.epub");
        crate::convert::azw3::tests::sample_epub(&sample);
        let mut lib = lib;
        let id = add_files(&mut lib, &[sample], |_| {}).unwrap().added[0];
        let cat = lib.load_catalog().unwrap();
        let sample_book: Vec<&Book> = cat.books.iter().filter(|b| b.id == id).collect();
        let r = send(
            &detect(&kindle_root).unwrap(),
            &sample_book,
            lib.root(),
            None,
            None,
            &Converter::default(),
            |_, _, _| {},
        );
        assert_eq!((r.sent, r.converted, r.unsupported.len()), (1, 1, 0), "{r:?}");
        assert!(kindle_root.join("documents/Ann Author/Sample Book - Ann Author.azw3").is_file());
    }

    #[test]
    fn refuses_when_the_device_is_full() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = setup(tmp.path());
        let cat = lib.load_catalog().unwrap();
        let root = tmp.path().join("KOBOeReader");
        fs::create_dir_all(root.join(".kobo")).unwrap();
        let books: Vec<&Book> = cat.books.iter().collect();
        let r =
            send(&detect(&root).unwrap(), &books, lib.root(), Some(1024), None, &Converter::default(), |_, _, _| {});
        assert_eq!(r.sent, 0);
        assert!(r.failed[0].starts_with("Not enough space"), "{r:?}");
    }
}
