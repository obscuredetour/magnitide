//! Plugins, of both kinds. Magnitide's own (WebAssembly components on the
//! `magnitide:plugin` API, see the magnitide-plugins-wasm crate and
//! docs/plugin-api.md): installing `.mgp` packages, turning them on and
//! off, their logs; they're asked in Find Metadata (lookup.rs).
//!
//! calibre plugins: installing, turning on and off, and the host's log
//! (Settings › Plugins), running GUI plugins on the open library, and the
//! other kinds as Magnitide's hooks: file-type plugins when books are added
//! (and after, on the shadow), metadata readers and writers, a plugin's
//! own settings, and library-closed plugins. The plugins run in Magnitide's
//! plugin runtime; see the magnitide-plugins crate.
//!
//! GUI plugins work on a calibre-format shadow of the open Magnitide library
//! (`magnitide_plugins::shadow`), rebuilt whenever the library has changed
//! since. What a plugin changes there is applied to the library through
//! Magnitide's own edit path at sync points: when the plugin is done, and
//! when it hands over to Magnitide mid-run (calibre's Edit Metadata and
//! Remove Books are Magnitide's editor and delete). After a sync point,
//! further changes are measured from the library as it then is.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use calibre_compat::write::WriteOptions;
use magnitide_catalog::diff::Changes;
use magnitide_catalog::{BookId, Catalog};
use magnitide_core::external::ApplyReport;
use magnitide_core::hooks::{ImportHooks, ImportOutcome, MetadataPlugins, Precedence};
use magnitide_core::metadata::BookMetadata;
use magnitide_plugins::shadow::{Shadow, baseline};
use magnitide_plugins::{Configured, Hook, HostChoice, Installed, PluginMenus, Plugins, Progress, RuntimeStatus};
use magnitide_plugins_wasm::{Installed as NativePlugin, WasmPlugins};
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
/// A GUI plugin is waiting for Magnitide's editor, delete, Find Metadata or
/// conversion. Matches `onPluginAsk`.
pub const PLUGIN_ASK_EVENT: &str = "plugin-ask";
/// A GUI plugin searched the library. Matches `onPluginSearch`.
pub const PLUGIN_SEARCH_EVENT: &str = "plugin-search";
/// A GUI plugin opened a book (calibre's View). Matches `onPluginOpenBook`.
pub const PLUGIN_OPEN_BOOK_EVENT: &str = "plugin-open-book";
/// A job a GUI plugin started in the host: started, progressed or done. Matches `onPluginJob`.
pub const PLUGIN_JOB_EVENT: &str = "plugin-job";

/// A plugin called calibre's Edit Metadata (`edit`), Remove Books
/// (`remove`), metadata download (`download`) or Convert Books (`convert`)
/// for these books; it waits until `plugin_answer`.
#[derive(Clone, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct PluginAsk {
    plugin: String,
    token: u64,
    kind: String,
    ids: Vec<BookId>,
}

/// A plugin asked to open a book (`format` when it named one).
#[derive(Clone, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct PluginOpenBook {
    plugin: Option<String>,
    id: BookId,
    format: Option<String>,
}

/// A job in the host's job manager (calibre's `gui.job_manager`), as it goes.
#[derive(Clone, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct PluginJob {
    plugin: Option<String>,
    /// The host's id for the job.
    job: u64,
    description: String,
    /// "start", "progress" or "done".
    phase: String,
    percent: Option<u32>,
    message: Option<String>,
    failed: bool,
    /// The job's log, when done.
    details: Option<String>,
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
    /// The plugin runtime (plugins can't run until it's installed).
    runtime: RuntimeStatus,
    plugins: Vec<Installed>,
    /// What books are converted to when a plugin asks (calibre's output format).
    output_format: String,
    /// Each plugin's latest error, by name.
    errors: BTreeMap<String, String>,
    /// Magnitide's own plugins.
    native: Vec<NativePlugin>,
    /// Their latest errors, by name.
    native_errors: BTreeMap<String, String>,
}

/// Both plugin runtimes as core's extension points: hooks on books being
/// added, and metadata readers and writers. Magnitide's own plugins go
/// first (they're in-process and quick); calibre plugins see what they
/// made of a file.
pub struct AppHooks(pub AppHandle);

impl AppHooks {
    fn native(&self) -> State<'_, WasmPlugins> {
        self.0.state::<WasmPlugins>()
    }

    fn calibre(&self) -> State<'_, Plugins> {
        self.0.state::<Plugins>()
    }
}

impl ImportHooks for AppHooks {
    fn file_types(&self) -> Vec<String> {
        let mut types = ImportHooks::file_types(&*self.native());
        types.extend(ImportHooks::file_types(&*self.calibre()));
        types.sort();
        types.dedup();
        types
    }

    fn on_import(&self, path: &Path, format: &str) -> ImportOutcome {
        let first = ImportHooks::on_import(&*self.native(), path, format);
        let (current, format_now, failed) = match &first {
            ImportOutcome::Replaced { path: p, .. } => {
                let f = p.extension().and_then(|e| e.to_str()).map_or(format.to_owned(), str::to_uppercase);
                (p.clone(), f, None)
            }
            ImportOutcome::Unchanged => (path.to_path_buf(), format.to_owned(), None),
            ImportOutcome::Failed(why) => (path.to_path_buf(), format.to_owned(), Some(why.clone())),
        };
        match ImportHooks::on_import(&*self.calibre(), &current, &format_now) {
            ImportOutcome::Unchanged => first,
            ImportOutcome::Replaced { path: new, temporary } => {
                if let ImportOutcome::Replaced { path: ours, temporary: true } = &first {
                    let _ = std::fs::remove_file(ours);
                }
                ImportOutcome::Replaced { path: new, temporary }
            }
            ImportOutcome::Failed(why) => match (first, failed) {
                (replaced @ ImportOutcome::Replaced { .. }, _) => replaced,
                (_, Some(earlier)) => ImportOutcome::Failed(format!("{earlier}; {why}")),
                _ => ImportOutcome::Failed(why),
            },
        }
    }
}

impl MetadataPlugins for AppHooks {
    fn reads(&self, format: &str) -> Option<Precedence> {
        match (MetadataPlugins::reads(&*self.native(), format), MetadataPlugins::reads(&*self.calibre(), format)) {
            (Some(Precedence::Override), _) | (_, Some(Precedence::Override)) => Some(Precedence::Override),
            (Some(p), _) | (None, Some(p)) => Some(p),
            (None, None) => None,
        }
    }

    fn read(&self, path: &Path, format: &str) -> Result<BookMetadata, String> {
        let native = self.native();
        let calibre = self.calibre();
        let mut errors = Vec::new();
        for (reads, read) in [
            (MetadataPlugins::reads(&*native, format), &*native as &dyn MetadataPlugins),
            (MetadataPlugins::reads(&*calibre, format), &*calibre as &dyn MetadataPlugins),
        ] {
            if reads.is_some() {
                match read.read(path, format) {
                    Ok(meta) => return Ok(meta),
                    Err(e) => errors.push(e),
                }
            }
        }
        Err(if errors.is_empty() { format!("no plugin reads {format}") } else { errors.join("; ") })
    }

    fn writes(&self, format: &str) -> bool {
        MetadataPlugins::writes(&*self.native(), format) || MetadataPlugins::writes(&*self.calibre(), format)
    }

    fn write(&self, path: &Path, format: &str, meta: &BookMetadata) -> Result<(), String> {
        let native = self.native();
        let calibre = self.calibre();
        let mut any = false;
        if MetadataPlugins::writes(&*native, format) {
            MetadataPlugins::write(&*native, path, format, meta)?;
            any = true;
        }
        if MetadataPlugins::writes(&*calibre, format) {
            MetadataPlugins::write(&*calibre, path, format, meta)?;
            any = true;
        }
        if any { Ok(()) } else { Err(format!("no plugin writes {format}")) }
    }
}

/// Run file-type plugins' post hooks (`postimport` and `postadd` after
/// adding, `postconvert`, `postdelete`) for these books and their formats
/// concerned, on the shadow library, and apply what they changed. Nothing
/// happens (and no host starts) unless an enabled plugin wants the hook.
pub async fn post_hooks(app: &AppHandle, hook: Hook, items: Vec<(BookId, Vec<String>)>) {
    let items: Vec<(BookId, Vec<String>)> = {
        let plugins = app.state::<Plugins>();
        items.into_iter().filter(|(_, formats)| formats.iter().any(|f| plugins.wants_post(hook, f))).collect()
    };
    if items.is_empty() {
        return;
    }
    let handle = app.clone();
    let ran = tauri::async_runtime::spawn_blocking(move || -> CmdResult<Vec<String>> {
        let library = ensure_shadow(&handle, false)?;
        let plugins = handle.state::<Plugins>();
        let mut ran: Vec<String> = Vec::new();
        let mut errors = Vec::new();
        for (id, formats) in &items {
            match plugins.run_post(hook, &library, *id, formats) {
                Ok(names) => ran.extend(names.into_iter().filter(|n| !ran.contains(n)).collect::<Vec<_>>()),
                Err(e) => errors.push(e),
            }
        }
        match errors.first() {
            Some(e) if ran.is_empty() => Err(e.clone()),
            _ => Ok(ran),
        }
    })
    .await
    .map_err(|e| e.to_string())
    .and_then(|r| r);
    match ran {
        Ok(ran) if ran.is_empty() => {}
        Ok(ran) => {
            let (report, error) = match read_back(app).await {
                Ok(report) => (report, None),
                Err(e) => (None, Some(e)),
            };
            if report.is_some() || error.is_some() {
                let plugin = ran.join(", ");
                let _ = app.emit(PLUGIN_DONE_EVENT, PluginDone { plugin, report, error, convert: Vec::new() });
            }
        }
        Err(message) => {
            let _ = app.emit(PLUGIN_MESSAGE_EVENT, PluginMessage { plugin: None, message, error: true });
        }
    }
}

/// The open library is closing (another is opened, or the app quits):
/// library-closed plugins run, if a host is up, and the shadow is closed.
pub fn library_closing(app: &AppHandle) {
    let plugins = app.state::<Plugins>();
    let library = {
        let state = app.state::<PluginLibrary>();
        let built = state.0.lock().unwrap_or_else(|e| e.into_inner());
        built.as_ref().map(|b| b.shadow.library())
    };
    let Some(library) = library else { return };
    if let Err(e) = plugins.library_closed(&library) {
        let _ = app.emit(PLUGIN_MESSAGE_EVENT, PluginMessage { plugin: None, message: e, error: true });
    }
    let _ = plugins.gui_close();
}

/// Plugins live in `<app config>/calibre-plugins`; the runtime in
/// `<app local data>/plugin-runtime` (it's large, and not configuration).
pub fn create(app: &AppHandle) -> Plugins {
    let dir = app.path().app_config_dir().unwrap_or_else(|_| PathBuf::from(".")).join("calibre-plugins");
    let runtime = app.path().app_local_data_dir().unwrap_or_else(|_| PathBuf::from(".")).join("plugin-runtime");
    let plugins = Plugins::new(dir, runtime);
    let handle = app.clone();
    plugins.set_event_handler(Arc::new(move |e| on_event(&handle, e)));
    app.manage(PluginLibrary::default());
    app.manage(RuntimeInstall::default());
    plugins
}

/// Magnitide's own plugins, from `<app config>/plugins`. A folder that
/// can't be read or made still gives a host, with no plugins and the
/// problem as the error of the folder itself.
pub fn create_native(app: &AppHandle) -> WasmPlugins {
    let dir = app.path().app_config_dir().unwrap_or_else(|_| PathBuf::from(".")).join("plugins");
    match WasmPlugins::new(dir.clone()) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("plugins: {e}");
            WasmPlugins::new(std::env::temp_dir().join("magnitide-plugins")).unwrap_or_else(|e| panic!("{e}"))
        }
    }
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
        Some("gui-search") => {
            let _ = app.emit(PLUGIN_SEARCH_EVENT, text("text").unwrap_or_default());
        }
        Some("gui-open-book") => {
            let (app, id, format) = (app.clone(), e.get("id").and_then(Value::as_i64), text("format"));
            let Some(id) = id else { return };
            tauri::async_runtime::spawn_blocking(move || {
                let id = to_library(&app, &[id])[0];
                let _ = app.emit(PLUGIN_OPEN_BOOK_EVENT, PluginOpenBook { plugin, id, format });
            });
        }
        Some(phase @ ("job-start" | "job-progress" | "job-done")) => {
            let job = PluginJob {
                plugin,
                job: e.get("job").and_then(Value::as_u64).unwrap_or_default(),
                description: text("description").unwrap_or_default(),
                phase: phase.trim_start_matches("job-").to_owned(),
                percent: e.get("percent").and_then(Value::as_u64).map(|p| p.min(100) as u32),
                message: text("message"),
                failed: e.get("failed").and_then(Value::as_bool).unwrap_or(false),
                details: text("details"),
            };
            let _ = app.emit(PLUGIN_JOB_EVENT, job);
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
    let library = ensure_shadow(app, reuse)?;
    app.state::<Plugins>().gui_open(&library)
}

/// Make the shadow match the open library (rebuilding it if the library
/// changed since, unless `reuse` and one exists for it). Returns its path.
/// While a plugin is still working on it, it's left as it is.
fn ensure_shadow(app: &AppHandle, reuse: bool) -> CmdResult<PathBuf> {
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
                Some(b) => Ok(b.shadow.library()),
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
    built.as_ref().map(|b| b.shadow.library()).ok_or_else(|| "no plugin library".into())
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
/// its toolbar button) on the selected books, with the library's current
/// search. Returns once it has started; `plugin-done` follows when it's
/// finished and its changes are applied.
#[tauri::command]
pub async fn run_plugin(
    app: AppHandle,
    plugin: String,
    path: Vec<u32>,
    texts: Vec<String>,
    selection: Vec<i64>,
    search: Option<String>,
) -> CmdResult<()> {
    tauri::async_runtime::spawn_blocking(move || {
        prepare(&app, false)?;
        app.state::<Plugins>().gui_trigger(&plugin, &path, &texts, &selection, search.as_deref().unwrap_or(""))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Magnitide's editor closed (`edit`), its delete finished (`remove`), Find
/// Metadata finished (`download`) or its conversion finished (`convert`)
/// for the books a plugin asked about (`plugin-ask`): tell the plugin,
/// which goes on with the books as they are now in the library.
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
        let answer = if kind == "edit" || kind == "download" {
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
        } else if kind == "convert" {
            // Every format the books have now; the host adds the ones the shadow lacks.
            let formats: serde_json::Map<String, Value> = ids
                .iter()
                .filter_map(|&id| lib.catalog.get(id))
                .map(|book| {
                    let files: Vec<Value> = book
                        .formats
                        .iter()
                        .map(|f| serde_json::json!({ "format": f.format, "path": book.format_path(&lib.root, f) }))
                        .collect();
                    (b.to_shadow(book.id).to_string(), Value::Array(files))
                })
                .collect();
            serde_json::json!({ "formats": formats })
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
    let app2 = app.clone();
    blocking(app, move |p| {
        let native = app2.state::<WasmPlugins>();
        Ok(PluginsStatus {
            runtime: p.runtime_status(),
            plugins: p.list(),
            output_format: p.output_format(),
            errors: p.errors(),
            native: native.list(),
            native_errors: native.errors(),
        })
    })
    .await
}

/// Install a Magnitide plugin (`.mgp`), or update one already installed.
#[tauri::command]
pub async fn install_native_plugin(app: AppHandle, path: String) -> CmdResult<NativePlugin> {
    tauri::async_runtime::spawn_blocking(move || app.state::<WasmPlugins>().install(Path::new(&path)))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remove_native_plugin(app: AppHandle, name: String) -> CmdResult<Vec<NativePlugin>> {
    let plugins = app.state::<WasmPlugins>();
    plugins.remove(&name)?;
    Ok(plugins.list())
}

#[tauri::command]
pub async fn set_native_plugin_enabled(app: AppHandle, name: String, enabled: bool) -> CmdResult<Vec<NativePlugin>> {
    let plugins = app.state::<WasmPlugins>();
    plugins.set_enabled(&name, enabled)?;
    Ok(plugins.list())
}

/// What a Magnitide plugin logged in its recent calls, oldest first.
#[tauri::command]
pub async fn native_plugin_log(app: AppHandle, name: String) -> CmdResult<Vec<String>> {
    Ok(app.state::<WasmPlugins>().log(&name))
}

/// Open a plugin's own settings (its dialog, in the host) and wait for it
/// to close.
#[tauri::command]
pub async fn configure_plugin(app: AppHandle, name: String) -> CmdResult<Configured> {
    tauri::async_runtime::spawn_blocking(move || {
        // A GUI plugin's settings want the plugin loaded in the host's
        // window with the library open, as in calibre; without a Magnitide
        // library the host makes do with its window alone.
        let _ = prepare(&app, true);
        app.state::<Plugins>().configure(&name)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Progress of the runtime's install. Matches `onPluginRuntimeProgress` in ui/src/api.ts.
pub const PLUGIN_RUNTIME_PROGRESS_EVENT: &str = "plugin-runtime-progress";

/// Whether an install is running, and how to stop it.
#[derive(Default)]
pub struct RuntimeInstall(Arc<std::sync::atomic::AtomicBool>);

/// The runtime's state, checked by running it (so a missing system library shows).
#[tauri::command]
pub async fn plugin_runtime_status(app: AppHandle) -> CmdResult<RuntimeStatus> {
    blocking(app, |p| Ok(p.check_runtime())).await
}

/// Download and install the plugin runtime, with `plugin-runtime-progress`
/// events meanwhile. Returns its state once installed.
#[tauri::command]
pub async fn install_plugin_runtime(app: AppHandle) -> CmdResult<RuntimeStatus> {
    install_runtime_with(app, |plugins, cancel, report| plugins.install_runtime(cancel, report)).await
}

/// Install the plugin runtime from an archive on disk: the published one
/// for this computer, downloaded by hand (checked by its hash).
#[tauri::command]
pub async fn install_plugin_runtime_from_file(app: AppHandle, path: String) -> CmdResult<RuntimeStatus> {
    let archive = PathBuf::from(path);
    install_runtime_with(app, move |plugins, cancel, report| {
        plugins.install_runtime_from_file(&archive, cancel, report)
    })
    .await
}

async fn install_runtime_with(
    app: AppHandle,
    install: impl FnOnce(&Plugins, &AtomicBool, &mut dyn FnMut(Progress)) -> Result<RuntimeStatus, String> + Send + 'static,
) -> CmdResult<RuntimeStatus> {
    let cancel = app.state::<RuntimeInstall>().0.clone();
    cancel.store(false, std::sync::atomic::Ordering::SeqCst);
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let plugins = handle.state::<Plugins>();
        let mut last = std::time::Instant::now() - std::time::Duration::from_secs(1);
        let mut report = |p: Progress| {
            let now = std::time::Instant::now();
            let boundary = !matches!(p, Progress::Downloading { .. } | Progress::Extracting { .. });
            if boundary || now.duration_since(last).as_millis() >= 100 {
                last = now;
                let _ = handle.emit(PLUGIN_RUNTIME_PROGRESS_EVENT, p);
            }
        };
        let status = install(&plugins, &cancel, &mut report)?;
        if status.host.as_deref() != Some("runtime") {
            return Ok(status);
        }
        Ok(plugins
            .check_runtime()
            .problem
            .map_or(status, |problem| RuntimeStatus { problem: Some(problem), ..plugins.runtime_status() }))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn cancel_plugin_runtime_install(state: State<'_, RuntimeInstall>) -> CmdResult<()> {
    state.0.store(true, std::sync::atomic::Ordering::SeqCst);
    Ok(())
}

/// Remove the runtime (plugins and their settings are kept).
#[tauri::command]
pub async fn remove_plugin_runtime(app: AppHandle) -> CmdResult<RuntimeStatus> {
    blocking(app, |p| p.remove_runtime().map(|()| p.runtime_status())).await
}

/// Download calibre's own build as a plugin host, with `plugin-runtime-progress`
/// events meanwhile (the same install flag and Cancel as the runtime).
#[tauri::command]
pub async fn install_plugin_calibre(app: AppHandle) -> CmdResult<RuntimeStatus> {
    install_runtime_with(app, |plugins, cancel, report| plugins.install_calibre(cancel, report)).await
}

/// Remove the calibre Magnitide downloaded (plugins and settings are kept).
#[tauri::command]
pub async fn remove_plugin_calibre(app: AppHandle) -> CmdResult<RuntimeStatus> {
    blocking(app, |p| p.remove_calibre().map(|()| p.runtime_status())).await
}

/// Choose where plugins run: an installed calibre, Magnitide's runtime, or
/// whichever is there (`auto`).
#[tauri::command]
pub async fn set_plugin_host(app: AppHandle, choice: HostChoice) -> CmdResult<RuntimeStatus> {
    blocking(app, move |p| p.set_host_choice(choice).map(|()| p.runtime_status())).await
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
