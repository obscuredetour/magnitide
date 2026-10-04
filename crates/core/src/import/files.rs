//! Adding book files (drag and drop, "Add Books…") to a library.
//!
//! Folders are searched for ebooks. Files with the same name in the same
//! folder become one book with several formats ("Dune.epub" + "Dune.mobi").
//! Metadata comes from inside the file where we can read it (EPUB), else
//! from the file name. Each book is stored in its own transaction, so one
//! bad file never affects the others.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{OptionalExtension, params};
use serde::Serialize;

use super::store::{Cover, add_formats, store_book};
use crate::error::Result;
use crate::library::{BookId, Library, NewBook};
use crate::metadata::{self, filename::book_stem};
use crate::{html, names};

/// Extensions Magnitide accepts as books (lower case). `.kepub.epub` is KEPUB.
pub const BOOK_EXTENSIONS: &[&str] = &[
    "epub", "kepub", "mobi", "azw", "azw3", "azw4", "kfx", "pdf", "cbz", "cbr", "cb7", "fb2", "fbz", "djvu", "txt",
    "rtf", "docx", "odt", "html", "htm", "lit", "pdb", "prc", "chm",
];

/// Formats preferred as the source of a book's metadata, best first.
const METADATA_PREFERENCE: &[&str] = &["EPUB", "KEPUB"];

#[derive(Debug, Default, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct AddReport {
    /// Ids of the books added, in order.
    pub added: Vec<BookId>,
    pub files: usize,
    pub covers: usize,
    pub cloned: usize,
    pub copied: usize,
    pub copied_bytes: u64,
    /// Books already in the library that got new formats (same title and
    /// first author): ids, in order. Not in `added`.
    pub formats_added_to: Vec<BookId>,
    /// Books already in the library in every format given (same title and
    /// first author).
    pub duplicates: Vec<String>,
    /// Files that aren't books (by extension), or are already inside the library.
    pub skipped: Vec<String>,
    /// Books that couldn't be added, with the reason. Nothing was left behind for them.
    pub failed: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct AddProgress<'a> {
    pub done: usize,
    pub total: usize,
    pub title: &'a str,
}

/// Add every book found in `paths` (files and folders) to `lib`.
pub fn add_files(lib: &mut Library, paths: &[PathBuf], mut progress: impl FnMut(AddProgress<'_>)) -> Result<AddReport> {
    let mut report = AddReport::default();
    let root = fs::canonicalize(&lib.root)?;

    let mut found = Vec::new();
    for p in paths {
        collect(p, &mut found, &mut report.skipped);
    }

    // Group by folder + name, so the formats of one book stay together.
    let mut groups: BTreeMap<(PathBuf, String), Vec<(String, PathBuf)>> = BTreeMap::new();
    for path in found {
        let canonical = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
        if canonical.starts_with(&root) {
            report.skipped.push(format!("{} is already in this library", path.display()));
            continue;
        }
        let Some(format) = format_of(&path) else {
            report.skipped.push(format!("{} isn't a book format Magnitide recognises", path.display()));
            continue;
        };
        let parent = path.parent().map(Path::to_path_buf).unwrap_or_default();
        let entry = groups.entry((parent, book_stem(&path).to_lowercase())).or_default();
        // One file per format per book.
        if !entry.iter().any(|(f, _)| *f == format) {
            entry.push((format, path));
        }
    }

    let total = groups.len();
    for (done, files) in groups.into_values().enumerate() {
        let (format, path) = metadata_source(&files);
        let meta = metadata::read(path, format);
        let title = meta.title.clone().unwrap_or_else(|| book_stem(path));
        progress(AddProgress { done, total, title: &title });

        let authors: Vec<String> =
            meta.authors.iter().map(|a| a.name.trim().to_owned()).filter(|a| !a.is_empty()).collect();
        // The same book (title and first author) already here: add only the
        // formats it doesn't have yet.
        if let Some((existing, have)) = existing_book(lib, &title, authors.first().map(String::as_str))? {
            let missing: Vec<(String, PathBuf)> = files.iter().filter(|(f, _)| !have.contains(f)).cloned().collect();
            if missing.is_empty() {
                report.duplicates.push(format!("“{title}” is already in the library (book {existing})"));
                continue;
            }
            match add_formats(lib, existing, &missing) {
                Ok(stored) => {
                    report.formats_added_to.push(existing);
                    report.files += stored.files;
                    report.cloned += stored.cloned;
                    report.copied += stored.copied;
                    report.copied_bytes += stored.copied_bytes;
                }
                Err(e) => report.failed.push(e.to_string()),
            }
            continue;
        }

        let new = NewBook {
            title_sort: Some(names::title_sort(&title)),
            title: title.clone(),
            authors,
            series: meta.series.clone(),
            publisher: meta.publisher.clone(),
            published: meta.published,
            description_html: meta.description.as_deref().map(html::sanitize).filter(|d| !d.trim().is_empty()),
            tags: meta.tags.clone(),
            languages: meta.languages.clone(),
            identifiers: meta.identifiers.clone(),
            // The file's own sort names ("Le Guin, Ursula K.") beat our guess.
            author_sorts: meta
                .authors
                .iter()
                .filter_map(|a| Some((a.name.trim().to_owned(), a.sort.as_deref()?.trim().to_owned())))
                .filter(|(_, s)| !s.is_empty())
                .collect(),
            ..Default::default()
        };

        match store_book(lib, &new, &files, meta.cover.as_deref().map(Cover::Bytes), |_, _| Ok(())) {
            Ok(stored) => {
                report.added.push(stored.id);
                report.files += stored.files;
                report.covers += stored.covers;
                report.cloned += stored.cloned;
                report.copied += stored.copied;
                report.copied_bytes += stored.copied_bytes;
                report.warnings.extend(stored.warnings);
            }
            Err(e) => report.failed.push(e.to_string()),
        }
    }
    progress(AddProgress { done: total, total, title: "" });
    Ok(report)
}

/// Files under `path`; folders are searched recursively (hidden entries and
/// symlinked folders skipped, so loops are impossible).
fn collect(path: &Path, out: &mut Vec<PathBuf>, skipped: &mut Vec<String>) {
    let hidden = path.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.'));
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => {
            if hidden {
                return;
            }
            let Ok(entries) = fs::read_dir(path) else {
                skipped.push(format!("{} couldn't be read", path.display()));
                return;
            };
            let mut children: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
            children.sort();
            for child in children {
                collect(&child, out, skipped);
            }
        }
        Ok(m) if m.is_file() || m.file_type().is_symlink() && path.is_file() => {
            if !hidden {
                out.push(path.to_path_buf());
            }
        }
        _ => skipped.push(format!("{} couldn't be read", path.display())),
    }
}

/// Upper-case format for a book file, or None if it isn't one.
pub fn format_of(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_string_lossy().to_lowercase();
    if name.ends_with(".kepub.epub") {
        return Some("KEPUB".into());
    }
    let ext = name.rsplit_once('.')?.1;
    BOOK_EXTENSIONS.contains(&ext).then(|| match ext {
        "htm" => "HTML".to_owned(),
        other => other.to_ascii_uppercase(),
    })
}

fn metadata_source(files: &[(String, PathBuf)]) -> (&str, &Path) {
    let best = METADATA_PREFERENCE.iter().find_map(|p| files.iter().find(|(f, _)| f == p)).unwrap_or(&files[0]);
    (best.0.as_str(), best.1.as_path())
}

/// The earliest book with this title (any case) and first author, and the
/// formats it has.
fn existing_book(lib: &Library, title: &str, author: Option<&str>) -> Result<Option<(BookId, Vec<String>)>> {
    let author = author.unwrap_or("Unknown");
    let id: Option<BookId> = lib
        .conn
        .prepare_cached(
            "SELECT b.id FROM books b
             JOIN book_authors ba ON ba.book_id = b.id AND ba.position = 0
             JOIN authors a ON a.id = ba.author_id
             WHERE b.title = ?1 COLLATE NOCASE AND a.name = ?2
             ORDER BY b.id LIMIT 1",
        )?
        .query_row(params![title, author], |r| r.get(0))
        .optional()?;
    let Some(id) = id else { return Ok(None) };
    let mut stmt = lib.conn.prepare_cached("SELECT format FROM files WHERE book_id = ?")?;
    let formats = stmt.query_map([id], |r| r.get(0))?.collect::<rusqlite::Result<Vec<String>>>()?;
    Ok(Some((id, formats)))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;
    use crate::metadata::epub::tests::{EPUB2_OPF, make_epub};

    fn png() -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        image::RgbImage::new(4, 6).write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    fn setup() -> (tempfile::TempDir, Library, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let lib = Library::create(tmp.path().join("lib"), "Test").unwrap();
        let drop = tmp.path().join("Downloads");
        fs::create_dir(&drop).unwrap();
        (tmp, lib, drop)
    }

    #[test]
    fn adds_a_folder_grouping_formats_and_reading_epub_metadata() {
        let (_tmp, mut lib, drop) = setup();
        // EPUB with a PNG cover, plus the same book as MOBI: one book, two formats.
        let opf = EPUB2_OPF.replace("images/My%20Cover.jpg", "cover.png").replace("image/jpeg", "image/png");
        make_epub(&drop.join("dispossessed.epub"), &opf, Some(("cover.png", &png())));
        fs::write(drop.join("dispossessed.mobi"), "mobi bytes").unwrap();
        fs::write(drop.join("Leviathan Wakes - James S. A. Corey.pdf"), "pdf bytes").unwrap();
        fs::write(drop.join("photo.jpg"), "not a book").unwrap();
        fs::create_dir(drop.join(".hidden")).unwrap();
        fs::write(drop.join(".hidden/secret.epub"), "ignored").unwrap();

        let report = add_files(&mut lib, std::slice::from_ref(&drop), |_| {}).unwrap();
        assert!(report.failed.is_empty(), "{:?}", report.failed);
        assert_eq!(report.added.len(), 2, "{report:?}");
        assert_eq!(report.files, 3);
        assert_eq!(report.covers, 1);
        assert_eq!(report.skipped.len(), 1, "photo.jpg: {:?}", report.skipped);

        let cat = lib.load_catalog().unwrap();
        let d = cat.books.iter().find(|b| b.title == "The Dispossessed").unwrap();
        assert_eq!(d.authors, ["Ursula K. Le Guin"]);
        assert_eq!(d.author_sort.as_deref(), Some("Le Guin, Ursula K."), "the file's own sort name");
        assert_eq!(d.series.as_deref(), Some("Hainish Cycle"));
        assert_eq!(d.tags, ["Classics", "Science Fiction"]);
        assert_eq!(d.comments.as_deref(), Some("<p>An <b>ambiguous</b> utopia.</p>"));
        let mut formats: Vec<&str> = d.formats.iter().map(|f| f.format.as_str()).collect();
        formats.sort();
        assert_eq!(formats, ["EPUB", "MOBI"]);
        let cover = fs::read(d.cover_path(lib.root()).unwrap()).unwrap();
        assert!(cover.starts_with(&[0xFF, 0xD8, 0xFF]), "PNG cover converted to JPEG");

        // The PDF's metadata came from its file name.
        let l = cat.books.iter().find(|b| b.title == "Leviathan Wakes").unwrap();
        assert_eq!(l.authors, ["James S. A. Corey"]);
        assert!(!l.has_cover);
    }

    #[test]
    fn skips_duplicates_and_files_already_in_the_library() {
        let (_tmp, mut lib, drop) = setup();
        let file = drop.join("Dune - Frank Herbert.epub");
        fs::write(&file, "not really an epub").unwrap();
        assert_eq!(add_files(&mut lib, std::slice::from_ref(&file), |_| {}).unwrap().added.len(), 1);

        let again = add_files(&mut lib, &[file], |_| {}).unwrap();
        assert_eq!((again.added.len(), again.duplicates.len()), (0, 1));

        let inside = lib.root().join("Frank Herbert");
        let r = add_files(&mut lib, &[inside], |_| {}).unwrap();
        assert!(r.added.is_empty() && r.skipped.iter().all(|s| s.contains("already in this library")), "{r:?}");
    }

    #[test]
    fn a_new_format_of_a_book_joins_it() {
        let (tmp, mut lib, drop) = setup();
        make_epub(&drop.join("dispossessed.epub"), EPUB2_OPF, None);
        let id = add_files(&mut lib, &[drop.join("dispossessed.epub")], |_| {}).unwrap().added[0];

        // Later, the same book as KEPUB (calibre-style name) and PDF, from elsewhere.
        let later = tmp.path().join("Later");
        fs::create_dir(&later).unwrap();
        make_epub(&later.join("The Dispossessed - Ursula K. Le Guin.kepub.epub"), EPUB2_OPF, None);
        fs::write(later.join("The Dispossessed - Ursula K. Le Guin.pdf"), "pdf bytes").unwrap();
        let r = add_files(&mut lib, std::slice::from_ref(&later), |_| {}).unwrap();
        assert_eq!((r.added.len(), r.formats_added_to.as_slice(), r.files), (0, [id].as_slice(), 2), "{r:?}");

        let cat = lib.load_catalog().unwrap();
        assert_eq!(cat.books.len(), 1);
        let b = cat.get(id).unwrap();
        let mut formats: Vec<&str> = b.formats.iter().map(|f| f.format.as_str()).collect();
        formats.sort();
        assert_eq!(formats, ["EPUB", "KEPUB", "PDF"]);
        for f in &b.formats {
            let path = b.format_path(lib.root(), f);
            assert!(path.is_file() && path.parent() == Some(b.dir(lib.root()).as_path()), "{}", path.display());
        }

        // Every format already there: a duplicate, nothing added.
        let again = add_files(&mut lib, &[later], |_| {}).unwrap();
        assert_eq!((again.formats_added_to.len(), again.duplicates.len(), again.files), (0, 1, 0));
    }

    #[test]
    fn reads_mobi_and_azw3_metadata_and_covers() {
        let (_tmp, mut lib, drop) = setup();
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        fs::copy(fixtures.join("test.mobi"), drop.join("whatever.mobi")).unwrap();
        let r = add_files(&mut lib, &[drop.join("whatever.mobi")], |_| {}).unwrap();
        let b = lib.book(r.added[0]).unwrap();
        assert_eq!(b.title, "Test Book: A Story");
        assert_eq!(b.authors, ["Ursula K. Le Guin", "Second Author"]);
        assert_eq!(b.identifiers.get("isbn").map(String::as_str), Some("9780061054884"));
        assert!(b.cover_file.is_some(), "cover taken from the MOBI");
        // The same book as AZW3 joins it.
        fs::copy(fixtures.join("test.azw3"), drop.join("other name.azw3")).unwrap();
        let r = add_files(&mut lib, &[drop.join("other name.azw3")], |_| {}).unwrap();
        assert_eq!(r.formats_added_to, [b.id]);
    }

    #[test]
    fn a_broken_epub_falls_back_to_its_file_name() {
        let (_tmp, mut lib, drop) = setup();
        let file = drop.join("Broken Book - Some Author.epub");
        fs::write(&file, "PK garbage").unwrap();
        let r = add_files(&mut lib, &[file], |_| {}).unwrap();
        let b = lib.book(r.added[0]).unwrap();
        assert_eq!((b.title.as_str(), b.authors[0].as_str()), ("Broken Book", "Some Author"));
    }

    #[test]
    fn recognises_book_formats() {
        assert_eq!(format_of(Path::new("a.kepub.epub")).as_deref(), Some("KEPUB"));
        assert_eq!(format_of(Path::new("a.AZW3")).as_deref(), Some("AZW3"));
        assert_eq!(format_of(Path::new("a.htm")).as_deref(), Some("HTML"));
        assert_eq!(format_of(Path::new("a.jpg")), None);
        assert_eq!(format_of(Path::new("README")), None);
    }
}
