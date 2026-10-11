//! Reading comics in the app: the open comic's pages, served to the viewer as
//! `page://localhost/<book>/<n>`, and where the reader is in it (the library's
//! reading status and progress).

use std::sync::{Arc, Mutex};

use magnitide_catalog::CustomValue;
use magnitide_core::pages::{Pages, ReadingState};
use serde::Serialize;
use tauri::http::{Response, StatusCode, header};
use tauri::{AppHandle, Manager, State};
use ts_rs::TS;

use crate::AppState;
use crate::commands::CmdResult;

/// Formats the viewer reads, best first: a CBZ is read in place, a CB7 or CBR unpacked.
const COMIC_FORMATS: &[&str] = &["CBZ", "CB7", "CBR"];

/// The comic open in the viewer, if any.
#[derive(Default)]
pub struct Reader(Mutex<Option<(i64, Arc<Pages>)>>);

/// A comic opened for reading.
#[derive(Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct OpenedComic {
    id: i64,
    title: String,
    /// The format read: CBZ, CB7 or CBR.
    format: String,
    pages: usize,
    /// Where to start: where it was left, or the first page.
    page: usize,
    /// Its Right to Left comic field (manga).
    right_to_left: bool,
    /// None for a calibre library, which is only read.
    reading: Option<ReadingState>,
}

/// Open a book's comic file (CBZ, else CB7, else CBR) for the viewer. A RAR or 7z
/// is unpacked here, so this can take a moment for a big one.
#[tauri::command]
pub async fn open_comic(app: AppHandle, state: State<'_, AppState>, id: i64) -> CmdResult<OpenedComic> {
    let lib = state.current().ok_or("No library is open")?;
    let b = lib.catalog.get(id).ok_or_else(|| format!("No book with id {id}"))?;
    let file = COMIC_FORMATS
        .iter()
        .find_map(|want| b.formats.iter().find(|f| f.format == *want).filter(|f| b.format_path(&lib.root, f).is_file()))
        .ok_or_else(|| format!("“{}” has no CBZ, CB7 or CBR file to read.", b.title))?;
    let path = b.format_path(&lib.root, file);
    let format = file.format.clone();
    let title = b.title.clone();
    let right_to_left = matches!(b.custom.get("#right_to_left"), Some(CustomValue::Bool(true)));

    let pages = tauri::async_runtime::spawn_blocking(move || Pages::open(&path)).await.map_err(|e| e.to_string())??;
    if pages.is_empty() {
        return Err(format!("“{title}” has no pages Magnitide can show."));
    }
    let reading = match &lib.native {
        Some(native) => {
            Some(native.lock().unwrap_or_else(|e| e.into_inner()).reading_state(id).map_err(|e| e.to_string())?)
        }
        None => None,
    };
    let last = pages.len() - 1;
    // Where it was left; a book read to the end starts over.
    let page = match &reading {
        Some(r) if r.progress.is_some_and(|p| p < 1.0) => (r.progress.unwrap_or(0.0) * last as f64).round() as usize,
        _ => 0,
    }
    .min(last);
    let opened = OpenedComic { id, title, format, pages: pages.len(), page, right_to_left, reading };
    *app.state::<Reader>().0.lock().unwrap_or_else(|e| e.into_inner()) = Some((id, Arc::new(pages)));
    Ok(opened)
}

/// Close the viewer's comic (an unpacked one's scratch folder goes with it).
#[tauri::command]
pub async fn close_comic(app: AppHandle) {
    let open = app.state::<Reader>().0.lock().unwrap_or_else(|e| e.into_inner()).take();
    // Removing an unpacked comic's folder can take a moment: not on this thread.
    tauri::async_runtime::spawn_blocking(move || drop(open));
}

/// Record the reader at `page` of `pages`: the book's being read, or at the
/// last page, read. Nothing for a calibre library, which is only read.
#[tauri::command]
pub async fn save_reading_progress(
    state: State<'_, AppState>,
    id: i64,
    page: usize,
    pages: usize,
) -> CmdResult<Option<ReadingState>> {
    let Some(native) = state.current().ok_or("No library is open")?.native.clone() else { return Ok(None) };
    let progress = if pages <= 1 { 1.0 } else { page as f64 / (pages - 1) as f64 };
    tauri::async_runtime::spawn_blocking(move || {
        native.lock().unwrap_or_else(|e| e.into_inner()).set_reading_progress(id, progress).map(Some)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

/// `page://localhost/<book>/<n>`: page n (from 0) of the open comic.
pub fn respond(app: &AppHandle, path: &str) -> Response<Vec<u8>> {
    let mut parts = path.trim_start_matches('/').split('/');
    let (Some(Ok(id)), Some(Ok(n))) = (parts.next().map(str::parse::<i64>), parts.next().map(str::parse::<usize>))
    else {
        return status(StatusCode::BAD_REQUEST);
    };
    let pages = match &*app.state::<Reader>().0.lock().unwrap_or_else(|e| e.into_inner()) {
        Some((open, pages)) if *open == id => pages.clone(),
        _ => return status(StatusCode::NOT_FOUND),
    };
    match pages.page(n) {
        Some((bytes, media_type)) => Response::builder()
            .header(header::CONTENT_TYPE, media_type)
            // The viewer asks with a per-opening query string, so a page is never one of an earlier file.
            .header(header::CACHE_CONTROL, "max-age=3600")
            .body(bytes)
            .unwrap_or_else(|_| status(StatusCode::INTERNAL_SERVER_ERROR)),
        None => status(StatusCode::NOT_FOUND),
    }
}

fn status(code: StatusCode) -> Response<Vec<u8>> {
    Response::builder().status(code).body(Vec::new()).expect("a bodiless response always builds")
}
