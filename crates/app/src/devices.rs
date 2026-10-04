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
use crate::commands::{CmdResult, ImportProgress, write_library};

/// Event name matches `onDevicesChanged` in ui/src/api.ts.
pub const DEVICES_CHANGED_EVENT: &str = "devices-changed";

/// Sent while books are copied to a device. Matches `onTransferProgress` in ui/src/api.ts.
pub const TRANSFER_PROGRESS_EVENT: &str = "transfer-progress";

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
    /// Formats it reads, best first.
    formats: Vec<String>,
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
        formats: devices::transfer::readable(scan.device.kind).iter().map(|f| (*f).to_owned()).collect(),
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

/// Rescan one device after changing it, and tell the UI.
fn rescanned(app: &AppHandle, root: &Path) {
    let state = app.state::<AppState>();
    let scans = state
        .devices()
        .into_iter()
        .map(|s| if s.device.root == root { Arc::new(devices::scan(&s.device)) } else { s })
        .collect();
    state.set_devices(scans);
    let _ = app.emit(DEVICES_CHANGED_EVENT, ());
}

/// Copy books from the open library to a device (in the format it reads
/// best; EPUBs get the library's metadata written into the copy).
#[tauri::command]
pub async fn send_to_device(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    book_ids: Vec<i64>,
    format: Option<String>,
) -> CmdResult<devices::transfer::SendReport> {
    let scan = state.device(&id)?;
    let lib = state.current().ok_or("No library is open")?;
    // Books already on the device aren't sent again.
    let already: std::collections::HashSet<i64> =
        devices::match_books(&scan, &lib.catalog, &lib.root).into_iter().flatten().map(|(b, _)| b).collect();
    let app2 = app.clone();
    let report = tauri::async_runtime::spawn_blocking(move || {
        let converter = magnitide_core::convert::Converter::detect();
        let books: Vec<&magnitide_catalog::Book> =
            book_ids.iter().filter(|b| !already.contains(b)).filter_map(|b| lib.catalog.get(*b)).collect();
        let free = devices::volumes::space(&scan.device.root).map(|s| s.1);
        let report = devices::transfer::send(
            &scan.device,
            &books,
            &lib.root,
            free,
            format.as_deref(),
            &converter,
            |done, total, title| {
                let _ = app2.emit(TRANSFER_PROGRESS_EVENT, ImportProgress { done, total, title: title.to_owned() });
            },
        );
        rescanned(&app2, &scan.device.root);
        report
    })
    .await
    .map_err(|e| e.to_string())?;
    Ok(report)
}

/// Delete books' files from a device (and KOReader's notes for them).
#[tauri::command]
pub async fn remove_from_device(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    book_ids: Vec<i64>,
) -> CmdResult<devices::transfer::RemoveReport> {
    let scan = state.device(&id)?;
    let lib = state.current().ok_or("No library is open")?;
    tauri::async_runtime::spawn_blocking(move || {
        let matches = devices::match_books(&scan, &lib.catalog, &lib.root);
        let paths: Vec<String> = scan
            .files
            .iter()
            .zip(matches)
            .filter(|(_, m)| m.is_some_and(|(b, _)| book_ids.contains(&b)))
            .map(|(f, _)| f.path.clone())
            .collect();
        let report = devices::transfer::remove(&scan.device, &paths);
        rescanned(&app, &scan.device.root);
        report
    })
    .await
    .map_err(|e| e.to_string())
}

/// Add device files that aren't in the library (paths as listed in
/// `DeviceInfo::unmatched`) to the open Magnitide library.
#[tauri::command]
pub async fn import_from_device(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    paths: Vec<String>,
) -> CmdResult<magnitide_core::import::AddReport> {
    let scan = state.device(&id)?;
    // Only files the scan found, so nothing outside the device is read.
    let files: Vec<std::path::PathBuf> =
        scan.files.iter().filter(|f| paths.contains(&f.path)).map(|f| scan.device.root.join(&f.path)).collect();
    let report = write_library(&state, move |lib| magnitide_core::import::add_files(lib, &files, |_| {})).await?;
    let root = scan.device.root.clone();
    tauri::async_runtime::spawn_blocking(move || rescanned(&app, &root)).await.map_err(|e| e.to_string())?;
    Ok(report)
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
