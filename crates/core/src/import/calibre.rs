//! Import a calibre library into a Magnitide library.
//!
//! The calibre library is only ever read. Book files and covers are cloned
//! (copy-on-write: no extra disk space on APFS, Btrfs, XFS) or copied where
//! cloning isn't supported. Each book is imported in its own transaction,
//! together with its files: if anything fails, that book's row is rolled
//! back and its half-written folder removed.
//!
//! Imports can be repeated: books already imported from the same calibre
//! library are skipped, so a second run picks up books added since.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use calibre_compat::{CustomColumn, CustomValue, Datatype};
use rusqlite::{OptionalExtension, Transaction, params};
use serde::Serialize;

use super::store::{Cover, store_book};
use crate::error::{Error, Result};
use crate::html;
use crate::library::{BookId, Library, NewBook, now, timestamp};

const SOURCE: &str = "calibre";

#[derive(Debug, Default, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub library_created: bool,
    pub books_imported: usize,
    /// Already imported from this calibre library on an earlier run.
    pub books_skipped: usize,
    pub files: usize,
    pub covers: usize,
    /// Copy-on-write clones (no extra disk space).
    pub cloned: usize,
    /// Plain copies, and the bytes they wrote.
    pub copied: usize,
    pub copied_bytes: u64,
    pub custom_fields: usize,
    /// Books that couldn't be imported, with the reason. Their rows were rolled
    /// back and anything written for them removed; the rest of the import continued.
    pub failed: Vec<String>,
    /// Non-fatal problems: missing files, skipped computed columns, …
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct Progress<'a> {
    pub done: usize,
    pub total: usize,
    pub title: &'a str,
}

/// Import the calibre library at `source` into the Magnitide library at
/// `dest`, creating it if needed (named after the calibre library).
pub fn import_calibre(source: &Path, dest: &Path, mut progress: impl FnMut(Progress<'_>)) -> Result<ImportReport> {
    let calibre = calibre_compat::Library::open(source).map_err(|e| Error::Import(format!("{e:#}")))?;
    let src_root = canonical(calibre.root())?;
    let dest_abs = absolute(dest)?;
    // Never write inside the calibre library, or make it part of ours.
    if dest_abs.starts_with(&src_root) || src_root.starts_with(&dest_abs) {
        return Err(Error::Import(format!(
            "the Magnitide library ({}) must be outside the calibre library ({}), and not contain it",
            dest_abs.display(),
            src_root.display()
        )));
    }
    let snap = calibre.load().map_err(|e| Error::Import(format!("{e:#}")))?;

    let mut report = ImportReport::default();
    report.warnings.extend(snap.warnings.iter().map(|w| format!("calibre: {w}")));

    // A new library goes in a new or empty folder, never scattered through an
    // existing one (Documents, Downloads, …).
    if !Library::exists_at(&dest_abs) && dest_abs.is_dir() && fs::read_dir(&dest_abs)?.next().is_some() {
        return Err(Error::Import(format!(
            "{} isn't empty; choose a new or empty folder for the Magnitide library",
            dest_abs.display()
        )));
    }

    let mut lib = if Library::exists_at(&dest_abs) {
        Library::open(&dest_abs)?
    } else {
        report.library_created = true;
        // Named after the folder the user chose ("Magnitide Library"), not the
        // calibre library, so the two are easy to tell apart.
        let name = dest_abs.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Library".into());
        Library::create(&dest_abs, &name)?
    };

    // Identifies this calibre library in provenance records. Very old
    // libraries may lack one; fall back to the folder path.
    let source_library = snap.library_uuid.clone().unwrap_or_else(|| format!("path:{}", src_root.display()));

    let fields = import_custom_fields(&mut lib, &snap.custom_columns, &mut report)?;

    // Each book commits on its own (so a failure rolls back just that book), but
    // SQLite's automatic WAL checkpoint after every ~1000 pages would then
    // fsync constantly: it was ~70% of import time. Checkpoint rarely during
    // the import and once at the end instead; durability per commit is unchanged.
    lib.conn.pragma_update(None, "wal_autocheckpoint", 100_000)?;

    let total = snap.books.len();
    for (done, book) in snap.books.iter().enumerate() {
        progress(Progress { done, total, title: &book.title });
        if already_imported(&lib, &source_library, book.id)? {
            report.books_skipped += 1;
            continue;
        }
        match import_book(&mut lib, calibre.root(), book, &source_library, &fields, &mut report) {
            Ok(()) => report.books_imported += 1,
            Err(e) => report.failed.push(e.to_string()),
        }
    }
    lib.conn.pragma_update(None, "wal_autocheckpoint", 1000)?;
    lib.conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
    progress(Progress { done: total, total, title: "" });
    Ok(report)
}

fn import_book(
    lib: &mut Library,
    src_root: &Path,
    book: &calibre_compat::Book,
    source_library: &str,
    fields: &HashMap<String, (i64, Datatype)>,
    report: &mut ImportReport,
) -> Result<()> {
    // calibre ids are unique within a library, but two libraries can share a
    // book uuid (e.g. one copied from the other). Keep the uuid when free.
    let uuid = book.uuid.clone().filter(|u| !uuid_taken(lib, u).unwrap_or(true));
    if book.uuid.is_some() && uuid.is_none() {
        report.warnings.push(format!("“{}”: uuid already used in this library; assigned a new one", book.title));
    }

    let new = NewBook {
        title: book.title.clone(),
        title_sort: book.sort.clone(),
        authors: book.authors.clone(),
        series: book.series.clone().map(|s| (s, book.series_index)),
        publisher: book.publisher.clone(),
        published: book.pubdate.map(|d| d.date_naive()),
        description_html: book.comments.as_deref().map(html::sanitize),
        tags: book.tags.clone(),
        languages: book.languages.clone(),
        identifiers: book.identifiers.clone(),
        rating: book.rating,
        uuid,
        added_at: book.timestamp,
        author_sorts: Default::default(),
    };

    let mut files = Vec::new();
    for f in &book.formats {
        let src = book.format_path(src_root, f);
        if src.is_file() {
            files.push((f.format.to_uppercase(), src));
        } else {
            report.warnings.push(format!("“{}”: {} file missing in the calibre library", book.title, f.format));
        }
    }
    let cover_path = book.cover_path(src_root).filter(|p| p.is_file());

    let stored = store_book(lib, &new, &files, cover_path.as_deref().map(Cover::File), |tx, id| {
        insert_custom_values(tx, id, book, fields)?;
        tx.prepare_cached(
            "INSERT INTO book_sources (book_id, source, source_library, source_book, imported_at) VALUES (?, ?, ?, ?, ?)",
        )?
        .execute(params![id, SOURCE, source_library, book.id.to_string(), now()])?;
        Ok(())
    })?;
    report.files += stored.files;
    report.covers += stored.covers;
    report.cloned += stored.cloned;
    report.copied += stored.copied;
    report.copied_bytes += stored.copied_bytes;
    report.warnings.extend(stored.warnings);
    Ok(())
}

/// Create (or reuse, on a repeat import) a custom field per calibre custom
/// column. Returns lookup name ("#genre") → (field id, datatype).
pub(crate) fn import_custom_fields(
    lib: &mut Library,
    columns: &[CustomColumn],
    report: &mut ImportReport,
) -> Result<HashMap<String, (i64, Datatype)>> {
    let tx = lib.conn.transaction()?;
    let mut map = HashMap::new();
    for col in columns {
        let kind = match &col.datatype {
            Datatype::Text => "text",
            Datatype::Comments => "long_text",
            Datatype::Series => "series",
            Datatype::Enumeration => "choice",
            Datatype::Rating => "rating",
            Datatype::Int => "integer",
            Datatype::Float => "number",
            Datatype::Bool => "bool",
            Datatype::Datetime => "date",
            Datatype::Composite => {
                let template = col.composite_template().unwrap_or("?");
                report.warnings.push(format!(
                    "custom column #{} ({}) is computed by a calibre template ({template}), so it has no stored values to import",
                    col.label, col.name
                ));
                continue;
            }
            Datatype::Unknown(t) => {
                report.warnings.push(format!("custom column #{}: unknown type {t:?}, skipped", col.label));
                continue;
            }
        };
        let existing: Option<i64> = tx
            .prepare_cached("SELECT id FROM custom_fields WHERE key = ?")?
            .query_row([&col.label], |r| r.get(0))
            .optional()?;
        let id = match existing {
            Some(id) => id,
            None => {
                let config = serde_json::json!({ "calibre": col.display }).to_string();
                tx.prepare_cached(
                    "INSERT INTO custom_fields (key, name, kind, multiple, config) VALUES (?, ?, ?, ?, ?)",
                )?
                .execute(params![col.label, col.name, kind, col.is_multiple, config])?;
                report.custom_fields += 1;
                tx.last_insert_rowid()
            }
        };
        map.insert(col.lookup_name(), (id, col.datatype.clone()));
    }
    tx.commit()?;
    Ok(map)
}

pub(crate) fn insert_custom_values(
    tx: &Transaction<'_>,
    book_id: BookId,
    book: &calibre_compat::Book,
    fields: &HashMap<String, (i64, Datatype)>,
) -> Result<()> {
    for (key, value) in &book.custom {
        let Some((field, datatype)) = fields.get(key) else { continue };
        insert_custom_value(tx, book_id, *field, datatype, value)?;
    }
    Ok(())
}

/// Store one custom value: a `custom_values` row per item (one, except for
/// multi-valued text), text in `text_value` and numbers in `num_value`.
pub(crate) fn insert_custom_value(
    tx: &Transaction<'_>,
    book_id: BookId,
    field: i64,
    datatype: &Datatype,
    value: &CustomValue,
) -> Result<()> {
    let rows: Vec<(Option<String>, Option<f64>)> = match value {
        CustomValue::Text(s) if *datatype == Datatype::Comments => vec![(Some(html::sanitize(s)), None)],
        CustomValue::Text(s) => vec![(Some(s.clone()), None)],
        CustomValue::TextList(v) => v.iter().map(|s| (Some(s.clone()), None)).collect(),
        CustomValue::Series { name, index } => vec![(Some(name.clone()), Some(*index))],
        CustomValue::Int(i) => vec![(None, Some(*i as f64))],
        CustomValue::Float(f) => vec![(None, Some(*f))],
        CustomValue::Bool(b) => vec![(None, Some(if *b { 1.0 } else { 0.0 }))],
        CustomValue::Rating(r) => vec![(None, Some(f64::from(*r)))],
        CustomValue::Datetime(d) => vec![(Some(timestamp(*d)), None)],
    };
    let mut insert = tx.prepare_cached(
        "INSERT INTO custom_values (book_id, field_id, position, text_value, num_value) VALUES (?, ?, ?, ?, ?)",
    )?;
    for (pos, (text, num)) in (0_i64..).zip(rows) {
        insert.execute(params![book_id, field, pos, text, num])?;
    }
    Ok(())
}

fn already_imported(lib: &Library, source_library: &str, calibre_id: i64) -> Result<bool> {
    Ok(lib
        .conn
        .prepare_cached("SELECT 1 FROM book_sources WHERE source = ? AND source_library = ? AND source_book = ?")?
        .query_row(params![SOURCE, source_library, calibre_id.to_string()], |_| Ok(()))
        .optional()?
        .is_some())
}

fn uuid_taken(lib: &Library, uuid: &str) -> Result<bool> {
    Ok(lib
        .conn
        .prepare_cached("SELECT 1 FROM books WHERE uuid = ?")?
        .query_row([uuid], |_| Ok(()))
        .optional()?
        .is_some())
}

fn canonical(p: &Path) -> Result<PathBuf> {
    Ok(fs::canonicalize(p)?)
}

/// Absolute form of a path that may not exist yet: canonicalize its nearest
/// existing ancestor (resolving symlinks such as /tmp → /private/tmp).
fn absolute(p: &Path) -> Result<PathBuf> {
    let p = if p.is_absolute() { p.to_path_buf() } else { std::env::current_dir()?.join(p) };
    let mut existing = p.as_path();
    let mut rest = Vec::new();
    while !existing.exists() {
        rest.push(existing.file_name().map(|n| n.to_owned()));
        existing = match existing.parent() {
            Some(parent) => parent,
            None => break,
        };
    }
    let mut out = fs::canonicalize(existing)?;
    for name in rest.into_iter().rev().flatten() {
        out.push(name);
    }
    Ok(out)
}
