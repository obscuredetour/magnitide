//! calibre plugins (Settings › Plugins): installing, turning on and off, and
//! the host's log. The plugins run in the user's own calibre; see the
//! magnitide-plugins crate.

use std::path::PathBuf;

use magnitide_plugins::{Calibre, Installed, Plugins};
use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use ts_rs::TS;

use crate::commands::CmdResult;

/// What Settings › Plugins shows.
#[derive(Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct PluginsStatus {
    /// None: calibre isn't installed, so plugins can't run.
    calibre: Option<Calibre>,
    plugins: Vec<Installed>,
}

/// Plugins live in `<app config>/calibre-plugins`; calibre is found where
/// Magnitide finds it for converting.
pub fn create(app: &AppHandle) -> Plugins {
    let dir = app.path().app_config_dir().unwrap_or_else(|_| PathBuf::from(".")).join("calibre-plugins");
    Plugins::new(dir, || {
        magnitide_core::convert::Converter::detect().calibre().map(magnitide_plugins::host::calibre_debug_beside)
    })
}

async fn blocking<T: Send + 'static>(
    app: AppHandle,
    f: impl FnOnce(&Plugins) -> CmdResult<T> + Send + 'static,
) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(move || f(&app.state::<Plugins>())).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn plugins_status(app: AppHandle) -> CmdResult<PluginsStatus> {
    blocking(app, |p| Ok(PluginsStatus { calibre: p.calibre(), plugins: p.list() })).await
}

/// Install a calibre plugin zip (or update one already installed).
#[tauri::command]
pub async fn install_plugin(app: AppHandle, path: String) -> CmdResult<Installed> {
    blocking(app, move |p| p.install(std::path::Path::new(&path))).await
}

#[tauri::command]
pub async fn remove_plugin(app: AppHandle, name: String) -> CmdResult<Vec<Installed>> {
    blocking(app, move |p| p.remove(&name).map(|()| p.list())).await
}

#[tauri::command]
pub async fn set_plugin_enabled(app: AppHandle, name: String, enabled: bool) -> CmdResult<Vec<Installed>> {
    blocking(app, move |p| p.set_enabled(&name, enabled).map(|()| p.list())).await
}

/// The plugin host's recent output (what plugins printed, tracebacks).
#[tauri::command]
pub async fn plugin_log(state: State<'_, Plugins>) -> CmdResult<Vec<String>> {
    Ok(state.log())
}
