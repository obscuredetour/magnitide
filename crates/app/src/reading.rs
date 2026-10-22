//! Reading positions synced from e-readers (KOReader's progress sync, served
//! by the library server: see server.rs and the server crate's kosync),
//! matched to the open library's books and shown across it. A position is
//! matched the moment a reader syncs it, and every stored one is tried again
//! when a library opens, for positions synced while another was.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use magnitide_core::pages::ReadingState;
use magnitide_core::positions::SyncedPosition;
use magnitide_server::kosync::{Position, SyncStore};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use ts_rs::TS;

use crate::AppState;
use crate::commands::CmdResult;

/// Books whose reading state just moved (payload: `Vec<BookReading>`).
/// Matches `onReadingChanged` in api.ts.
pub const READING_CHANGED_EVENT: &str = "reading-changed";

/// A book's reading state, for the views.
#[derive(Serialize, Clone, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct BookReading {
    pub id: i64,
    #[serde(flatten)]
    #[ts(flatten)]
    pub state: ReadingState,
}

/// The positions database: the app's, not any library's.
pub fn store_path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join("reading-positions.sqlite"))
}

fn synced(p: &Position) -> SyncedPosition {
    SyncedPosition {
        document: p.document.clone(),
        progress: p.percentage,
        at: DateTime::<Utc>::from_timestamp(p.timestamp, 0).unwrap_or_else(Utc::now),
        device: Some(p.device.trim().to_owned()).filter(|d| !d.is_empty()),
    }
}

/// Match `positions` to the open library's books and tell the UI what moved.
/// Blocking: call off the main thread.
fn apply(app: &AppHandle, positions: &[SyncedPosition]) {
    let Some(native) = app.state::<AppState>().current().and_then(|l| l.native.clone()) else { return };
    let applied = native.lock().unwrap_or_else(|e| e.into_inner()).apply_positions(positions);
    match applied {
        Ok(applied) if !applied.is_empty() => {
            let moved: Vec<BookReading> =
                applied.into_iter().map(|a| BookReading { id: a.book, state: a.state }).collect();
            let _ = app.emit(READING_CHANGED_EVENT, moved);
        }
        Ok(_) => {}
        Err(e) => eprintln!("matching reading positions to the library: {e}"),
    }
}

/// A reader just synced a position (on the server's thread).
pub fn position_synced(app: &AppHandle, position: &Position) {
    apply(app, &[synced(position)]);
}

/// Every stored position against the library just opened. Blocking.
pub fn catch_up(app: &AppHandle) {
    let Some(path) = store_path(app).filter(|p| p.is_file()) else { return };
    let entries = SyncStore::open(&path).and_then(|s| s.entries().map_err(|e| e.to_string()));
    match entries {
        Ok(entries) => {
            let positions: Vec<SyncedPosition> = entries.iter().map(|e| synced(&e.position)).collect();
            apply(app, &positions);
        }
        Err(e) => eprintln!("reading synced positions: {e}"),
    }
}

/// The reading state of every book in the open library that has one.
#[tauri::command]
pub async fn reading_states(state: State<'_, AppState>) -> CmdResult<Vec<BookReading>> {
    let Some(native) = state.current().and_then(|l| l.native.clone()) else { return Ok(Vec::new()) };
    tauri::async_runtime::spawn_blocking(move || {
        let states = native.lock().unwrap_or_else(|e| e.into_inner()).reading_states()?;
        Ok(states.into_iter().map(|(id, state)| BookReading { id, state }).collect())
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e: magnitide_core::Error| e.to_string())
}
