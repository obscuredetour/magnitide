use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use calibre_compat::locate;
use magnitide_catalog::{Book, CustomValue, Datatype, Query, SortDir, SortKey, sort_books, sort_books_by_field};
use magnitide_core::{CustomField, FieldKind};
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
    /// Identifies the library (its uuid, or a hash of its folder). Book ids are
    /// only unique within a library, so cover URLs carry this to keep the
    /// webview from showing another library's cached cover for the same id.
    cache_key: String,
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
            cache_key: cache_key.clone(),
            kind,
            book_count: catalog.books.len(),
            load_ms: t.elapsed().as_millis(),
            warnings: catalog.warnings.clone(),
            on_network_share: magnitide_core::fsinfo::is_network_filesystem(&root),
        };
        Ok((
            Loaded { root, name: info.name.clone(), kind, catalog, native, thumb_dir: thumbs_root.join(cache_key) },
            info,
        ))
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| format!("{e:#}"))?;

    if let Some(prev) = state.current().filter(|prev| !same_path(&prev.root, &loaded.root)) {
        drop(prev);
        let handle = app.clone();
        tauri::async_runtime::spawn_blocking(move || crate::plugins::library_closing(&handle))
            .await
            .map_err(|e| e.to_string())?;
    }
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
    let hooks = crate::plugins::AppHooks(app.clone());
    let handle = app.clone();
    let report = write_library(&state, move |lib| {
        let mut last = Instant::now();
        magnitide_core::import::add_files_with(lib, &paths, &hooks, &hooks, |p| {
            if p.done == 0 || p.done == p.total || last.elapsed().as_millis() >= 50 {
                last = Instant::now();
                let _ = handle.emit(
                    ADD_PROGRESS_EVENT,
                    ImportProgress { done: p.done, total: p.total, title: p.title.to_owned() },
                );
            }
        })
    })
    .await?;
    // File-type plugins that run after an add (on the shadow library).
    let mut items: Vec<(i64, Vec<String>)> = Vec::new();
    for &(id, ref format) in &report.formats {
        match items.iter_mut().find(|(b, _)| *b == id) {
            Some((_, formats)) => formats.push(format.clone()),
            None => items.push((id, vec![format.clone()])),
        }
    }
    crate::plugins::post_hooks(&app, magnitide_plugins::Hook::Postimport, items.clone()).await;
    crate::plugins::post_hooks(&app, magnitide_plugins::Hook::Postadd, items).await;
    Ok(report)
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
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
) -> CmdResult<Option<magnitide_core::writeback::FileMetadataStatus>> {
    let Some(native) = state.current().ok_or("No library is open")?.native.clone() else { return Ok(None) };
    let hooks = crate::plugins::AppHooks(app);
    tauri::async_runtime::spawn_blocking(move || {
        native.lock().unwrap_or_else(|e| e.into_inner()).file_metadata_status_with(id, &hooks).map(Some)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

/// Write a book's metadata and cover into its files that differ (EPUB, and
/// formats a metadata writer plugin handles).
#[tauri::command]
pub async fn write_metadata_to_files(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
) -> CmdResult<magnitide_core::writeback::WriteReport> {
    let hooks = crate::plugins::AppHooks(app);
    write_library(&state, move |lib| lib.write_metadata_to_files_with(id, &hooks)).await
}

/// How many books "Write metadata to files" would rewrite.
#[tauri::command]
pub async fn writeback_preview(
    app: AppHandle,
    state: State<'_, AppState>,
) -> CmdResult<magnitide_core::writeback::WritebackPreview> {
    let native = native(&state)?;
    let hooks = crate::plugins::AppHooks(app);
    tauri::async_runtime::spawn_blocking(move || {
        native.lock().unwrap_or_else(|e| e.into_inner()).writeback_preview_with(&hooks)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

/// Write every book's metadata into its files that differ from the library.
#[tauri::command]
pub async fn write_all_metadata(
    app: AppHandle,
    state: State<'_, AppState>,
) -> CmdResult<magnitide_core::writeback::WriteReport> {
    let hooks = crate::plugins::AppHooks(app);
    write_library(&state, move |lib| lib.write_all_metadata_with(|_, _| {}, &hooks)).await
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

/// Totals over the open library, for the sidebar.
#[derive(Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct LibraryStats {
    /// Distinct authors, compared case-insensitively (as `name_lists` keys them).
    authors: usize,
    /// Distinct series, main and further ones alike.
    series: usize,
    /// Each format and how many books have it, most common first.
    formats: Vec<FormatCount>,
    /// Bytes of every book file as the library records them (not measured on disk).
    total_bytes: u64,
}

#[derive(Serialize, TS)]
#[ts(export)]
pub struct FormatCount {
    format: String,
    count: usize,
}

#[tauri::command]
pub async fn library_stats(state: State<'_, AppState>) -> CmdResult<LibraryStats> {
    let lib = state.current().ok_or("No library is open")?;
    Ok(stats_of(&lib.catalog))
}

fn stats_of(catalog: &magnitide_catalog::Catalog) -> LibraryStats {
    let (mut authors, mut series) = (HashSet::<String>::new(), HashSet::<String>::new());
    let mut formats: BTreeMap<&str, usize> = BTreeMap::new();
    let mut total_bytes = 0u64;
    for b in &catalog.books {
        authors.extend(b.authors.iter().map(|a| a.to_lowercase()));
        series.extend(b.series.iter().chain(b.more_series.iter().map(|s| &s.name)).map(|s| s.to_lowercase()));
        let mut seen: Vec<&str> = Vec::new(); // a book counts once per format
        for f in &b.formats {
            total_bytes += f.size;
            if !seen.contains(&f.format.as_str()) {
                seen.push(&f.format);
                *formats.entry(&f.format).or_default() += 1;
            }
        }
    }
    let mut formats: Vec<FormatCount> =
        formats.into_iter().map(|(format, count)| FormatCount { format: format.to_owned(), count }).collect();
    formats.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.format.cmp(&b.format)));
    LibraryStats { authors: authors.len(), series: series.len(), formats, total_bytes }
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
    let handle = app.clone();
    let format = to.clone();
    let report = write_library(&state, move |lib| {
        let converter = magnitide_core::convert::Converter::detect();
        lib.convert_books(&book_ids, &to, &converter, |done, total, title| {
            let _ = handle.emit(CONVERT_PROGRESS_EVENT, ImportProgress { done, total, title: title.to_owned() });
        })
    })
    .await?;
    let items = report.converted_ids.iter().map(|&id| (id, vec![format.clone()])).collect();
    crate::plugins::post_hooks(&app, magnitide_plugins::Hook::Postconvert, items).await;
    Ok(report)
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

/// Delete books, moving their folders to the Trash.
#[tauri::command]
pub async fn delete_books(
    state: State<'_, AppState>,
    ids: Vec<i64>,
) -> CmdResult<magnitide_core::delete::DeleteReport> {
    write_library(&state, move |lib| lib.delete_books(&ids, |path| trash::delete(path).map_err(std::io::Error::other)))
        .await
}

/// Remove one format from a book, moving its file to the Trash.
#[tauri::command]
pub async fn remove_format(app: AppHandle, state: State<'_, AppState>, id: i64, format: String) -> CmdResult<()> {
    let removed = format.clone();
    write_library(&state, move |lib| {
        lib.remove_format(id, &format, |path| trash::delete(path).map_err(std::io::Error::other))
    })
    .await?;
    crate::plugins::post_hooks(&app, magnitide_plugins::Hook::Postdelete, vec![(id, vec![removed])]).await;
    Ok(())
}

pub(crate) const READ_ONLY: &str =
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

    state.replace(lib.with_catalog(catalog));
    Ok(out)
}

/// Rename the open Magnitide library. Returns the cleaned-up name.
#[tauri::command]
pub async fn rename_library(state: State<'_, AppState>, name: String) -> CmdResult<String> {
    let lib = state.current().ok_or("No library is open")?;
    let native = lib.native.clone().ok_or_else(|| READ_ONLY.to_owned())?;
    let (name, catalog) = tauri::async_runtime::spawn_blocking(move || {
        let mut native = native.lock().unwrap_or_else(|e| e.into_inner());
        native.set_name(&name)?;
        Ok::<_, magnitide_core::Error>((native.info()?.name, native.load_catalog()?))
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    state.replace(Loaded { name: name.clone(), ..lib.with_catalog(catalog) });
    Ok(name)
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
    /// Custom fields with a value, by key ("pages"), as text the UI formats by
    /// the field's kind: numbers, "true"/"false", ISO dates, "Name [2]" for
    /// series, comma-separated lists.
    custom: BTreeMap<String, String>,
}

impl From<&Book> for BookRow {
    fn from(b: &Book) -> Self {
        let date = |d: Option<chrono::DateTime<chrono::Utc>>| d.map(|d| d.format("%Y-%m-%d").to_string());
        Self {
            id: b.id,
            title: b.title.clone(),
            authors: b.authors.join(" & "),
            series: b.series.clone(),
            series_index: b.series.as_ref().map(|_| magnitide_catalog::format_number(b.series_index, b.series_end)),
            has_cover: b.has_cover,
            rating: b.rating,
            publisher: b.publisher.clone(),
            published: date(b.pubdate),
            added: date(b.timestamp),
            tags: b.tags.join(", "),
            formats: b.formats.iter().map(|f| f.format.clone()).collect(),
            size: b.formats.iter().map(|f| f.size).max(),
            custom: b.custom.iter().map(|(k, v)| (k.trim_start_matches('#').to_owned(), raw_value(v))).collect(),
        }
    }
}

/// A custom value as plain text (see `BookRow::custom`).
fn raw_value(v: &CustomValue) -> String {
    match v {
        CustomValue::Text(s) => s.clone(),
        CustomValue::TextList(l) => l.join(", "),
        CustomValue::Series { name, index } => format!("{name} [{}]", magnitide_catalog::format_number(*index, None)),
        CustomValue::Int(i) => i.to_string(),
        CustomValue::Float(x) => x.to_string(),
        CustomValue::Bool(b) => b.to_string(),
        CustomValue::Rating(r) => r.to_string(),
        CustomValue::Datetime(d) => d.format("%Y-%m-%d").to_string(),
    }
}

/// Books matching `query`, sorted by `sort` (in `dir`, or the key's natural
/// direction when omitted), or by the custom field `field` ("pages") if given.
#[tauri::command]
pub async fn list_books(
    state: State<'_, AppState>,
    query: String,
    sort: SortKey,
    dir: Option<SortDir>,
    field: Option<String>,
) -> CmdResult<Vec<BookRow>> {
    let lib = state.current().ok_or("No library is open")?;
    let query = query.trim();
    let query = (!query.is_empty()).then(|| Query::parse(query)).transpose().map_err(|e| e.to_string())?;
    let mut books: Vec<&Book> =
        lib.catalog.books.iter().filter(|b| query.as_ref().is_none_or(|q| q.matches(b))).collect();
    let column = field.as_deref().and_then(|f| lib.catalog.custom_columns.iter().find(|c| c.label == f));
    match column {
        Some(c) => {
            // Text A→Z; numbers, dates, ratings and yes first, highest first.
            let text =
                matches!(c.datatype, Datatype::Text | Datatype::Comments | Datatype::Enumeration | Datatype::Series);
            let natural = if text { SortDir::Asc } else { SortDir::Desc };
            sort_books_by_field(&mut books, &c.lookup_name(), dir.unwrap_or(natural));
        }
        None => sort_books(&mut books, sort, dir.unwrap_or(sort.natural_dir())),
    }
    Ok(books.into_iter().map(BookRow::from).collect())
}

/// These books' list rows, in the order asked (unknown ids are skipped).
#[tauri::command]
pub async fn book_rows(state: State<'_, AppState>, ids: Vec<i64>) -> CmdResult<Vec<BookRow>> {
    let lib = state.current().ok_or("No library is open")?;
    Ok(ids.iter().filter_map(|id| lib.catalog.get(*id)).map(BookRow::from).collect())
}

#[derive(Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct BookDetails {
    id: i64,
    title: String,
    authors: Vec<String>,
    /// The main series, then any further ones.
    series: Vec<SeriesLine>,
    /// Shelves it's on.
    shelves: Vec<String>,
    stars: Option<f32>,
    /// Books this omnibus contains.
    collects: Vec<BookLink>,
    /// Omnibuses that contain this book.
    collected_in: Vec<BookLink>,
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
    /// Custom fields with a value, in the library's field order.
    custom: Vec<CustomDetail>,
}

/// A custom field's value for the details panel.
#[derive(Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct CustomDetail {
    key: String,
    name: String,
    kind: FieldKind,
    /// As in `BookRow::custom`; long text is sanitized HTML.
    value: String,
}

/// "Book 3 of Mistborn", or "Books 1–3 of …" for an omnibus.
#[derive(Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SeriesLine {
    name: String,
    /// "3", or "1–3" for an omnibus.
    number: String,
    omnibus: bool,
}

#[derive(Serialize, TS)]
#[ts(export)]
pub struct BookLink {
    id: i64,
    title: String,
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
        series: {
            let line = |name: &str, index: f64, end: Option<f64>| SeriesLine {
                name: name.to_owned(),
                number: magnitide_catalog::format_number(index, end),
                omnibus: end.is_some(),
            };
            b.series
                .iter()
                .map(|s| line(s, b.series_index, b.series_end))
                .chain(b.more_series.iter().map(|s| line(&s.name, s.index, s.end)))
                .collect()
        },
        shelves: b.shelves.clone(),
        collects: links(&lib.catalog, &b.collects),
        collected_in: links(&lib.catalog, &b.collected_in),
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
        custom: lib
            .catalog
            .custom_columns
            .iter()
            .filter_map(CustomField::from_column)
            .filter_map(|f| {
                let v = b.custom.get(&format!("#{}", f.key))?;
                let value = match f.kind {
                    FieldKind::LongText => sanitize(&raw_value(v)),
                    _ => raw_value(v),
                };
                Some(CustomDetail { key: f.key, name: f.name, kind: f.kind, value })
            })
            .collect(),
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

fn links(catalog: &magnitide_catalog::Catalog, ids: &[i64]) -> Vec<BookLink> {
    ids.iter().filter_map(|id| catalog.get(*id)).map(|b| BookLink { id: b.id, title: b.title.clone() }).collect()
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
    fn totals_the_catalog() {
        use magnitide_catalog::{Format, SeriesEntry};
        let fmt = |format: &str, size: u64| Format { format: format.into(), size, name: String::new() };
        let books = vec![
            Book {
                id: 1,
                authors: vec!["Ann".into(), "Bo".into()],
                series: Some("Dune".into()),
                formats: vec![fmt("EPUB", 100), fmt("PDF", 1000)],
                ..Default::default()
            },
            Book {
                id: 2,
                authors: vec!["ann".into()],
                more_series: vec![SeriesEntry { name: "Other".into(), index: 1.0, end: None }],
                formats: vec![fmt("EPUB", 10)],
                ..Default::default()
            },
            Book { id: 3, authors: vec!["Cy".into()], formats: vec![fmt("EPUB", 1)], ..Default::default() },
        ];
        let s = stats_of(&magnitide_catalog::Catalog::new(None, vec![], books, vec![]));
        assert_eq!((s.authors, s.series, s.total_bytes), (3, 2, 1111));
        let formats: Vec<(&str, usize)> = s.formats.iter().map(|f| (f.format.as_str(), f.count)).collect();
        assert_eq!(formats, [("EPUB", 3), ("PDF", 1)]);
    }

    #[test]
    fn formats_series_index() {
        assert_eq!(magnitide_catalog::format_number(3.0, None), "3");
        assert_eq!(magnitide_catalog::format_number(2.5, None), "2.5");
        assert_eq!(magnitide_catalog::format_number(1.0, Some(3.0)), "1–3");
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

// ---------- Shelves ----------

/// The open library's shelves (none for calibre libraries).
#[tauri::command]
pub async fn list_shelves(state: State<'_, AppState>) -> CmdResult<Vec<magnitide_core::shelves::Shelf>> {
    let Some(native) = state.current().and_then(|l| l.native.clone()) else { return Ok(Vec::new()) };
    tauri::async_runtime::spawn_blocking(move || native.lock().unwrap_or_else(|e| e.into_inner()).shelves())
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn create_shelf(state: State<'_, AppState>, name: String) -> CmdResult<magnitide_core::shelves::Shelf> {
    write_library(&state, move |lib| lib.create_shelf(&name)).await
}

#[tauri::command]
pub async fn rename_shelf(state: State<'_, AppState>, id: i64, name: String) -> CmdResult<()> {
    write_library(&state, move |lib| lib.rename_shelf(id, &name)).await
}

#[tauri::command]
pub async fn delete_shelf(state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    write_library(&state, move |lib| lib.delete_shelf(id)).await
}

/// A shelf per series or per author with at least `min_books` books (see `Library::make_shelves`).
#[tauri::command]
pub async fn make_shelves(
    state: State<'_, AppState>,
    source: magnitide_core::shelves::ShelfSource,
    min_books: usize,
) -> CmdResult<magnitide_core::shelves::ShelvesMade> {
    write_library(&state, move |lib| lib.make_shelves(source, min_books)).await
}

/// Put books on a shelf; how many weren't on it already.
#[tauri::command]
pub async fn add_to_shelf(state: State<'_, AppState>, id: i64, book_ids: Vec<i64>) -> CmdResult<usize> {
    write_library(&state, move |lib| lib.add_to_shelf(id, &book_ids)).await
}

/// Take books off a shelf; how many were on it.
#[tauri::command]
pub async fn remove_from_shelf(state: State<'_, AppState>, id: i64, book_ids: Vec<i64>) -> CmdResult<usize> {
    write_library(&state, move |lib| lib.remove_from_shelf(id, &book_ids)).await
}
