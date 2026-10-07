//! calibre plugins: installing, turning on and off, and the host's log
//! (Settings › Plugins), and running GUI plugins on the open library. The
//! plugins run in the user's own calibre; see the magnitide-plugins crate.
//!
//! GUI plugins work on a calibre-format shadow of the open Magnitide library
//! (`magnitide_plugins::shadow`), rebuilt whenever the library has changed
//! since. When a plugin is done, what it changed there is applied to the
//! library through Magnitide's own edit path.

use std::collections::{BTreeSet, HashMap};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use calibre_compat::write::WriteOptions;
use magnitide_catalog::Catalog;
use magnitide_core::external::ApplyReport;
use magnitide_plugins::shadow::Shadow;
use magnitide_plugins::{Calibre, Installed, PluginMenus, Plugins};
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State};
use ts_rs::TS;

use crate::commands::{CmdResult, write_library};
use crate::{AppState, Loaded};

/// A GUI plugin finished; what it changed was applied. Matches `onPluginDone` in ui/src/api.ts.
pub const PLUGIN_DONE_EVENT: &str = "plugin-done";
/// A GUI plugin's status message or error. Matches `onPluginMessage`.
pub const PLUGIN_MESSAGE_EVENT: &str = "plugin-message";
/// A GUI plugin selected books. Matches `onPluginSelect`.
pub const PLUGIN_SELECT_EVENT: &str = "plugin-select";

#[derive(Clone, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct PluginDone {
    plugin: String,
    /// None when it changed nothing.
    report: Option<ApplyReport>,
    /// Its changes couldn't be read back or applied.
    error: Option<String>,
}

#[derive(Clone, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct PluginMessage {
    plugin: Option<String>,
    message: String,
    error: bool,
}

/// The shadow library GUI plugins work on, and what it was written from.
#[derive(Default)]
pub struct PluginLibrary(Mutex<Option<Built>>);

struct Built {
    root: PathBuf,
    /// The library as it was written from (its catalog): a plugin's changes
    /// are measured against it.
    before: Arc<Loaded>,
    fingerprint: u64,
    shadow: Shadow,
}

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
    let plugins = Plugins::new(dir, || {
        magnitide_core::convert::Converter::detect().calibre().map(magnitide_plugins::host::calibre_debug_beside)
    });
    let handle = app.clone();
    plugins.set_event_handler(Arc::new(move |e| on_event(&handle, e)));
    app.manage(PluginLibrary::default());
    plugins
}

fn on_event(app: &AppHandle, e: Value) {
    let text = |k: &str| e.get(k).and_then(Value::as_str).map(str::to_owned);
    let plugin = text("plugin");
    match e.get("event").and_then(Value::as_str) {
        Some("gui-idle") => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let (report, error) = match read_back(&app).await {
                    Ok(report) => (report, None),
                    Err(e) => (None, Some(e)),
                };
                let _ = app.emit(PLUGIN_DONE_EVENT, PluginDone { plugin: plugin.unwrap_or_default(), report, error });
            });
        }
        Some(kind @ ("gui-status" | "gui-error")) => {
            let message = text("message").unwrap_or_default();
            let _ = app.emit(PLUGIN_MESSAGE_EVENT, PluginMessage { plugin, message, error: kind == "gui-error" });
        }
        Some("gui-select") => {
            let ids: Vec<i64> = e
                .get("ids")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_i64).collect())
                .unwrap_or_default();
            let _ = app.emit(PLUGIN_SELECT_EVENT, ids);
        }
        _ => {}
    }
}

/// Apply what the plugin changed in the shadow to the library. None if nothing.
async fn read_back(app: &AppHandle) -> CmdResult<Option<ApplyReport>> {
    let handle = app.clone();
    let changes = tauri::async_runtime::spawn_blocking(move || {
        let lib = handle.state::<AppState>().current().ok_or("No library is open")?;
        let built = handle.state::<PluginLibrary>();
        let built = built.0.lock().unwrap_or_else(|e| e.into_inner());
        match built.as_ref() {
            // The library changed (another one was opened) meanwhile: nothing to apply to.
            Some(b) if b.root == lib.root => b.shadow.changes(&b.before.catalog, &b.root).map(Some),
            _ => Ok(None),
        }
    })
    .await
    .map_err(|e| e.to_string())??;
    let Some(changes) = changes.filter(|c| !c.is_empty()) else { return Ok(None) };
    // The library's catalog reloads, so the shadow is rebuilt before the next run.
    let report = write_library(&app.state::<AppState>(), move |lib| {
        lib.apply_changes(&changes, |path| trash::delete(path).map_err(std::io::Error::other))
    })
    .await?;
    Ok(Some(report))
}

/// Changes whenever a book's metadata does, or the set of books or fields.
fn fingerprint(catalog: &Catalog) -> u64 {
    let mut h = DefaultHasher::new();
    for b in &catalog.books {
        (b.id, b.last_modified).hash(&mut h);
    }
    for c in &catalog.custom_columns {
        (c.id, &c.label).hash(&mut h);
    }
    h.finish()
}

/// Make the shadow match the open library (rebuilding it if the library
/// changed since, unless `reuse` and one exists for it) and open it in the
/// host. Returns the GUI plugins' menus.
fn prepare(app: &AppHandle, reuse: bool) -> CmdResult<PluginMenus> {
    let lib = app.state::<AppState>().current().ok_or("No library is open")?;
    let native = lib.native.clone().ok_or(
        "calibre plugins work on Magnitide libraries. Import this calibre library into one (library menu) to use them.",
    )?;
    let plugins = app.state::<Plugins>();
    let state = app.state::<PluginLibrary>();
    let mut built = state.0.lock().unwrap_or_else(|e| e.into_inner());
    let fingerprint = fingerprint(&lib.catalog);
    let same_library = built.as_ref().is_some_and(|b| b.root == lib.root);
    let current = same_library && (reuse || built.as_ref().is_some_and(|b| b.fingerprint == fingerprint));
    if !current {
        if !plugins.gui_close()? {
            // A plugin is still working on the shadow: leave it be.
            return match built.as_ref().filter(|_| same_library) {
                Some(b) => plugins.gui_open(&b.shadow.library()),
                None => Err("A plugin is still working. Finish in its window first.".into()),
            };
        }
        let key = lib.catalog.library_uuid.clone().unwrap_or_else(|| "library".into());
        let dir = app.path().app_cache_dir().map_err(|e| e.to_string())?.join("plugin-libraries").join(key);
        let names: Vec<String> = lib
            .catalog
            .books
            .iter()
            .flat_map(|b| b.authors.iter().cloned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let author_sorts: HashMap<String, String> = native
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .author_sorts(&names)
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|a| (a.name, a.sort))
            .collect();
        let shadow = Shadow::new(dir);
        shadow.build(&lib.catalog, &lib.root, &WriteOptions { author_sorts })?;
        *built = Some(Built { root: lib.root.clone(), before: lib.clone(), fingerprint, shadow });
    }
    let library = built.as_ref().map(|b| b.shadow.library()).ok_or("no plugin library")?;
    plugins.gui_open(&library)
}

/// The names of the GUI plugins that are on (without starting calibre).
#[tauri::command]
pub async fn gui_plugin_names(state: State<'_, Plugins>) -> CmdResult<Vec<String>> {
    Ok(state.gui_plugins().into_iter().map(|p| p.name).collect())
}

/// The GUI plugins' menus, for the open library. Empty when none are on
/// (calibre isn't started for nothing).
#[tauri::command]
pub async fn plugin_menus(app: AppHandle) -> CmdResult<PluginMenus> {
    tauri::async_runtime::spawn_blocking(move || {
        if app.state::<Plugins>().gui_plugins().is_empty() {
            return Ok(PluginMenus::default());
        }
        prepare(&app, true)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Run a GUI plugin's menu item (`path` and `texts` from its menu; empty for
/// its toolbar button) on the selected books. Returns once it has started;
/// `plugin-done` follows when it's finished and its changes are applied.
#[tauri::command]
pub async fn run_plugin(
    app: AppHandle,
    plugin: String,
    path: Vec<u32>,
    texts: Vec<String>,
    selection: Vec<i64>,
) -> CmdResult<()> {
    tauri::async_runtime::spawn_blocking(move || {
        prepare(&app, false)?;
        app.state::<Plugins>().gui_trigger(&plugin, &path, &texts, &selection)
    })
    .await
    .map_err(|e| e.to_string())?
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
