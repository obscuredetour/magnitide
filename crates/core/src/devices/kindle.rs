// Derived from calibre's src/calibre/devices/kindle/driver.py (calibre 9.18).
// Copyright: 2009, John Schember <john at nachtimwald.com>, and Kovid Goyal. License: GPLv3.
// Ported to Rust for Magnitide, 2026.

//! Kindle cover thumbnails.
//!
//! A Kindle's library shows each book's cover from
//! `system/thumbnails/thumbnail_<ASIN>_<type>_portrait.jpg`, keyed on the
//! ASIN and content type inside the book file. For a book that didn't come
//! from Amazon it may never make one, and once online it can delete or
//! replace the ones it has (it looks the made-up ASIN up at Amazon), leaving
//! a blank cover. So, as calibre does: a thumbnail is written when a book is
//! sent, missing ones are put back for library books on the device when it's
//! connected, and a book's goes when the book is removed.
//!
//! The ASIN and type are read from the file on the device, whoever made it
//! (Magnitide's AZW3 writer and calibre use the book's uuid as the ASIN and
//! `EBOK` as the type). Personal documents (`PDOC`) are left alone: the
//! Kindle doesn't look those up at Amazon, so it keeps the cover it makes
//! from the book. calibre writes one whatever the type.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use magnitide_catalog::{BookId, Catalog};
use serde::Serialize;

use super::{DeviceKind, MatchedBy, Scan};
use crate::images::thumbnail_jpeg;
use crate::metadata::mobi::book_key;

/// The longest side of a thumbnail: calibre's `THUMBNAIL_HEIGHT` for Kindles
/// (the Oasis uses 500; earlier models scale it).
pub const THUMBNAIL_SIZE: u32 = 500;

/// JPEG quality: calibre's `THUMBNAIL_COMPRESSION_QUALITY`.
const QUALITY: u8 = 75;

/// Where calibre keeps a copy of each thumbnail it writes, to put back ones
/// the Kindle deletes. Not written here, but cleared with the book, or
/// calibre would bring the thumbnail of a removed book back.
const CALIBRE_BACKUPS: &str = "amazon-cover-bug";

fn thumbnails_dir(root: &Path) -> PathBuf {
    root.join("system").join("thumbnails")
}

/// The name of the thumbnail a Kindle shows for `book_file` (a MOBI, AZW or
/// AZW3 on it), from the ASIN and content type inside it.
pub fn thumbnail_name(book_file: &Path) -> Option<String> {
    let (asin, kind) = book_key(book_file)?;
    Some(format!("thumbnail_{asin}_{kind}_portrait.jpg"))
}

/// The thumbnail name, for a book that gets one written (not a `PDOC`).
fn wanted_name(book_file: &Path) -> Option<String> {
    let (asin, kind) = book_key(book_file)?;
    (kind != "PDOC").then(|| format!("thumbnail_{asin}_{kind}_portrait.jpg"))
}

/// Write the thumbnail for `book_file`, just sent to the Kindle at `root`,
/// from `cover` (an image file). The name written, or None when there's
/// nothing to do: no cover, no `system/thumbnails` folder, a file with no
/// ASIN or type (PDF, TXT…), or a personal document.
pub fn write_thumbnail(root: &Path, book_file: &Path, cover: Option<&Path>) -> Result<Option<String>, String> {
    let dir = thumbnails_dir(root);
    let (Some(cover), true) = (cover, dir.is_dir()) else { return Ok(None) };
    let Some(name) = wanted_name(book_file) else { return Ok(None) };
    write(&dir.join(&name), cover)?;
    Ok(Some(name))
}

fn write(dest: &Path, cover: &Path) -> Result<(), String> {
    let bytes = fs::read(cover).map_err(|e| format!("reading the cover: {e}"))?;
    let jpeg = thumbnail_jpeg(&bytes, THUMBNAIL_SIZE, QUALITY).map_err(|e| format!("the cover: {e}"))?;
    fs::write(dest, jpeg).map_err(|e| e.to_string())
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ThumbnailReport {
    /// Thumbnails written.
    pub restored: usize,
    /// Books that should have one but couldn't, with the reason.
    pub failed: Vec<String>,
}

/// Put back the cover thumbnails missing from a Kindle, for the books on it
/// matched to the library (`matches`, as from [`super::match_books`]) that
/// have a cover there. Thumbnails already present are left as they are, and
/// unmatched files aren't opened. Nothing happens on other devices, or a
/// Kindle with no `system/thumbnails` folder.
pub fn restore_thumbnails(
    scan: &Scan,
    matches: &[Option<(BookId, MatchedBy)>],
    catalog: &Catalog,
    library_root: &Path,
) -> ThumbnailReport {
    let mut report = ThumbnailReport::default();
    let root = &scan.device.root;
    let dir = thumbnails_dir(root);
    if scan.device.kind != DeviceKind::Kindle || !dir.is_dir() {
        return report;
    }
    // Listed once, rather than a check per book (slow on some USB storage).
    let Ok(entries) = fs::read_dir(&dir) else { return report };
    let mut present: HashSet<String> =
        entries.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    for (file, m) in scan.files.iter().zip(matches) {
        let Some((id, _)) = m else { continue };
        if !matches!(file.format.as_str(), "AZW3" | "MOBI" | "AZW") {
            continue;
        }
        let Some(book) = catalog.get(*id) else { continue };
        let Some(cover) = book.cover_path(library_root) else { continue };
        let Some(name) = wanted_name(&root.join(&file.path)) else { continue };
        if present.contains(&name) {
            continue;
        }
        match write(&dir.join(&name), &cover) {
            Ok(()) => {
                report.restored += 1;
                present.insert(name);
            }
            Err(e) => report.failed.push(format!("“{}”: {e}", book.title)),
        }
    }
    report
}

/// The thumbnail of `book_file` on the Kindle at `root`, and calibre's backup
/// of it. Read before the book goes: the name comes from inside it.
pub(crate) fn thumbnails_of(root: &Path, book_file: &Path) -> Vec<PathBuf> {
    match thumbnail_name(book_file) {
        Some(name) => vec![thumbnails_dir(root).join(&name), root.join(CALIBRE_BACKUPS).join(name)],
        None => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use magnitide_catalog::Book;

    use super::*;
    use crate::Library;
    use crate::convert::{Converter, azw3};
    use crate::devices::transfer::{Origin, remove, send};
    use crate::devices::{detect, match_books, scan};
    use crate::import::add_files;

    const SAMPLE_UUID: &str = "0f0f0f0f-1111-4222-8333-444444444444";

    fn kindle(root: &Path, thumbnails: bool) -> crate::devices::Device {
        fs::create_dir_all(root.join("documents")).unwrap();
        fs::create_dir_all(root.join(if thumbnails { "system/thumbnails" } else { "system" })).unwrap();
        detect(root).unwrap()
    }

    /// A library holding the AZW3 test book, converted by the native writer
    /// (with a cover, which comes from the EPUB).
    fn library(tmp: &Path) -> Library {
        let src = tmp.join("src");
        fs::create_dir_all(&src).unwrap();
        let epub = tmp.join("sample.epub");
        azw3::tests::sample_epub(&epub);
        azw3::convert(&epub, &src.join("Sample Book - Ann Author.azw3")).unwrap();
        let mut lib = Library::create(tmp.join("lib"), "Test").unwrap();
        let report = add_files(&mut lib, &[src], |_| {}).unwrap();
        assert_eq!(report.added.len(), 1, "{report:?}");
        lib
    }

    fn send_all(device: &crate::devices::Device, lib: &Library) -> crate::devices::transfer::SendReport {
        let cat = lib.load_catalog().unwrap();
        let books: Vec<&Book> = cat.books.iter().collect();
        let origin = Origin { root: lib.root(), keep_in: None };
        send(device, &books, origin, None, None, &Converter::default(), |_| {})
    }

    fn thumbnails(root: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(thumbnails_dir(root))
            .map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
            .unwrap_or_default();
        names.sort();
        names
    }

    #[test]
    fn sending_an_azw3_writes_its_thumbnail() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = library(tmp.path());
        let root = tmp.path().join("Kindle");
        let device = kindle(&root, true);

        let report = send_all(&device, &lib);
        assert_eq!((report.sent, report.warnings.len()), (1, 0), "{report:?}");
        let sent = root.join("documents/Ann Author/Sample Book - Ann Author.azw3");
        // Named from the ASIN and type the native writer put in the file.
        assert_eq!(book_key(&sent), Some((SAMPLE_UUID.to_owned(), "EBOK".to_owned())));
        let name = format!("thumbnail_{SAMPLE_UUID}_EBOK_portrait.jpg");
        assert_eq!(thumbnails(&root), std::slice::from_ref(&name));
        let jpeg = fs::read(thumbnails_dir(&root).join(&name)).unwrap();
        assert!(jpeg.starts_with(&[0xFF, 0xD8]));
        // The 60×90 cover isn't scaled up.
        let img = image::load_from_memory(&jpeg).unwrap();
        assert_eq!((img.width(), img.height()), (60, 90));
    }

    #[test]
    fn big_covers_are_scaled_to_fit() {
        let mut png = Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(1200, 1800, image::Rgba([0, 0, 0, 0]))
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let jpeg = thumbnail_jpeg(png.get_ref(), THUMBNAIL_SIZE, QUALITY).unwrap();
        let img = image::load_from_memory(&jpeg).unwrap().to_rgb8();
        assert_eq!(img.dimensions(), (333, 500));
        assert!(img.get_pixel(10, 10).0.iter().all(|c| *c > 250), "transparency becomes white");
    }

    #[test]
    fn skips_books_that_get_no_thumbnail() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = library(tmp.path());
        let cat = lib.load_catalog().unwrap();
        let book = &cat.books[0];
        let cover = book.cover_path(lib.root()).expect("the sample has a cover");
        let azw3 = book.format_path(lib.root(), &book.formats[0]);

        // No thumbnails folder: older Kindles, or not set up yet. None is made.
        let bare = tmp.path().join("Bare");
        kindle(&bare, false);
        assert_eq!(write_thumbnail(&bare, &azw3, Some(&cover)), Ok(None));
        assert!(!thumbnails_dir(&bare).exists());

        let root = tmp.path().join("Kindle");
        kindle(&root, true);
        // No cover.
        assert_eq!(write_thumbnail(&root, &azw3, None), Ok(None));
        // No ASIN to name it by.
        let pdf = tmp.path().join("book.pdf");
        fs::write(&pdf, "%PDF").unwrap();
        assert_eq!(write_thumbnail(&root, &pdf, Some(&cover)), Ok(None));
        // A personal document.
        let pdoc = tmp.path().join("pdoc.azw3");
        fs::write(&pdoc, with_content_type(&fs::read(&azw3).unwrap(), b"PDOC")).unwrap();
        assert_eq!(book_key(&pdoc).map(|k| k.1).as_deref(), Some("PDOC"));
        assert_eq!(write_thumbnail(&root, &pdoc, Some(&cover)), Ok(None));
        assert!(thumbnails(&root).is_empty());
        // And the book itself does get one.
        assert!(write_thumbnail(&root, &azw3, Some(&cover)).unwrap().is_some());
    }

    fn replace_all(bytes: &mut [u8], from: &[u8], to: &[u8]) {
        let mut at = 0;
        while let Some(i) = bytes[at..].windows(from.len()).position(|w| w == from) {
            bytes[at + i..at + i + to.len()].copy_from_slice(to);
            at += i + to.len();
        }
    }

    /// `azw3` with its EXTH 501 (content type, four bytes) changed.
    fn with_content_type(azw3: &[u8], kind: &[u8; 4]) -> Vec<u8> {
        let mut out = azw3.to_vec();
        let at = out.windows(12).position(|w| w[..8] == [0, 0, 1, 0xF5, 0, 0, 0, 12]).expect("EXTH 501");
        out[at + 8..at + 12].copy_from_slice(kind);
        out
    }

    #[test]
    fn restores_only_missing_thumbnails_of_library_books() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = library(tmp.path());
        let root = tmp.path().join("Kindle");
        let device = kindle(&root, true);
        send_all(&device, &lib);
        let name = format!("thumbnail_{SAMPLE_UUID}_EBOK_portrait.jpg");
        let thumb = thumbnails_dir(&root).join(&name);
        // A book the library doesn't have (another ASIN and title), with no thumbnail either.
        let mut other = fs::read(root.join("documents/Ann Author/Sample Book - Ann Author.azw3")).unwrap();
        replace_all(&mut other, SAMPLE_UUID.as_bytes(), b"0f0f0f0f-9999-4222-8333-444444444444");
        replace_all(&mut other, b"Sample Book", b"Other Title");
        let other_file = root.join("documents/Other Title - Someone.azw3");
        fs::write(&other_file, &other).unwrap();
        assert!(thumbnail_name(&other_file).is_some_and(|n| n != name));

        let restore = || {
            let cat = lib.load_catalog().unwrap();
            let s = scan(&device);
            let m = match_books(&s, &cat, lib.root());
            restore_thumbnails(&s, &m, &cat, lib.root())
        };

        // Present: left as it is (the Kindle may have put a better one there).
        fs::write(&thumb, b"the kindle's own").unwrap();
        assert_eq!(restore(), ThumbnailReport::default());
        assert_eq!(fs::read(&thumb).unwrap(), b"the kindle's own");

        // Deleted (by the Kindle, once online): put back, and only that one.
        fs::remove_file(&thumb).unwrap();
        assert_eq!(restore(), ThumbnailReport { restored: 1, failed: vec![] });
        assert!(fs::read(&thumb).unwrap().starts_with(&[0xFF, 0xD8]));
        assert_eq!(thumbnails(&root), [name]);
        assert_eq!(restore().restored, 0);

        // Not a Kindle with thumbnails: nothing.
        fs::remove_dir_all(thumbnails_dir(&root)).unwrap();
        assert_eq!(restore(), ThumbnailReport::default());
        assert!(!thumbnails_dir(&root).exists());
    }

    #[test]
    fn removing_a_book_removes_its_thumbnail() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = library(tmp.path());
        let root = tmp.path().join("Kindle");
        let device = kindle(&root, true);
        send_all(&device, &lib);
        let name = format!("thumbnail_{SAMPLE_UUID}_EBOK_portrait.jpg");
        // calibre's backup of it, and another book's thumbnail.
        fs::create_dir_all(root.join(CALIBRE_BACKUPS)).unwrap();
        fs::write(root.join(CALIBRE_BACKUPS).join(&name), b"jpeg").unwrap();
        fs::write(thumbnails_dir(&root).join("thumbnail_B000000000_EBOK_portrait.jpg"), b"jpeg").unwrap();

        let r = remove(&device, &["documents/Ann Author/Sample Book - Ann Author.azw3".into()]);
        assert_eq!((r.removed, r.failed.len()), (1, 0), "{r:?}");
        assert_eq!(thumbnails(&root), ["thumbnail_B000000000_EBOK_portrait.jpg"]);
        assert!(!root.join(CALIBRE_BACKUPS).join(&name).exists());
        assert!(!root.join("documents/Ann Author").exists());
    }
}
