//! calibre plugins: installing, turning on and off, and the host's log
//! (Settings › Plugins), and running GUI plugins on the open library. The
//! plugins run in the user's own calibre; see the magnitide-plugins crate.
//!
//! GUI plugins work on a calibre-format shadow of the open Magnitide library
//! (`magnitide_plugins::shadow`), rebuilt whenever the library has changed
//! since. What a plugin changes there is applied to the library through
//! Magnitide's own edit path at sync points: when the plugin is done, and
//! when it hands over to Magnitide mid-run (calibre's Edit Metadata and
//! Remove Books are Magnitide's editor and delete). After a sync point,
//! further changes are measured from the library as it then is.

use std::collections::{BTreeSet, HashMap};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use calibre_compat::write::WriteOptions;
use magnitide_catalog::diff::Changes;
use magnitide_catalog::{BookId, Catalog};
use magnitide_core::external::ApplyReport;
use magnitide_plugins::shadow::{Shadow, baseline};
use magnitide_plugins::{Calibre, Installed, PluginMenus, Plugins};
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State};
use ts_rs::TS;

use crate::AppState;
use crate::commands::{CmdResult, write_library};

/// A GUI plugin finished; what it changed was applied. Matches `onPluginDone` in ui/src/api.ts.
pub const PLUGIN_DONE_EVENT: &str = "plugin-done";
/// A GUI plugin's status message or error. Matches `onPluginMessage`.
pub const PLUGIN_MESSAGE_EVENT: &str = "plugin-message";
/// A GUI plugin selected books. Matches `onPluginSelect`.
pub const PLUGIN_SELECT_EVENT: &str = "plugin-select";
/// A GUI plugin is waiting for Magnitide's editor or delete. Matches `onPluginAsk`.
pub const PLUGIN_ASK_EVENT: &str = "plugin-ask";

/// A plugin called calibre's Edit Metadata (`edit`) or Remove Books
/// (`remove`) for these books; it waits until `plugin_answer`.
#[derive(Clone, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct PluginAsk {
    plugin: String,
    token: u64,
    kind: String,
    ids: Vec<BookId>,
}

#[derive(Clone, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct PluginDone {
    plugin: String,
    /// None when it changed nothing.
    report: Option<ApplyReport>,
    /// Its changes couldn't be read back or applied.
    error: Option<String>,
    /// Conversions it asked for (calibre's Convert Books), for the app to run now.
    convert: Vec<PluginConversion>,
}

/// Books a plugin asked to have converted, and to what.
#[derive(Clone, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct PluginConversion {
    format: String,
    ids: Vec<BookId>,
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
    shadow: Shadow,
    /// The library's catalog it was written from, for knowing when to rebuild.
    fingerprint: u64,
    /// What a plugin's changes are measured against: the catalog it was
    /// written from, then the library as of the last sync point.
    baseline: Catalog,
    /// Books a plugin added that are in the library now: shadow id → library id.
    ids: HashMap<BookId, BookId>,
    /// What this run has applied so far (reported when the plugin is done).
    report: ApplyReport,
    /// Conversions this run asked for, by format, in shadow ids.
    conversions: Vec<(String, Vec<BookId>)>,
}

impl Built {
    fn to_library(&self, id: BookId) -> BookId {
        self.ids.get(&id).copied().unwrap_or(id)
    }

    fn to_shadow(&self, id: BookId) -> BookId {
        self.ids.iter().find(|&(_, &ours)| ours == id).map_or(id, |(&theirs, _)| theirs)
    }
}

/// What Settings › Plugins shows.
#[derive(Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct PluginsStatus {
    /// None: calibre isn't installed, so plugins can't run.
    calibre: Option<Calibre>,
    plugins: Vec<Installed>,
    /// What books are converted to when a plugin asks (calibre's output format).
    output_format: String,
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
                let convert = take_conversions(&app);
                let _ = app
                    .emit(PLUGIN_DONE_EVENT, PluginDone { plugin: plugin.unwrap_or_default(), report, error, convert });
            });
        }
        Some(kind @ ("gui-status" | "gui-error")) => {
            let message = text("message").unwrap_or_default();
            let _ = app.emit(PLUGIN_MESSAGE_EVENT, PluginMessage { plugin, message, error: kind == "gui-error" });
        }
        Some("gui-select") => {
            // Not on this thread (the host's reader): the lock it needs may be
            // held by a call waiting for the host's reply.
            let (app, ids) = (app.clone(), ids_of(&e));
            tauri::async_runtime::spawn_blocking(move || {
                let _ = app.emit(PLUGIN_SELECT_EVENT, to_library(&app, &ids));
            });
        }
        Some("gui-convert") => {
            // Run when the plugin is done, once the books it added are in the library.
            let (app, ids, format) = (app.clone(), ids_of(&e), text("format").unwrap_or_default());
            tauri::async_runtime::spawn_blocking(move || {
                let state = app.state::<PluginLibrary>();
                let mut built = state.0.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(b) = built.as_mut() {
                    b.conversions.push((format, ids));
                }
            });
        }
        Some("gui-ask") => {
            let app = app.clone();
            let token = e.get("token").and_then(Value::as_u64).unwrap_or_default();
            let kind = text("kind").unwrap_or_default();
            let ids = ids_of(&e);
            tauri::async_runtime::spawn(async move {
                // The library catches up first, so the books asked about (one
                // the plugin just made, say) are in it.
                match sync(&app).await {
                    Ok(()) => {
                        let ids = to_library(&app, &ids);
                        let plugin = plugin.unwrap_or_default();
                        let _ = app.emit(PLUGIN_ASK_EVENT, PluginAsk { plugin, token, kind, ids });
                    }
                    Err(err) => {
                        // Don't leave the plugin waiting.
                        let _ = app.state::<Plugins>().gui_resume(token);
                        let _ = app.emit(PLUGIN_MESSAGE_EVENT, PluginMessage { plugin, message: err, error: true });
                    }
                }
            });
        }
        _ => {}
    }
}

fn ids_of(e: &Value) -> Vec<BookId> {
    e.get("ids").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_i64).collect()).unwrap_or_default()
}

/// Shadow ids as the library's.
fn to_library(app: &AppHandle, ids: &[BookId]) -> Vec<BookId> {
    let state = app.state::<PluginLibrary>();
    let built = state.0.lock().unwrap_or_else(|e| e.into_inner());
    ids.iter().map(|&id| built.as_ref().map_or(id, |b| b.to_library(id))).collect()
}

/// The conversions the run asked for, as library ids, merged by format.
fn take_conversions(app: &AppHandle) -> Vec<PluginConversion> {
    let state = app.state::<PluginLibrary>();
    let mut built = state.0.lock().unwrap_or_else(|e| e.into_inner());
    let Some(b) = built.as_mut() else { return Vec::new() };
    let mut out: Vec<PluginConversion> = Vec::new();
    for (format, ids) in std::mem::take(&mut b.conversions) {
        let ids: Vec<BookId> = ids.into_iter().map(|id| b.to_library(id)).collect();
        match out.iter_mut().find(|c| c.format == format) {
            Some(c) => c.ids.extend(ids.into_iter().filter(|id| !c.ids.contains(id)).collect::<Vec<_>>()),
            None => out.push(PluginConversion { format, ids }),
        }
    }
    out
}

/// The plugin is done: catch up, and report everything this run applied.
async fn read_back(app: &AppHandle) -> CmdResult<Option<ApplyReport>> {
    sync(app).await?;
    let state = app.state::<PluginLibrary>();
    let mut built = state.0.lock().unwrap_or_else(|e| e.into_inner());
    // The library's catalog has reloaded if anything changed, so the shadow
    // is rebuilt before the next run.
    Ok(built.as_mut().map(|b| std::mem::take(&mut b.report)).filter(|r| !r.is_empty()))
}

/// A sync point: apply what the plugin changed since the last one, then
/// measure further changes from the library as it is now.
async fn sync(app: &AppHandle) -> CmdResult<()> {
    let handle = app.clone();
    let pending = tauri::async_runtime::spawn_blocking(move || -> CmdResult<Option<(Changes, Catalog)>> {
        let lib = handle.state::<AppState>().current().ok_or("No library is open")?;
        let state = handle.state::<PluginLibrary>();
        let built = state.0.lock().unwrap_or_else(|e| e.into_inner());
        // Another library was opened meanwhile: nothing to apply to.
        let Some(b) = built.as_ref().filter(|b| b.root == lib.root) else { return Ok(None) };
        let after = b.shadow.read()?;
        let mut changes = b.shadow.diff(&b.baseline, &after, &b.root)?;
        for book in &mut changes.books {
            book.id = b.to_library(book.id);
        }
        for id in &mut changes.removed {
            *id = b.to_library(*id);
        }
        Ok(Some((changes, after)))
    })
    .await
    .map_err(|e| e.to_string())??;
    let Some((changes, after)) = pending else { return Ok(()) };
    let report = if changes.is_empty() {
        ApplyReport::default()
    } else {
        write_library(&app.state::<AppState>(), move |lib| {
            lib.apply_changes(&changes, |path| trash::delete(path).map_err(std::io::Error::other))
        })
        .await?
    };
    let lib = app.state::<AppState>().current().ok_or("No library is open")?;
    let state = app.state::<PluginLibrary>();
    let mut built = state.0.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(b) = built.as_mut().filter(|b| b.root == lib.root) {
        b.ids.extend(report.added_from.iter().copied());
        b.report.absorb(report);
        b.baseline = baseline(&lib.catalog, &after, &b.ids);
    }
    Ok(())
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
        *built = Some(Built {
            root: lib.root.clone(),
            shadow,
            fingerprint,
            baseline: lib.catalog.clone(),
            ids: HashMap::new(),
            report: ApplyReport::default(),
            conversions: Vec::new(),
        });
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

/// Magnitide's editor closed (`edit`) or its delete finished (`remove`) for
/// the books a plugin asked about (`plugin-ask`): tell the plugin, which
/// goes on with the books as they are now in the library.
#[tauri::command]
pub async fn plugin_answer(app: AppHandle, token: u64, kind: String, ids: Vec<BookId>) -> CmdResult<()> {
    tauri::async_runtime::spawn_blocking(move || {
        let lib = app.state::<AppState>().current().ok_or("No library is open")?;
        let state = app.state::<PluginLibrary>();
        let plugins = app.state::<Plugins>();
        let built = state.0.lock().unwrap_or_else(|e| e.into_inner());
        let Some(b) = built.as_ref().filter(|b| b.root == lib.root) else {
            drop(built);
            return plugins.gui_resume(token);
        };
        let answer = if kind == "edit" {
            let books: serde_json::Map<String, Value> = ids
                .iter()
                .filter_map(|&id| lib.catalog.get(id))
                .map(|book| {
                    let mut v = serde_json::to_value(book).unwrap_or_default();
                    if let Some(cover) = book.cover_path(&lib.root).filter(|p| p.is_file()) {
                        v["coverPath"] = Value::from(cover.to_string_lossy().into_owned());
                    }
                    (b.to_shadow(book.id).to_string(), v)
                })
                .collect();
            serde_json::json!({ "books": books })
        } else {
            let removed: Vec<BookId> =
                ids.iter().filter(|&&id| lib.catalog.get(id).is_none()).map(|&id| b.to_shadow(id)).collect();
            serde_json::json!({ "removed": removed })
        };
        // Not holding the lock while the host works: its events may need it.
        drop(built);
        let answered = plugins.gui_answer(token, answer);
        // The shadow now matches what the user did: measure from here, before
        // the plugin goes on (and changes it again).
        let rebased = (|| -> CmdResult<()> {
            let mut built = state.0.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(b) = built.as_mut().filter(|b| b.root == lib.root) {
                let after = b.shadow.read()?;
                b.baseline = baseline(&lib.catalog, &after, &b.ids);
            }
            Ok(())
        })();
        plugins.gui_resume(token)?;
        answered.and(rebased)
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
    blocking(app, |p| Ok(PluginsStatus { calibre: p.calibre(), plugins: p.list(), output_format: p.output_format() }))
        .await
}

/// Set what books are converted to when a plugin asks.
#[tauri::command]
pub async fn set_plugin_output_format(app: AppHandle, format: String) -> CmdResult<()> {
    blocking(app, move |p| p.set_output_format(&format)).await
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
