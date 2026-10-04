//! Connected e-readers: a background watcher notices devices being mounted
//! and unmounted, scans them, and tells the UI (`devices-changed`). Which
//! library books are on a device is worked out on request, against whatever
//! library is open, so it follows library switches and edits.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use magnitide_core::devices::{self, DeviceKind, MatchedBy, Scan};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use ts_rs::TS;

use crate::AppState;
use crate::commands::CmdResult;

/// Event name matches `onDevicesChanged` in ui/src/api.ts.
pub const DEVICES_CHANGED_EVENT: &str = "devices-changed";

/// How often the mounted volumes are checked (a directory listing; cheap).
const POLL: Duration = Duration::from_secs(3);

/// A connected device and which library books are on it.
#[derive(Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    /// Its mount point; names it in commands.
    id: String,
    name: String,
    kind: DeviceKind,
    total_bytes: Option<u64>,
    free_bytes: Option<u64>,
    /// Book files on the device.
    files: usize,
    hits: Vec<DeviceHit>,
    /// Device files not matched to a library book ('/'-separated paths).
    unmatched: Vec<String>,
}

/// A library book found on a device.
#[derive(Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct DeviceHit {
    book_id: i64,
    /// Relative to the device root.
    path: String,
    format: String,
    matched_by: MatchedBy,
}

/// Watch for devices for as long as the app runs.
pub fn watch(app: AppHandle) {
    std::thread::Builder::new()
        .name("device-watcher".into())
        .spawn(move || {
            loop {
                update(&app);
                std::thread::sleep(POLL);
            }
        })
        .expect("spawning the device watcher");
}

/// Rescan if the set of connected devices changed.
fn update(app: &AppHandle) {
    let found = devices::connected();
    let state = app.state::<AppState>();
    let current = state.devices();
    let unchanged = found.len() == current.len() && found.iter().all(|d| current.iter().any(|s| s.device == *d));
    if unchanged {
        return;
    }
    let scans = found
        .into_iter()
        .map(|d| current.iter().find(|s| s.device == d).cloned().unwrap_or_else(|| Arc::new(devices::scan(&d))))
        .collect();
    state.set_devices(scans);
    let _ = app.emit(DEVICES_CHANGED_EVENT, ());
}

#[tauri::command]
pub async fn list_devices(state: State<'_, AppState>) -> CmdResult<Vec<DeviceInfo>> {
    let scans = state.devices();
    let lib = state.current();
    tauri::async_runtime::spawn_blocking(move || scans.iter().map(|s| info(s, lib.as_deref())).collect())
        .await
        .map_err(|e| e.to_string())
}

fn info(scan: &Scan, lib: Option<&crate::Loaded>) -> DeviceInfo {
    let matches = match lib {
        Some(lib) => devices::match_books(scan, &lib.catalog, &lib.root),
        None => vec![None; scan.files.len()],
    };
    let mut hits = Vec::new();
    let mut unmatched = Vec::new();
    for (f, m) in scan.files.iter().zip(matches) {
        match m {
            Some((book_id, matched_by)) => {
                hits.push(DeviceHit { book_id, path: f.path.clone(), format: f.format.clone(), matched_by })
            }
            None => unmatched.push(f.path.clone()),
        }
    }
    DeviceInfo {
        id: scan.device.root.display().to_string(),
        name: scan.device.name.clone(),
        kind: scan.device.kind,
        total_bytes: scan.space.map(|s| s.0),
        free_bytes: scan.space.map(|s| s.1),
        files: scan.files.len(),
        hits,
        unmatched,
    }
}

/// Scan a device again (after books were added or removed on it).
#[tauri::command]
pub async fn rescan_device(app: AppHandle, state: State<'_, AppState>, id: String) -> CmdResult<()> {
    let device = state.device(&id)?.device.clone();
    let scan = tauri::async_runtime::spawn_blocking(move || devices::scan(&device)).await.map_err(|e| e.to_string())?;
    let fresh = Arc::new(scan);
    let scans = state.devices().into_iter().map(|s| if s.device == fresh.device { fresh.clone() } else { s }).collect();
    state.set_devices(scans);
    let _ = app.emit(DEVICES_CHANGED_EVENT, ());
    Ok(())
}

/// Eject the device so it can be unplugged. It leaves the list straight away.
#[tauri::command]
pub async fn eject_device(app: AppHandle, state: State<'_, AppState>, id: String) -> CmdResult<()> {
    let root = state.device(&id)?.device.root.clone();
    let target = root.clone();
    tauri::async_runtime::spawn_blocking(move || devices::volumes::eject(&target))
        .await
        .map_err(|e| e.to_string())??;
    state.set_devices(state.devices().into_iter().filter(|s| s.device.root != root).collect());
    let _ = app.emit(DEVICES_CHANGED_EVENT, ());
    Ok(())
}

/// Open the device in the file manager.
#[tauri::command]
pub async fn reveal_device(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    let root = state.device(&id)?.device.root.clone();
    tauri::async_runtime::spawn_blocking(move || open::that(Path::new(&root)).map_err(|e| e.to_string()))
        .await
        .map_err(|e| e.to_string())?
}
