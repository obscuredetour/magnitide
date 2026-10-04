use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use calibre_compat::locate;
use magnitide_catalog::{Book, Query, SortDir, SortKey, sort_books};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use ts_rs::TS;

use crate::{AppState, LibraryKind, Loaded};

pub(crate) type CmdResult<T> = Result<T, String>;

#[derive(Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct LibraryInfo {
    path: String,
    name: String,
    kind: LibraryKind,
    book_count: usize,
    load_ms: u128,
    warnings: Vec<String>,
    /// On SMB/NFS/…: SQLite and file locks aren't reliable there.
    on_network_share: bool,
}

/// Open a Magnitide or calibre library at `path` (calibre ones read-only), or
/// calibre's current/default library when `path` is None.
#[tauri::command]
pub async fn open_library(app: AppHandle, state: State<'_, AppState>, path: Option<String>) -> CmdResult<LibraryInfo> {
    let path = match path {
        Some(p) => PathBuf::from(p),
        None => locate::default_library()
            .ok_or("No library found. Choose a Magnitide or calibre library folder to open.")?,
    };
    let thumbs_root = thumbs_root(&app)?;
    // Reopening the library we already hold must reuse its handle: opening it
    // again would trip our own single-writer lock.
    let current = state.current().filter(|l| l.native.is_some() && same_path(&l.root, &path));

    let (loaded, info) = tauri::async_runtime::spawn_blocking(move || -> anyhow::Result<_> {
        let t = Instant::now();
        let (root, kind, name, catalog, native) = if let Some(current) = current {
            let native = current.native.clone().expect("filtered on native above");
            let (name, catalog) = {
                let lib = native.lock().unwrap_or_else(|e| e.into_inner());
                (lib.info()?.name, lib.load_catalog()?)
            };
            (current.root.clone(), LibraryKind::Magnitide, name, catalog, Some(native))
        } else if magnitide_core::Library::exists_at(&path) {
            let lib = magnitide_core::Library::open(&path)?;
            let (root, name, catalog) = (lib.root().to_path_buf(), lib.info()?.name, lib.load_catalog()?);
            (root, LibraryKind::Magnitide, name, catalog, Some(Arc::new(Mutex::new(lib))))
        } else {
            let lib = calibre_compat::Library::open(&path)?;
            let root = lib.root().to_path_buf();
            let name = root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            (root, LibraryKind::Calibre, name, lib.load()?, None)
        };
        let cache_key = catalog.library_uuid.clone().unwrap_or_else(|| {
            let mut h = DefaultHasher::new();
            root.hash(&mut h);
            format!("path-{:016x}", h.finish())
        });
        let info = LibraryInfo {
            path: root.display().to_string(),
            name,
            kind,
            book_count: catalog.books.len(),
            load_ms: t.elapsed().as_millis(),
            warnings: catalog.warnings.clone(),
            on_network_share: magnitide_core::fsinfo::is_network_filesystem(&root),
        };
        Ok((Loaded { root, kind, catalog, native, thumb_dir: thumbs_root.join(cache_key) }, info))
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| format!("{e:#}"))?;

    state.replace(loaded);
    Ok(info)
}

/// Sent while books are being added. Event name matches `onAddProgress` in ui/src/api.ts.
pub const ADD_PROGRESS_EVENT: &str = "add-progress";

/// Add book files and folders (drag and drop, "Add Books…") to the open
/// Magnitide library, then reload it. Progress arrives as `add-progress` events
/// (same payload as import progress).
#[tauri::command]
pub async fn add_books(
    app: AppHandle,
    state: State<'_, AppState>,
    paths: Vec<String>,
) -> CmdResult<magnitide_core::import::AddReport> {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    write_library(&state, move |lib| {
        let mut last = Instant::now();
        magnitide_core::import::add_files(lib, &paths, |p| {
            if p.done == 0 || p.done == p.total || last.elapsed().as_millis() >= 50 {
                last = Instant::now();
                let _ = app.emit(
                    ADD_PROGRESS_EVENT,
                    ImportProgress { done: p.done, total: p.total, title: p.title.to_owned() },
                );
            }
        })
    })
    .await
}

/// How many books "Tidy file names" would rename.
#[tauri::command]
pub async fn tidy_preview(state: State<'_, AppState>) -> CmdResult<magnitide_core::tidy::TidyPreview> {
    let native = native(&state)?;
    tauri::async_runtime::spawn_blocking(move || native.lock().unwrap_or_else(|e| e.into_inner()).tidy_preview())
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// Rename books whose folder or file names don't match their metadata.
#[tauri::command]
pub async fn tidy_file_names(state: State<'_, AppState>) -> CmdResult<magnitide_core::tidy::TidyReport> {
    write_library(&state, |lib| lib.tidy_file_names(|_, _| {})).await
}

/// Whether a book's files hold its current metadata; null for calibre libraries.
#[tauri::command]
pub async fn file_metadata_status(
    state: State<'_, AppState>,
    id: i64,
) -> CmdResult<Option<magnitide_core::writeback::FileMetadataStatus>> {
    let Some(native) = state.current().ok_or("No library is open")?.native.clone() else { return Ok(None) };
    tauri::async_runtime::spawn_blocking(move || {
        native.lock().unwrap_or_else(|e| e.into_inner()).file_metadata_status(id).map(Some)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

/// Write a book's metadata and cover into its EPUB files (those that differ).
#[tauri::command]
pub async fn write_metadata_to_files(
    state: State<'_, AppState>,
    id: i64,
) -> CmdResult<magnitide_core::writeback::WriteReport> {
    write_library(&state, move |lib| lib.write_metadata_to_files(id)).await
}

/// How many books "Write metadata to files" would rewrite.
#[tauri::command]
pub async fn writeback_preview(state: State<'_, AppState>) -> CmdResult<magnitide_core::writeback::WritebackPreview> {
    let native = native(&state)?;
    tauri::async_runtime::spawn_blocking(move || native.lock().unwrap_or_else(|e| e.into_inner()).writeback_preview())
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// Write every book's metadata into its EPUB files that differ from the library.
#[tauri::command]
pub async fn write_all_metadata(state: State<'_, AppState>) -> CmdResult<magnitide_core::writeback::WriteReport> {
    write_library(&state, |lib| lib.write_all_metadata(|_, _| {})).await
}

/// Names already used in the library, for autocomplete in the edit form.
#[derive(Serialize, TS)]
#[ts(export)]
pub struct NameLists {
    authors: Vec<String>,
    tags: Vec<String>,
    series: Vec<String>,
    publishers: Vec<String>,
}

#[tauri::command]
pub async fn name_lists(state: State<'_, AppState>) -> CmdResult<NameLists> {
    let lib = state.current().ok_or("No library is open")?;
    let sorted = |names: std::collections::BTreeMap<String, String>| names.into_values().collect::<Vec<_>>();
    // Keyed case-insensitively, keeping the first spelling seen.
    let (mut authors, mut tags, mut series, mut publishers) = Default::default();
    let add = |map: &mut std::collections::BTreeMap<String, String>, name: &str| {
        map.entry(name.to_lowercase()).or_insert_with(|| name.to_owned());
    };
    for b in &lib.catalog.books {
        b.authors.iter().for_each(|a| add(&mut authors, a));
        b.tags.iter().for_each(|t| add(&mut tags, t));
        b.series.iter().for_each(|s| add(&mut series, s));
        b.publisher.iter().for_each(|p| add(&mut publishers, p));
    }
    Ok(NameLists {
        authors: sorted(authors),
        tags: sorted(tags),
        series: sorted(series),
        publishers: sorted(publishers),
    })
}

/// How these authors sort (saved sort names, or the automatic ones for new authors).
#[tauri::command]
pub async fn author_sorts(
    state: State<'_, AppState>,
    names: Vec<String>,
) -> CmdResult<Vec<magnitide_core::AuthorSort>> {
    let native = native(&state)?;
    tauri::async_runtime::spawn_blocking(move || native.lock().unwrap_or_else(|e| e.into_inner()).author_sorts(&names))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// What these books have in common, for the bulk edit form.
#[tauri::command]
pub async fn bulk_values(state: State<'_, AppState>, ids: Vec<i64>) -> CmdResult<magnitide_core::bulk::BulkValues> {
    let native = native(&state)?;
    tauri::async_runtime::spawn_blocking(move || native.lock().unwrap_or_else(|e| e.into_inner()).bulk_values(&ids))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// Sent while several books are saved. Matches `onBulkProgress` in ui/src/api.ts.
pub const BULK_PROGRESS_EVENT: &str = "bulk-progress";

/// Apply one change to several books (in the order given, for series numbering).
#[tauri::command]
pub async fn bulk_edit(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<i64>,
    change: magnitide_core::bulk::BulkEdit,
) -> CmdResult<magnitide_core::bulk::BulkReport> {
    write_library(&state, move |lib| {
        lib.bulk_edit(&ids, &change, |done, total| {
            let _ = app.emit(BULK_PROGRESS_EVENT, ImportProgress { done, total, title: String::new() });
        })
    })
    .await
}

/// A book's editable fields, for the edit form.
#[tauri::command]
pub async fn book_edit(state: State<'_, AppState>, id: i64) -> CmdResult<magnitide_core::BookEdit> {
    let native = native(&state)?;
    tauri::async_runtime::spawn_blocking(move || native.lock().unwrap_or_else(|e| e.into_inner()).book_edit(id))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// Save an edit. Renames the book's folder and files when its title or first
/// author changes; all-or-nothing with the database.
#[tauri::command]
pub async fn save_book_edit(
    state: State<'_, AppState>,
    id: i64,
    edit: magnitide_core::BookEdit,
) -> CmdResult<magnitide_core::EditOutcome> {
    write_library(&state, move |lib| lib.update_book(id, &edit)).await
}

/// Sent while books are converted. Matches `onConvertProgress` in ui/src/api.ts.
pub const CONVERT_PROGRESS_EVENT: &str = "convert-progress";

/// What can convert books here.
#[derive(Serialize, TS)]
#[ts(export)]
pub struct ConversionInfo {
    /// calibre's `ebook-convert`, if installed (needed for everything but EPUB → KEPUB).
    calibre: Option<String>,
}

#[tauri::command]
pub async fn conversion_info() -> CmdResult<ConversionInfo> {
    let c = magnitide_core::convert::Converter::detect();
    Ok(ConversionInfo { calibre: c.calibre().map(|p| p.display().to_string()) })
}

/// Formats at least one of these books can be converted to (that it lacks).
#[tauri::command]
pub async fn convert_targets(state: State<'_, AppState>, book_ids: Vec<i64>) -> CmdResult<Vec<String>> {
    let lib = state.current().ok_or("No library is open")?;
    let c = magnitide_core::convert::Converter::detect();
    let mut out: Vec<String> = Vec::new();
    for id in book_ids {
        let Some(b) = lib.catalog.get(id) else { continue };
        let have: Vec<String> = b.formats.iter().map(|f| f.format.clone()).collect();
        for t in c.targets(&have) {
            if !out.contains(&t) {
                out.push(t);
            }
        }
    }
    // A stable order: the converter's.
    let all = c.targets(&["EPUB".to_owned(), "MOBI".to_owned()]);
    out.sort_by_key(|t| all.iter().position(|a| a == t).unwrap_or(usize::MAX));
    Ok(out)
}

/// Convert books to `to` and add the result to each as a new format.
#[tauri::command]
pub async fn convert_books(
    app: AppHandle,
    state: State<'_, AppState>,
    book_ids: Vec<i64>,
    to: String,
) -> CmdResult<magnitide_core::convert::ConvertReport> {
    write_library(&state, move |lib| {
        let converter = magnitide_core::convert::Converter::detect();
        lib.convert_books(&book_ids, &to, &converter, |done, total, title| {
            let _ = app.emit(CONVERT_PROGRESS_EVENT, ImportProgress { done, total, title: title.to_owned() });
        })
    })
    .await
}

/// Merge book `other` into book `keep` (formats, missing cover, tags and
/// identifiers), then remove `other`.
#[tauri::command]
pub async fn merge_books(
    state: State<'_, AppState>,
    keep: i64,
    other: i64,
) -> CmdResult<magnitide_core::merge::MergeOutcome> {
    write_library(&state, move |lib| lib.merge_books(keep, other)).await
}

/// Replace a book's cover with an image file (stored as JPEG).
#[tauri::command]
pub async fn set_cover_from_file(state: State<'_, AppState>, id: i64, path: String) -> CmdResult<()> {
    write_library(&state, move |lib| {
        let bytes = std::fs::read(&path)?;
        lib.set_cover(id, &bytes)
    })
    .await
}

#[tauri::command]
pub async fn remove_cover(state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    write_library(&state, move |lib| lib.remove_cover(id)).await
}

const READ_ONLY: &str =
    "This calibre library is read-only. Import it into a Magnitide library (library menu) to change it.";

/// The open Magnitide library, or an error for calibre libraries.
fn native(state: &State<'_, AppState>) -> CmdResult<Arc<Mutex<magnitide_core::Library>>> {
    state.current().ok_or("No library is open")?.native.clone().ok_or_else(|| READ_ONLY.to_owned())
}

/// Run a change against the open Magnitide library off the main thread, then
/// reload its catalog so every view (and search) sees the result.
pub(crate) async fn write_library<T: Send + 'static>(
    state: &State<'_, AppState>,
    change: impl FnOnce(&mut magnitide_core::Library) -> magnitide_core::Result<T> + Send + 'static,
) -> CmdResult<T> {
    let lib = state.current().ok_or("No library is open")?;
    let native = lib.native.clone().ok_or_else(|| READ_ONLY.to_owned())?;
    let (out, catalog) = tauri::async_runtime::spawn_blocking(move || {
        let mut native = native.lock().unwrap_or_else(|e| e.into_inner());
        let out = change(&mut native)?;
        Ok::<_, magnitide_core::Error>((out, native.load_catalog()?))
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;

    state.replace(Loaded {
        root: lib.root.clone(),
        kind: lib.kind,
        catalog,
        native: lib.native.clone(),
        thumb_dir: lib.thumb_dir.clone(),
    });
    Ok(out)
}

/// Rename the open Magnitide library. Returns the cleaned-up name.
#[tauri::command]
pub async fn rename_library(state: State<'_, AppState>, name: String) -> CmdResult<String> {
    write_library(&state, move |lib| {
        lib.set_name(&name)?;
        Ok(lib.info()?.name)
    })
    .await
}

/// Sent while an import runs. Event name matches `onImportProgress` in ui/src/api.ts.
pub const IMPORT_PROGRESS_EVENT: &str = "import-progress";

#[derive(Clone, Serialize, TS)]
#[ts(export)]
pub struct ImportProgress {
    pub(crate) done: usize,
    pub(crate) total: usize,
    pub(crate) title: String,
}

/// Import the open calibre library into a Magnitide library at `dest`
/// (created if new; repeat imports add only new books). The calibre library
/// is only read. Progress arrives as `import-progress` events.
#[tauri::command]
pub async fn import_calibre_library(
    app: AppHandle,
    state: State<'_, AppState>,
    dest: String,
) -> CmdResult<magnitide_core::import::ImportReport> {
    let lib = state.current().ok_or("No library is open")?;
    if lib.kind != LibraryKind::Calibre {
        return Err("Only calibre libraries can be imported.".into());
    }
    let src = lib.root.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut last = Instant::now();
        magnitide_core::import::import_calibre(&src, dest.as_ref(), |p| {
            // ~20 updates a second is plenty for a progress bar.
            if p.done == 0 || p.done == p.total || last.elapsed().as_millis() >= 50 {
                last = Instant::now();
                let _ = app.emit(
                    IMPORT_PROGRESS_EVENT,
                    ImportProgress { done: p.done, total: p.total, title: p.title.to_owned() },
                );
            }
        })
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

/// One book in the grid or table. Flat and display-ready, because every
/// search sends the whole result set across IPC.
#[derive(Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct BookRow {
    id: i64,
    title: String,
    authors: String,
    series: Option<String>,
    /// Formatted for display, e.g. "3" or "2.5".
    series_index: Option<String>,
    has_cover: bool,
    /// 0–10 (half stars), as calibre stores it.
    rating: Option<u8>,
    publisher: Option<String>,
    /// ISO date (`YYYY-MM-DD`); the UI formats it for the user's locale.
    published: Option<String>,
    /// ISO date (`YYYY-MM-DD`) the book was added to the library.
    added: Option<String>,
    /// Comma-separated.
    tags: String,
    formats: Vec<String>,
    /// Bytes of the largest format.
    size: Option<u64>,
}

impl From<&Book> for BookRow {
    fn from(b: &Book) -> Self {
        let date = |d: Option<chrono::DateTime<chrono::Utc>>| d.map(|d| d.format("%Y-%m-%d").to_string());
        Self {
            id: b.id,
            title: b.title.clone(),
            authors: b.authors.join(" & "),
            series: b.series.clone(),
            series_index: b.series.as_ref().map(|_| fmt_index(b.series_index)),
            has_cover: b.has_cover,
            rating: b.rating,
            publisher: b.publisher.clone(),
            published: date(b.pubdate),
            added: date(b.timestamp),
            tags: b.tags.join(", "),
            formats: b.formats.iter().map(|f| f.format.clone()).collect(),
            size: b.formats.iter().map(|f| f.size).max(),
        }
    }
}

/// Books matching `query`, sorted by `sort` (in `dir`, or the key's natural
/// direction when omitted).
#[tauri::command]
pub async fn list_books(
    state: State<'_, AppState>,
    query: String,
    sort: SortKey,
    dir: Option<SortDir>,
) -> CmdResult<Vec<BookRow>> {
    let lib = state.current().ok_or("No library is open")?;
    let query = query.trim();
    let query = (!query.is_empty()).then(|| Query::parse(query)).transpose().map_err(|e| e.to_string())?;
    let mut books: Vec<&Book> =
        lib.catalog.books.iter().filter(|b| query.as_ref().is_none_or(|q| q.matches(b))).collect();
    sort_books(&mut books, sort, dir.unwrap_or(sort.natural_dir()));
    Ok(books.into_iter().map(BookRow::from).collect())
}

#[derive(Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct BookDetails {
    id: i64,
    title: String,
    authors: Vec<String>,
    series: Option<String>,
    stars: Option<f32>,
    tags: Vec<String>,
    publisher: Option<String>,
    published: Option<String>,
    added: Option<String>,
    languages: Vec<String>,
    identifiers: Vec<(String, String)>,
    formats: Vec<FormatInfo>,
    /// Sanitized: scripts, styles, event handlers and images removed.
    comments_html: Option<String>,
    has_cover: bool,
    folder: String,
}

#[derive(Serialize, TS)]
#[ts(export)]
pub struct FormatInfo {
    format: String,
    size: u64,
    present: bool,
}

#[tauri::command]
pub async fn book_details(state: State<'_, AppState>, id: i64) -> CmdResult<BookDetails> {
    let lib = state.current().ok_or("No library is open")?;
    let b = lib.catalog.get(id).ok_or_else(|| format!("No book with id {id}"))?;
    Ok(BookDetails {
        id: b.id,
        title: b.title.clone(),
        authors: b.authors.clone(),
        series: b.series.as_ref().map(|s| format!("Book {} of {s}", fmt_index(b.series_index))),
        stars: b.stars(),
        tags: b.tags.clone(),
        publisher: b.publisher.clone(),
        published: b.pubdate.map(|d| d.format("%-d %B %Y").to_string()),
        added: b.timestamp.map(|d| d.format("%-d %B %Y").to_string()),
        languages: b.languages.clone(),
        identifiers: b.identifiers.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        formats: b
            .formats
            .iter()
            .map(|f| FormatInfo {
                format: f.format.clone(),
                size: f.size,
                present: b.format_path(&lib.root, f).is_file(),
            })
            .collect(),
        comments_html: b.comments.as_deref().map(sanitize),
        has_cover: b.has_cover,
        folder: b.dir(&lib.root).display().to_string(),
    })
}

/// Formats most likely to have a default reader app, best first. KEPUB is
/// near the end because only Kobo devices and apps understand the extension.
const OPEN_PREFERENCE: &[&str] =
    &["EPUB", "PDF", "AZW3", "MOBI", "AZW", "CBZ", "CBR", "FB2", "DOCX", "RTF", "TXT", "HTML", "KEPUB"];

/// `available` reordered by `OPEN_PREFERENCE`; unknown formats go last.
fn by_preference<'a>(available: &[&'a str]) -> Vec<&'a str> {
    let rank = |f: &str| OPEN_PREFERENCE.iter().position(|p| p.eq_ignore_ascii_case(f)).unwrap_or(usize::MAX);
    let mut v = available.to_vec();
    v.sort_by_key(|f| rank(f));
    v
}

/// Open a book's file in the system's default app for that format.
///
/// With `format`, only that file is tried. Otherwise each present format is
/// tried in preference order until one has an app to open it (on macOS, for
/// example, Books opens EPUB but nothing opens MOBI by default). Returns the
/// format that was opened.
#[tauri::command]
pub async fn open_book(state: State<'_, AppState>, id: i64, format: Option<String>) -> CmdResult<String> {
    let lib = state.current().ok_or("No library is open")?;
    let b = lib.catalog.get(id).ok_or_else(|| format!("No book with id {id}"))?;
    // Only paths derived from the database are opened; the UI just names a book and format.
    let present: Vec<_> = b.formats.iter().filter(|f| b.format_path(&lib.root, f).is_file()).collect();
    let names: Vec<&str> = present.iter().map(|f| f.format.as_str()).collect();
    let candidates = match &format {
        Some(want) => names.iter().copied().filter(|n| n.eq_ignore_ascii_case(want)).collect(),
        None => by_preference(&names),
    };
    if candidates.is_empty() {
        return Err(match &format {
            Some(want) => format!("“{}” has no {want} file in the library folder.", b.title),
            None => format!("“{}” has no book files in the library folder.", b.title),
        });
    }

    let attempts: Vec<(String, PathBuf)> = candidates
        .iter()
        .map(|fmt| {
            let f = present.iter().find(|f| f.format == *fmt).expect("candidates come from present formats");
            ((*fmt).to_owned(), b.format_path(&lib.root, f))
        })
        .collect();
    let tried = candidates.join(", ");

    // `open::that` waits for the launcher (`open`, `xdg-open`, `start`) to report
    // success, unlike the detached variant, so a format with no app falls
    // through to the next one instead of failing silently.
    let opened = tauri::async_runtime::spawn_blocking(move || {
        attempts.into_iter().find_map(|(fmt, path)| open::that(&path).is_ok().then_some(fmt))
    })
    .await
    .map_err(|e| e.to_string())?;
    if let Some(fmt) = opened {
        return Ok(fmt);
    }
    Err(if candidates.len() == 1 {
        format!("No app on this computer opens {tried} files.")
    } else {
        format!("No app on this computer opens this book’s formats ({tried}).")
    })
}

/// Show a book in Finder / File Explorer / the file manager, with its most
/// readable file selected (or its folder, if no files are present).
#[tauri::command]
pub async fn reveal_book(state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    let lib = state.current().ok_or("No library is open")?;
    let b = lib.catalog.get(id).ok_or_else(|| format!("No book with id {id}"))?;
    let present: Vec<_> = b.formats.iter().filter(|f| b.format_path(&lib.root, f).is_file()).collect();
    let names: Vec<&str> = present.iter().map(|f| f.format.as_str()).collect();
    let target = by_preference(&names)
        .first()
        .and_then(|fmt| present.iter().find(|f| f.format == *fmt))
        .map(|f| b.format_path(&lib.root, f))
        .unwrap_or_else(|| b.dir(&lib.root));
    tauri_plugin_opener::reveal_item_in_dir(&target).map_err(|e| format!("Couldn’t show the book’s files: {e}"))
}

/// Open the library's folder in Finder / File Explorer / the file manager.
#[tauri::command]
pub async fn show_library_folder(state: State<'_, AppState>) -> CmdResult<()> {
    let lib = state.current().ok_or("No library is open")?;
    let root = lib.root.clone();
    tauri::async_runtime::spawn_blocking(move || open::that(&root))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("Couldn’t open the library folder: {e}"))
}

/// Where cover thumbnails for all libraries are cached.
pub(crate) fn thumbs_root(app: &AppHandle) -> CmdResult<PathBuf> {
    Ok(app.path().app_cache_dir().map_err(|e| e.to_string())?.join("thumbs"))
}

#[derive(Serialize, TS)]
#[ts(export)]
pub struct CacheInfo {
    bytes: u64,
    files: u64,
}

/// Size of the thumbnail cache (all libraries).
#[tauri::command]
pub async fn thumbnail_cache_info(app: AppHandle) -> CmdResult<CacheInfo> {
    let root = thumbs_root(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let mut info = CacheInfo { bytes: 0, files: 0 };
        let mut dirs = vec![root];
        while let Some(dir) = dirs.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else { continue };
            for entry in entries.flatten() {
                match entry.metadata() {
                    Ok(m) if m.is_dir() => dirs.push(entry.path()),
                    Ok(m) => {
                        info.bytes += m.len();
                        info.files += 1;
                    }
                    Err(_) => {}
                }
            }
        }
        info
    })
    .await
    .map_err(|e| e.to_string())
}

/// Delete all cached thumbnails; they are regenerated as covers are shown.
#[tauri::command]
pub async fn clear_thumbnail_cache(app: AppHandle) -> CmdResult<()> {
    let root = thumbs_root(&app)?;
    tauri::async_runtime::spawn_blocking(move || match std::fs::remove_dir_all(&root) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(format!("Couldn’t clear the thumbnail cache: {e}")),
        _ => Ok(()),
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Comments are HTML from arbitrary sources (metadata downloads, publishers).
/// Anything that could run script or load remote content is stripped.
pub(crate) fn sanitize(html: &str) -> String {
    ammonia::Builder::default().rm_tags(["img"]).clean(html).to_string()
}

fn fmt_index(i: f64) -> String {
    if i.fract() == 0.0 { format!("{}", i as i64) } else { format!("{i}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orders_formats_by_readability() {
        assert_eq!(by_preference(&["KEPUB", "MOBI", "EPUB"]), ["EPUB", "MOBI", "KEPUB"]);
        assert_eq!(by_preference(&["ODT", "KEPUB"]), ["KEPUB", "ODT"]);
        assert!(by_preference(&[]).is_empty());
    }

    #[test]
    fn sanitizes_comments() {
        let dirty = r#"<p onclick="x()">Hi<script>alert(1)</script><img src="http://evil/x.png"><a href="javascript:x()">l</a></p>"#;
        let clean = sanitize(dirty);
        assert!(
            !clean.contains("script")
                && !clean.contains("onclick")
                && !clean.contains("img")
                && !clean.contains("javascript")
        );
        assert!(clean.contains("Hi"));
    }

    #[test]
    fn formats_series_index() {
        assert_eq!(fmt_index(3.0), "3");
        assert_eq!(fmt_index(2.5), "2.5");
    }
}

/// Serialization cost of a full `list_books` result, which every search pays.
/// Run with: MAGNITIDE_BENCH_LIBRARY=/path/to/library cargo test -p magnitide --release -- --ignored --nocapture
#[cfg(test)]
mod bench {
    use std::time::Instant;

    use super::*;

    #[test]
    #[ignore]
    fn list_payload_size() {
        let path = std::env::var("MAGNITIDE_BENCH_LIBRARY").expect("set MAGNITIDE_BENCH_LIBRARY");
        let snap = calibre_compat::Library::open(path).unwrap().load().unwrap();
        let t = Instant::now();
        let mut books: Vec<&Book> = snap.books.iter().collect();
        sort_books(&mut books, SortKey::Title, SortDir::Asc);
        let sorted = t.elapsed();
        let rows: Vec<BookRow> = books.into_iter().map(BookRow::from).collect();
        let json = serde_json::to_vec(&rows).unwrap();
        println!(
            "{} books: sort {sorted:.1?}, rows+json {:.1?}, {:.1} MB",
            rows.len(),
            t.elapsed() - sorted,
            json.len() as f64 / 1e6
        );
    }
}

/// Whether two paths name the same folder (resolving symlinks and `..`).
fn same_path(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}
