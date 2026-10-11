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

use magnitide_catalog::{CustomValue, Datatype};
use rusqlite::{OptionalExtension, params};
use serde::Serialize;

use super::store::{Cover, add_formats, store_book};
use crate::custom::{CustomField, FieldValue};
use crate::error::Result;
use crate::hooks::{ImportHooks, ImportOutcome, MetadataPlugins, NoHooks};
use crate::library::{BookId, Library, NewBook};
use crate::metadata::comic::ComicDetails;
use crate::metadata::{self, filename::book_stem};
use crate::{html, names};

/// Extensions Magnitide accepts as books (lower case). `.kepub.epub` is KEPUB.
pub const BOOK_EXTENSIONS: &[&str] = &[
    "epub", "kepub", "mobi", "azw", "azw3", "azw4", "kfx", "pdf", "cbz", "cbr", "cb7", "fb2", "fbz", "djvu", "txt",
    "rtf", "docx", "odt", "html", "htm", "lit", "pdb", "prc", "chm",
];

/// Book formats that are as often just documents: a folder like Documents or
/// Downloads is full of them (READMEs, saved pages, letters).
pub const DOCUMENT_FORMATS: &[&str] = &["TXT", "HTML", "RTF", "DOCX", "ODT"];

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
    /// Every file stored: the book it went to and its format (upper case).
    #[serde(default)]
    pub formats: Vec<(BookId, String)>,
}

/// What adding some files and folders would take, found before anything is
/// copied: enough to ask first about a big or mixed drop.
#[derive(Debug, Default, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct DropScan {
    /// Files by format (upper case), most first; documents included.
    pub formats: Vec<(String, usize)>,
    /// Files in a book format, documents included.
    pub files: usize,
    /// Files in one of [`DOCUMENT_FORMATS`].
    pub documents: usize,
    /// calibre libraries found (their folders). Their files aren't counted:
    /// they're better imported, with what calibre knows about them.
    pub calibre_libraries: Vec<String>,
    /// A folder was among the paths (a single file is never a surprise).
    pub has_folder: bool,
}

/// Which of the files found adding takes. The default takes them all.
#[derive(Debug, Default, Clone)]
pub struct AddFilter {
    /// Leave out files in [`DOCUMENT_FORMATS`].
    pub skip_documents: bool,
    /// Folders left out (calibre libraries imported instead).
    pub exclude: Vec<PathBuf>,
}

#[derive(Debug, Clone, Copy)]
pub struct AddProgress<'a> {
    pub done: usize,
    pub total: usize,
    pub title: &'a str,
}

/// Add every book found in `paths` (files and folders) to `lib`.
pub fn add_files(lib: &mut Library, paths: &[PathBuf], progress: impl FnMut(AddProgress<'_>)) -> Result<AddReport> {
    add_files_with(lib, paths, &NoHooks, &NoHooks, progress)
}

/// What adding `paths` would find: book files by format, documents, and
/// calibre libraries. `extra` are lower-case extensions plugins handle.
pub fn scan(paths: &[PathBuf], extra: &[String]) -> DropScan {
    let mut found = Vec::new();
    let mut calibre = Vec::new();
    for p in paths {
        collect(p, &[], Some(&mut calibre), &mut found, &mut Vec::new());
    }
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for path in &found {
        if let Some(format) = format_of_with(path, extra) {
            *counts.entry(format).or_default() += 1;
        }
    }
    let documents = counts.iter().filter(|(f, _)| DOCUMENT_FORMATS.contains(&f.as_str())).map(|(_, n)| n).sum();
    let mut formats: Vec<(String, usize)> = counts.into_iter().collect();
    formats.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    DropScan {
        files: formats.iter().map(|(_, n)| n).sum(),
        formats,
        documents,
        calibre_libraries: calibre.iter().map(|p| p.display().to_string()).collect(),
        has_folder: paths.iter().any(|p| fs::symlink_metadata(p).is_ok_and(|m| m.is_dir())),
    }
}

/// [`add_files`] with plugins: `hooks` run on each file before its metadata
/// is read (and may hand back another file to add instead), and `plugins`
/// may read metadata of formats Magnitide can't.
pub fn add_files_with(
    lib: &mut Library,
    paths: &[PathBuf],
    hooks: &dyn ImportHooks,
    plugins: &dyn MetadataPlugins,
    progress: impl FnMut(AddProgress<'_>),
) -> Result<AddReport> {
    add_files_filtered(lib, paths, &AddFilter::default(), hooks, plugins, progress)
}

/// [`add_files_with`], taking only what `filter` lets through.
pub fn add_files_filtered(
    lib: &mut Library,
    paths: &[PathBuf],
    filter: &AddFilter,
    hooks: &dyn ImportHooks,
    plugins: &dyn MetadataPlugins,
    mut progress: impl FnMut(AddProgress<'_>),
) -> Result<AddReport> {
    let mut report = AddReport::default();
    let root = fs::canonicalize(&lib.root)?;
    let extra: Vec<String> = hooks.file_types().into_iter().map(|e| e.to_lowercase()).collect();

    let mut found = Vec::new();
    for p in paths {
        collect(p, &filter.exclude, None, &mut found, &mut report.skipped);
    }

    // Group by folder + name, so the formats of one book stay together.
    let mut groups: BTreeMap<(PathBuf, String), Vec<(String, PathBuf)>> = BTreeMap::new();
    for path in found {
        let canonical = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
        if canonical.starts_with(&root) {
            report.skipped.push(format!("{} is already in this library", path.display()));
            continue;
        }
        let Some(format) = format_of_with(&path, &extra) else {
            report.skipped.push(format!("{} isn't a book format Magnitide recognises", path.display()));
            continue;
        };
        // Left out on purpose, so not reported as skipped.
        if filter.skip_documents && DOCUMENT_FORMATS.contains(&format.as_str()) {
            continue;
        }
        let parent = path.parent().map(Path::to_path_buf).unwrap_or_default();
        let entry = groups.entry((parent, book_stem(&path).to_lowercase())).or_default();
        // One file per format per book.
        if !entry.iter().any(|(f, _)| *f == format) {
            entry.push((format, path));
        }
    }

    let total = groups.len();
    let mut comic_fields = None;
    let mut artists_as_authors = None;
    for (done, files) in groups.into_values().enumerate() {
        // Plugins first (calibre's on_import), on each file; a plugin may
        // hand back a rewritten file, possibly with another extension.
        let mut temporary = Vec::new();
        let files: Vec<(String, PathBuf)> = files
            .into_iter()
            .map(|(format, path)| match hooks.on_import(&path, &format) {
                ImportOutcome::Unchanged => (format, path),
                ImportOutcome::Replaced { path: new, temporary: tmp } => {
                    if tmp {
                        temporary.push(new.clone());
                    }
                    let format = format_of_with(&new, &extra).unwrap_or(format);
                    (format, new)
                }
                ImportOutcome::Failed(why) => {
                    report.warnings.push(format!("{}: {why}; added as it is", path.display()));
                    (format, path)
                }
            })
            .collect();
        let (format, path) = metadata_source(&files);
        let meta = metadata::read_with(path, format, plugins);
        let title = meta.title.clone().unwrap_or_else(|| book_stem(path));
        progress(AddProgress { done, total, title: &title });

        let mut authors: Vec<String> =
            meta.authors.iter().map(|a| a.name.trim().to_owned()).filter(|a| !a.is_empty()).collect();
        // A comic's artists are its authors too, after the writers (unless the library says not).
        if let Some(comic) = meta.comic.as_deref() {
            let artists_too = match artists_as_authors {
                Some(on) => on,
                None => *artists_as_authors.insert(lib.comic_artists_as_authors()?),
            };
            authors = crate::comics::comic_authors(&authors, comic, artists_too);
        }
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
                    report.formats.extend(missing.iter().map(|(f, _)| (existing, f.clone())));
                }
                Err(e) => report.failed.push(e.to_string()),
            }
            remove_temporary(&temporary);
            continue;
        }

        let new = NewBook {
            title_sort: Some(names::title_sort(&title)),
            title: title.clone(),
            authors,
            series: meta.series.clone(),
            series_end: meta.comic.as_ref().and_then(|c| c.issue_end),
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

        // A comic's own fields (issue, artists, …), made the first time one is added.
        let custom = match meta.comic.as_deref() {
            Some(comic) => match comic_custom(lib, comic, &mut comic_fields) {
                Ok(values) => values,
                Err(e) => {
                    report.failed.push(format!("{}: {e}", path.display()));
                    remove_temporary(&temporary);
                    continue;
                }
            },
            None => Vec::new(),
        };
        let cover = meta.cover.as_deref().map(Cover::Bytes);
        match store_book(lib, &new, &files, cover, |tx, id| crate::custom::write_custom(tx, id, &custom)) {
            Ok(stored) => {
                report.added.push(stored.id);
                report.files += stored.files;
                report.covers += stored.covers;
                report.cloned += stored.cloned;
                report.copied += stored.copied;
                report.copied_bytes += stored.copied_bytes;
                report.warnings.extend(stored.warnings);
                report.formats.extend(files.iter().map(|(f, _)| (stored.id, f.clone())));
            }
            Err(e) => report.failed.push(e.to_string()),
        }
        remove_temporary(&temporary);
    }
    progress(AddProgress { done: total, total, title: "" });
    Ok(report)
}

/// Files under `path`; folders are searched recursively (hidden entries and
/// symlinked folders skipped, so loops are impossible). Folders in `exclude`
/// are left out; with `calibre`, so are calibre libraries, listed there.
fn collect(
    path: &Path,
    exclude: &[PathBuf],
    mut calibre: Option<&mut Vec<PathBuf>>,
    out: &mut Vec<PathBuf>,
    skipped: &mut Vec<String>,
) {
    let hidden = path.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.'));
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => {
            if hidden || exclude.iter().any(|e| e == path) {
                return;
            }
            if let Some(found) = calibre.as_deref_mut()
                && path.join("metadata.db").is_file()
            {
                found.push(path.to_path_buf());
                return;
            }
            let Ok(entries) = fs::read_dir(path) else {
                skipped.push(format!("{} couldn't be read", path.display()));
                return;
            };
            let mut children: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
            children.sort();
            for child in children {
                collect(&child, exclude, calibre.as_deref_mut(), out, skipped);
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

/// A comic's values for the library's comic fields, ready to write. The fields
/// are looked up (and made, if need be) once per add, in `fields`.
fn comic_custom(
    lib: &mut Library,
    comic: &ComicDetails,
    fields: &mut Option<Vec<CustomField>>,
) -> Result<Vec<(i64, Datatype, Option<CustomValue>)>> {
    if fields.is_none() {
        *fields = Some(lib.ensure_comic_fields()?);
    }
    let fields = fields.as_deref().unwrap_or_default();
    let values = crate::comics::comic_values(comic)
        .into_iter()
        .filter(|(key, value)| {
            let Some(field) = fields.iter().find(|f| &f.key == key) else { return false };
            match value {
                // A Format whose choice was taken out of the field is left unset.
                Some(FieldValue::Text(t)) if !field.choices.is_empty() => {
                    field.choices.iter().any(|c| c.eq_ignore_ascii_case(t))
                }
                Some(_) => true,
                None => false,
            }
        })
        .collect();
    lib.clean_custom(&values)
}

/// Files a plugin made while adding a book, no longer needed.
fn remove_temporary(paths: &[PathBuf]) {
    for p in paths {
        let _ = fs::remove_file(p);
    }
}

/// Upper-case format for a book file, or None if it isn't one.
pub fn format_of(path: &Path) -> Option<String> {
    format_of_with(path, &[])
}

/// [`format_of`], also accepting `extra` lower-case extensions (ones plugins handle).
pub fn format_of_with(path: &Path, extra: &[String]) -> Option<String> {
    let name = path.file_name()?.to_string_lossy().to_lowercase();
    if name.ends_with(".kepub.epub") {
        return Some("KEPUB".into());
    }
    let ext = name.rsplit_once('.')?.1;
    (BOOK_EXTENSIONS.contains(&ext) || extra.iter().any(|e| e == ext)).then(|| match ext {
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

    /// A Documents-like folder: books, documents, a photo, hidden files, and a
    /// calibre library.
    fn mixed_folder(drop: &Path) {
        fs::write(drop.join("Dune - Frank Herbert.epub"), "epub").unwrap();
        fs::write(drop.join("Dune - Frank Herbert.pdf"), "pdf").unwrap();
        fs::create_dir(drop.join("Series")).unwrap();
        fs::write(drop.join("Series/Leviathan Wakes - James S. A. Corey.epub"), "epub").unwrap();
        fs::write(drop.join("README.txt"), "read me").unwrap();
        fs::write(drop.join("letter.docx"), "docx").unwrap();
        fs::write(drop.join("photo.jpg"), "not a book").unwrap();
        fs::write(drop.join(".secret.epub"), "hidden").unwrap();
        let calibre = drop.join("Calibre Library");
        fs::create_dir_all(calibre.join("Ann/Book (1)")).unwrap();
        fs::write(calibre.join("metadata.db"), "").unwrap();
        fs::write(calibre.join("Ann/Book (1)/Book - Ann.epub"), "epub").unwrap();
    }

    #[test]
    fn scans_a_drop_counting_formats_documents_and_calibre_libraries() {
        let (_tmp, _lib, drop) = setup();
        mixed_folder(&drop);
        let s = scan(std::slice::from_ref(&drop), &[]);
        let formats: Vec<(&str, usize)> = s.formats.iter().map(|(f, n)| (f.as_str(), *n)).collect();
        assert_eq!(formats, [("EPUB", 2), ("DOCX", 1), ("PDF", 1), ("TXT", 1)], "most first, then by name");
        assert_eq!((s.files, s.documents), (5, 2));
        assert_eq!(s.calibre_libraries, [drop.join("Calibre Library").display().to_string()]);
        assert!(s.has_folder);

        // A single file isn't a folder; a calibre library dropped by itself is found.
        let one = scan(&[drop.join("README.txt")], &[]);
        assert!(!one.has_folder);
        assert_eq!((one.files, one.documents), (1, 1));
        assert_eq!(scan(&[drop.join("Calibre Library")], &[]).calibre_libraries.len(), 1);
    }

    #[test]
    fn adds_only_what_the_filter_lets_through() {
        let (_tmp, mut lib, drop) = setup();
        mixed_folder(&drop);
        let filter = AddFilter { skip_documents: true, exclude: vec![drop.join("Calibre Library")] };
        let report =
            add_files_filtered(&mut lib, std::slice::from_ref(&drop), &filter, &NoHooks, &NoHooks, |_| {}).unwrap();
        // Dune (EPUB + PDF) and Leviathan Wakes; no documents, nothing from calibre's folder.
        assert_eq!((report.added.len(), report.files), (2, 3));
        assert_eq!(report.skipped.len(), 1, "only the photo: {:?}", report.skipped);

        // Unfiltered, the same folder takes everything, calibre's files included.
        let (_tmp2, mut all, drop2) = setup();
        mixed_folder(&drop2);
        let report = add_files(&mut all, &[drop2], |_| {}).unwrap();
        assert_eq!(report.added.len(), 5);
    }

    #[test]
    fn a_comic_gets_its_own_fields_made_and_filled() {
        use crate::custom::{FieldKind, FieldValue, NewField};
        use crate::metadata::comic::tests::{COMIC_INFO, make_cbz};

        let (_tmp, mut lib, drop) = setup();
        // A field of the user's own called "volume", of another kind: kept, and not written to.
        lib.add_custom_field(&NewField {
            name: "Volume".into(),
            key: "volume".into(),
            kind: Some(FieldKind::Text),
            multiple: false,
            choices: vec![],
        })
        .unwrap();
        make_cbz(&drop.join("saga1.cbz"), &[("1.jpg", &png()), ("ComicInfo.xml", COMIC_INFO.as_bytes())], None);
        let tpb = "<ComicInfo><Title>Saga, Volume One</Title><Series>Saga TPB</Series><Number>1-6</Number>\
                   <Format>TPB</Format></ComicInfo>";
        make_cbz(&drop.join("saga-tpb.cbz"), &[("1.jpg", &png()), ("ComicInfo.xml", tpb.as_bytes())], None);

        let report = add_files(&mut lib, &[drop], |_| {}).unwrap();
        assert_eq!((report.added.len(), report.covers), (2, 2), "{report:?}");

        let fields = lib.custom_fields().unwrap();
        let comic_keys: Vec<&str> =
            fields.iter().filter(|f| f.group.as_deref() == Some("comic")).map(|f| f.key.as_str()).collect();
        assert!(comic_keys.contains(&"comic") && comic_keys.contains(&"artists"), "{comic_keys:?}");
        assert!(!comic_keys.contains(&"volume"), "the user's own field isn't taken over");

        let books: Vec<_> = report.added.iter().map(|id| (*id, lib.book(*id).unwrap())).collect();
        let (issue, saga) = books.iter().find(|(_, b)| b.title == "Saga #1").unwrap();
        assert_eq!(saga.authors, ["Brian K. Vaughan", "Fiona Staples"], "the artist is an author too");
        let values = lib.custom_edit_values(*issue).unwrap();
        assert_eq!(values["comic"], Some(FieldValue::Bool(true)));
        assert_eq!(values["issue"], Some(FieldValue::Text("1".into())));
        assert_eq!(values["artists"], Some(FieldValue::List(vec!["Fiona Staples".into()])));
        assert_eq!(values["characters"], Some(FieldValue::List(vec!["Alana".into(), "Marko".into(), "Hazel".into()])));
        assert_eq!(values["volume"], None, "not written into the user's text field");
        assert_eq!(values["right_to_left"], None);

        let (collected_id, collected) = books.iter().find(|(_, b)| b.title == "Saga, Volume One").unwrap();
        assert_eq!(lib.custom_edit_values(*collected_id).unwrap()["format"], Some(FieldValue::Text("TPB".into())));
        assert_eq!(values["format"], None, "Saga #1's file names no format");
        assert_eq!(
            (collected.series.clone(), collected.series_end),
            (Some(("Saga TPB".into(), 1.0)), Some(6.0)),
            "an omnibus of issues 1–6"
        );
    }

    #[test]
    fn a_format_the_field_no_longer_offers_is_left_unset() {
        use crate::custom::FieldValue;
        use crate::metadata::comic::tests::make_cbz;

        let (_tmp, mut lib, drop) = setup();
        lib.ensure_comic_fields().unwrap();
        let format = lib.custom_fields().unwrap().into_iter().find(|f| f.key == "format").unwrap();
        lib.update_custom_field(format.id, "Format", Some(&["Issue".to_owned()])).unwrap();
        let xml = "<ComicInfo><Series>Saga</Series><Number>1-6</Number><Format>TPB</Format></ComicInfo>";
        make_cbz(&drop.join("tpb.cbz"), &[("ComicInfo.xml", xml.as_bytes())], None);
        let report = add_files(&mut lib, &[drop], |_| {}).unwrap();
        assert_eq!(report.added.len(), 1, "{report:?}");
        let values = lib.custom_edit_values(report.added[0]).unwrap();
        assert_eq!((values["format"].clone(), values["issue"].clone()), (None, Some(FieldValue::Text("1-6".into()))));
    }

    #[test]
    fn a_library_can_keep_comic_artists_off_the_author_line() {
        use crate::metadata::comic::tests::{COMIC_INFO, make_cbz};

        let (_tmp, mut lib, drop) = setup();
        assert!(lib.comic_artists_as_authors().unwrap(), "on by default");
        lib.set_comic_artists_as_authors(false).unwrap();
        assert!(!lib.comic_artists_as_authors().unwrap());
        make_cbz(&drop.join("saga1.cbz"), &[("ComicInfo.xml", COMIC_INFO.as_bytes())], None);
        let report = add_files(&mut lib, &[drop], |_| {}).unwrap();
        assert_eq!(lib.book(report.added[0]).unwrap().authors, ["Brian K. Vaughan"]);
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

    struct RecordingHooks {
        calls: std::sync::Mutex<Vec<String>>,
        replace_with: Option<PathBuf>,
        fail: bool,
    }

    impl ImportHooks for RecordingHooks {
        fn file_types(&self) -> Vec<String> {
            vec!["xyz".into()]
        }

        fn on_import(&self, path: &Path, format: &str) -> ImportOutcome {
            self.calls.lock().unwrap().push(format!("{format} {}", path.file_name().unwrap().to_string_lossy()));
            if self.fail {
                return ImportOutcome::Failed("boom".into());
            }
            match &self.replace_with {
                Some(p) => ImportOutcome::Replaced { path: p.clone(), temporary: true },
                None => ImportOutcome::Unchanged,
            }
        }
    }

    struct TxtReader;

    impl crate::hooks::MetadataPlugins for TxtReader {
        fn reads(&self, format: &str) -> Option<crate::hooks::Precedence> {
            (format == "TXT" || format == "XYZ").then_some(crate::hooks::Precedence::Fallback)
        }

        fn read(&self, path: &Path, _format: &str) -> std::result::Result<crate::metadata::BookMetadata, String> {
            let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
            Ok(crate::metadata::BookMetadata {
                title: Some(text.lines().next().unwrap_or("").to_owned()),
                authors: vec![crate::metadata::Author { name: "Reader Plugin".into(), sort: None }],
                ..Default::default()
            })
        }
    }

    #[test]
    fn hooks_rewrite_files_and_read_metadata() {
        let (tmp, mut lib, drop) = setup();
        // A plugin that accepts .xyz files and replaces what's added with a temporary file.
        let rewritten = tmp.path().join("rewritten.xyz");
        fs::write(
            &rewritten,
            "From the Plugin
body",
        )
        .unwrap();
        fs::write(
            drop.join("original.xyz"),
            "Original
body",
        )
        .unwrap();
        let hooks = RecordingHooks { calls: Default::default(), replace_with: Some(rewritten.clone()), fail: false };
        let r = add_files_with(&mut lib, &[drop.join("original.xyz")], &hooks, &TxtReader, |_| {}).unwrap();
        assert_eq!(r.added.len(), 1, "{r:?}");
        assert_eq!(hooks.calls.lock().unwrap().as_slice(), ["XYZ original.xyz"]);
        let b = lib.book(r.added[0]).unwrap();
        assert_eq!(
            (b.title.as_str(), b.authors[0].as_str()),
            ("From the Plugin", "Reader Plugin"),
            "read through the plugin"
        );
        assert!(!rewritten.exists(), "the temporary file is removed once stored");
        assert_eq!(r.formats, vec![(b.id, "XYZ".to_owned())]);

        // A failing hook: the file is added as it is, with a warning.
        fs::write(
            drop.join("plain.txt"),
            "Plain Title
body",
        )
        .unwrap();
        let hooks = RecordingHooks { calls: Default::default(), replace_with: None, fail: true };
        let r = add_files_with(&mut lib, &[drop.join("plain.txt")], &hooks, &TxtReader, |_| {}).unwrap();
        assert_eq!(r.added.len(), 1);
        assert!(r.warnings.iter().any(|w| w.contains("boom")), "{:?}", r.warnings);
        assert_eq!(lib.book(r.added[0]).unwrap().title, "Plain Title");

        // Without a reader plugin, a TXT's title comes from its name.
        fs::write(drop.join("Named Book - Some One.txt"), "x").unwrap();
        let r = add_files(&mut lib, &[drop.join("Named Book - Some One.txt")], |_| {}).unwrap();
        assert_eq!(lib.book(r.added[0]).unwrap().title, "Named Book");
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
