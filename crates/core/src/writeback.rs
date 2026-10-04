//! "Write metadata to files": copy a book's metadata and cover from the
//! library into its files, so e-readers and other apps see the same thing.
//! EPUB (and KEPUB) only for now; other formats are reported as skipped.
//!
//! Files are only rewritten when they differ from the library, because a
//! rewrite changes the file's fingerprint (which some reading-progress sync,
//! like KOReader's, keys on). `files.metadata_written_at` remembers when a
//! file was last known to match, so unchanged books aren't re-read.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::edit::dir_for;
use crate::error::{Error, Result};
use crate::library::{BookId, Library, now};
use crate::metadata::{Author, BookMetadata, epub, epub_write};

/// Formats whose metadata Magnitide can write.
const WRITABLE: &[&str] = &["EPUB", "KEPUB"];

#[derive(Debug, Default, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct WriteReport {
    /// Books with at least one writable file.
    pub books: usize,
    /// Files rewritten.
    pub written: usize,
    /// Files that already matched.
    pub up_to_date: usize,
    /// Files in formats Magnitide can't write yet.
    pub skipped: usize,
    /// Files that couldn't be written, with the reason; left as they were.
    pub failed: Vec<String>,
}

/// What writing every book would do.
#[derive(Debug, Default, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct WritebackPreview {
    /// Books with a file that differs from the library.
    pub books: usize,
    /// Files that would be rewritten.
    pub files: usize,
}

/// Whether a book's files hold its current metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum FileMetadataStatus {
    Current,
    Outdated,
    /// No file Magnitide can write to (PDF only, say).
    Unsupported,
}

struct Target {
    file_id: i64,
    format: String,
    path: PathBuf,
    /// Known to match: written (or checked) after the book last changed.
    fresh: bool,
}

/// What the library says, as it would be written.
struct Wanted {
    meta: BookMetadata,
    title_sort: String,
}

impl Library {
    /// Whether this book's files match the library, reading them if needed.
    pub fn file_metadata_status(&self, id: BookId) -> Result<FileMetadataStatus> {
        let (targets, _) = self.targets(id)?;
        if targets.is_empty() {
            return Ok(FileMetadataStatus::Unsupported);
        }
        let wanted = self.wanted(id)?;
        for t in &targets {
            if !self.is_current(t, &wanted, false)? {
                return Ok(FileMetadataStatus::Outdated);
            }
        }
        Ok(FileMetadataStatus::Current)
    }

    /// Write this book's metadata into its files that differ from the library.
    pub fn write_metadata_to_files(&mut self, id: BookId) -> Result<WriteReport> {
        let mut report = WriteReport::default();
        // Asked for explicitly: check the files themselves, not the record.
        self.write_book(id, true, &mut report)?;
        Ok(report)
    }

    /// How many books and files [`Self::write_all_metadata`] would rewrite.
    pub fn writeback_preview(&self) -> Result<WritebackPreview> {
        let mut preview = WritebackPreview::default();
        for id in self.writable_books()? {
            let (targets, _) = self.targets(id)?;
            let wanted = self.wanted(id)?;
            let mut stale = 0;
            for t in &targets {
                stale += usize::from(!self.is_current(t, &wanted, false)?);
            }
            preview.books += usize::from(stale > 0);
            preview.files += stale;
        }
        Ok(preview)
    }

    /// Write every book's metadata into its files that differ from the library.
    /// A book that fails is reported and left as it was; the rest go ahead.
    pub fn write_all_metadata(&mut self, mut progress: impl FnMut(usize, usize)) -> Result<WriteReport> {
        let ids = self.writable_books()?;
        let mut report = WriteReport::default();
        for (done, id) in ids.iter().enumerate() {
            progress(done, ids.len());
            if let Err(e) = self.write_book(*id, false, &mut report) {
                report.failed.push(format!("Book {id}: {e}"));
            }
        }
        progress(ids.len(), ids.len());
        Ok(report)
    }

    fn write_book(&mut self, id: BookId, check_files: bool, report: &mut WriteReport) -> Result<()> {
        let (targets, unsupported) = self.targets(id)?;
        report.skipped += unsupported;
        if targets.is_empty() {
            return Ok(());
        }
        report.books += 1;
        let wanted = self.wanted(id)?;
        for t in &targets {
            if self.is_current(t, &wanted, check_files)? {
                report.up_to_date += 1;
                continue;
            }
            let title = wanted.meta.title.as_deref().unwrap_or_default();
            match epub_write::write(&t.path, &wanted.meta, Some(&wanted.title_sort)) {
                Ok(()) => {
                    let size = fs::metadata(&t.path)?.len();
                    self.conn.execute(
                        "UPDATE files SET size = ?, metadata_written_at = ? WHERE id = ?",
                        rusqlite::params![size as i64, now(), t.file_id],
                    )?;
                    report.written += 1;
                }
                Err(e) => report.failed.push(format!("“{title}” ({}): {e}", t.format)),
            }
        }
        Ok(())
    }

    /// Does the file match? Trusts `metadata_written_at` unless `check_file`;
    /// otherwise reads the file, and records a match so it isn't read again.
    fn is_current(&self, t: &Target, wanted: &Wanted, check_file: bool) -> Result<bool> {
        if t.fresh && !check_file {
            return Ok(true);
        }
        let matches = epub::read(&t.path).is_some_and(|file| same_metadata(&file, &wanted.meta));
        if matches && !t.fresh {
            self.conn.execute("UPDATE files SET metadata_written_at = ? WHERE id = ?", rusqlite::params![
                now(),
                t.file_id
            ])?;
        }
        Ok(matches)
    }

    /// Books with at least one writable file.
    fn writable_books(&self) -> Result<Vec<BookId>> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT book_id FROM files WHERE format IN ('EPUB', 'KEPUB') ORDER BY book_id",
        )?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// The book's writable files, and how many others it has.
    fn targets(&self, id: BookId) -> Result<(Vec<Target>, usize)> {
        let mut stmt = self.conn.prepare_cached(
            // Timestamps are whole seconds: equal means "can't tell", so not fresh.
            "SELECT f.id, f.format, f.filename, b.path, coalesce(f.metadata_written_at > b.modified_at, 0)
             FROM files f JOIN books b ON b.id = f.book_id
             WHERE f.book_id = ? ORDER BY f.id",
        )?;
        let rows = stmt.query_map([id], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get(4)?))
        })?;
        let (mut targets, mut other) = (Vec::new(), 0);
        for row in rows {
            let (file_id, format, filename, dir, fresh) = row?;
            if WRITABLE.contains(&format.as_str()) {
                targets.push(Target { file_id, format, path: dir_for(&self.root, &dir).join(filename), fresh });
            } else {
                other += 1;
            }
        }
        Ok((targets, other))
    }

    fn wanted(&self, id: BookId) -> Result<Wanted> {
        let b = self.book(id)?;
        let mut stmt = self.conn.prepare_cached(
            "SELECT a.name, a.sort_name FROM book_authors ba JOIN authors a ON a.id = ba.author_id
             WHERE ba.book_id = ? ORDER BY ba.position",
        )?;
        let authors = stmt
            .query_map([id], |r| Ok(Author { name: r.get(0)?, sort: Some(r.get(1)?) }))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let cover = match &b.cover_file {
            Some(name) => Some(read_cover(&dir_for(&self.root, &b.path).join(name))?),
            None => None,
        };
        Ok(Wanted {
            title_sort: b.title_sort,
            meta: BookMetadata {
                title: Some(b.title),
                authors,
                series: b.series,
                publisher: b.publisher,
                published: b.published,
                languages: b.languages,
                identifiers: b.identifiers,
                description: b.description_html,
                tags: b.tags,
                cover,
            },
        })
    }
}

fn read_cover(path: &Path) -> Result<Vec<u8>> {
    fs::read(path).map_err(|e| Error::WriteFile { file: path.display().to_string(), reason: format!("cover: {e}") })
}

/// Does the file already say what the library says? Compared the way the
/// reader sees the file. Identifiers only need to be present: the file may
/// hold others Magnitide keeps (its package id).
fn same_metadata(file: &BookMetadata, want: &BookMetadata) -> bool {
    let squash = |s: &Option<String>| s.as_deref().map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "));
    let folded = |v: &[String]| {
        let mut v: Vec<String> = v.iter().map(|s| s.to_lowercase()).collect();
        v.sort();
        v
    };
    let series_eq = match (&file.series, &want.series) {
        (Some((a, i)), Some((b, j))) => a == b && (i - j).abs() < 1e-9,
        (None, None) => true,
        _ => false,
    };
    squash(&file.title) == squash(&want.title)
        && file.authors == want.authors
        && series_eq
        && file.publisher == want.publisher
        && file.published == want.published
        && (want.languages.is_empty() || file.languages == want.languages)
        && want.identifiers.iter().all(|(k, v)| file.identifiers.get(k) == Some(v))
        && folded(&file.tags) == folded(&want.tags)
        && squash(&file.description) == squash(&want.description)
        && (want.cover.is_none() || file.cover == want.cover)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BookEdit;
    use crate::import::add_files;
    use crate::metadata::epub::tests::{EPUB2_OPF, make_epub};

    fn library_with_epub() -> (tempfile::TempDir, Library, BookId) {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("The Dispossessed - Ursula K. Le Guin.epub");
        make_epub(&src, EPUB2_OPF, None);
        let mut lib = Library::create(tmp.path().join("lib"), "Test").unwrap();
        let id = add_files(&mut lib, &[src], |_| {}).unwrap().added[0];
        (tmp, lib, id)
    }

    fn epub_path(lib: &Library, id: BookId) -> PathBuf {
        let (targets, _) = lib.targets(id).unwrap();
        targets[0].path.clone()
    }

    #[test]
    fn writes_edits_into_the_file_and_tracks_status() {
        let (_tmp, mut lib, id) = library_with_epub();
        // Added from the file, so it matches apart from what import normalized.
        lib.write_metadata_to_files(id).unwrap();
        assert_eq!(lib.file_metadata_status(id).unwrap(), FileMetadataStatus::Current);

        let mut edit = lib.book_edit(id).unwrap();
        edit.title = "The Dispossessed: An Ambiguous Utopia".into();
        edit.tags = vec!["Anarchism".into()];
        lib.update_book(id, &edit).unwrap();
        assert_eq!(lib.file_metadata_status(id).unwrap(), FileMetadataStatus::Outdated);
        assert_eq!(lib.writeback_preview().unwrap(), WritebackPreview { books: 1, files: 1 });

        let report = lib.write_metadata_to_files(id).unwrap();
        assert_eq!((report.books, report.written, report.up_to_date), (1, 1, 0), "{:?}", report.failed);
        // The rename after the title change moved the file; we wrote the new one.
        let path = epub_path(&lib, id);
        let file = epub::read(&path).unwrap();
        assert_eq!(file.title.as_deref(), Some("The Dispossessed: An Ambiguous Utopia"));
        assert_eq!(file.tags, ["Anarchism"]);
        let size: i64 = lib.conn.query_row("SELECT size FROM files WHERE book_id = ?", [id], |r| r.get(0)).unwrap();
        assert_eq!(size as u64, fs::metadata(&path).unwrap().len());

        assert_eq!(lib.file_metadata_status(id).unwrap(), FileMetadataStatus::Current);
        assert_eq!(lib.writeback_preview().unwrap(), WritebackPreview::default());
        let again = lib.write_all_metadata(|_, _| {}).unwrap();
        assert_eq!((again.written, again.up_to_date), (0, 1));
    }

    #[test]
    fn writes_the_cover_and_skips_other_formats() {
        let (tmp, mut lib, id) = library_with_epub();
        let png = {
            let mut buf = std::io::Cursor::new(Vec::new());
            image::RgbImage::new(4, 6).write_to(&mut buf, image::ImageFormat::Png).unwrap();
            buf.into_inner()
        };
        lib.set_cover(id, &png).unwrap();
        let pdf = tmp.path().join("Other - Someone.pdf");
        fs::write(&pdf, "%PDF").unwrap();
        let pdf_id = add_files(&mut lib, &[pdf], |_| {}).unwrap().added[0];

        // The PDF-only book isn't visited at all.
        let report = lib.write_all_metadata(|_, _| {}).unwrap();
        assert_eq!((report.books, report.written, report.skipped), (1, 1, 0), "{:?}", report.failed);
        let cover = fs::read(dir_for(lib.root(), &lib.book(id).unwrap().path).join("cover.jpg")).unwrap();
        assert_eq!(epub::read(&epub_path(&lib, id)).unwrap().cover, Some(cover));
        assert_eq!(lib.file_metadata_status(pdf_id).unwrap(), FileMetadataStatus::Unsupported);
        assert_eq!(lib.write_metadata_to_files(pdf_id).unwrap().skipped, 1);
    }

    #[test]
    fn a_damaged_file_is_reported_not_fatal() {
        let (_tmp, mut lib, id) = library_with_epub();
        fs::write(epub_path(&lib, id), "damaged").unwrap();
        let edit = BookEdit { title: "Changed".into(), ..lib.book_edit(id).unwrap() };
        lib.update_book(id, &edit).unwrap();
        let report = lib.write_all_metadata(|_, _| {}).unwrap();
        assert_eq!(report.written, 0);
        assert_eq!(report.failed.len(), 1, "{report:?}");
        assert!(report.failed[0].starts_with("“Changed” (EPUB): "), "{}", report.failed[0]);
        assert_eq!(fs::read(epub_path(&lib, id)).unwrap(), b"damaged");
    }
}
