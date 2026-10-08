//! calibre plugins in Magnitide, run in Magnitide's own plugin runtime.
//!
//! Plugins load in a helper process ([`host`]): the plugin runtime's Python
//! ([`runtime`], downloaded once) running the `magnitide_host` package from
//! the compat package embedded in the binary, which provides the `calibre`
//! and `qt` modules plugins import. Plugin settings live in Magnitide's own
//! folder, never calibre's; calibre needn't be installed.
//!
//! Supported so far: metadata sources, in Find Metadata ([`sources`]), and
//! GUI plugins ([`gui`]), which work on a calibre-format shadow of the open
//! library ([`shadow`]). Other kinds of plugin can be installed and are
//! kept, marked as not supported yet. See docs/calibre-plugins.md for the plan.

pub mod calibre;
pub mod filetype;
pub mod gui;
pub mod host;
pub mod metadata;
pub mod prefs;
pub mod runtime;
pub mod shadow;
pub mod sources;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use include_dir::{Dir, include_dir};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub use calibre::{CalibreDownloadStatus, CalibreInstall, HostChoice};
pub use filetype::Hook;
pub use gui::{DEFAULT_OUTPUT_FORMAT, MenuItem, PluginMenu, PluginMenus};
pub use host::{EventHandler, Host, Transport};
pub use prefs::Configured;
pub use runtime::{Progress, Runtime};

/// The compat package: `magnitide_host`, `calibre`, `qt` and friends, embedded
/// so it versions with Magnitide; extracted beside the plugins at run time.
static COMPAT: Dir = include_dir!("$CARGO_MANIFEST_DIR/python");

/// A hash of the compat package, from build.rs: the folder it's extracted to.
pub const COMPAT_HASH: &str = env!("MAGNITIDE_COMPAT_HASH");

/// The calibre API level the compat package provides; plugins see it as
/// calibre's version. Matches `COMPAT_VERSION` in `magnitide_host/__init__.py`.
pub const CALIBRE_API_VERSION: &str = "9.15.0";

/// The error when the runtime isn't installed (the UI recognizes it).
pub const RUNTIME_MISSING: &str = "The plugin runtime isn't installed. Install it in Settings › Plugins.";

/// Environment variable naming a compat package directory to use instead of
/// the embedded one (development: Python edits without a Rust rebuild).
pub const COMPAT_ENV_OVERRIDE: &str = "MAGNITIDE_COMPAT_DIR";

/// Inspecting a zip loads the plugin's code, which can take a moment.
const INSPECT_TIMEOUT: Duration = Duration::from_secs(60);

/// What kind of calibre plugin it is (calibre's plugin base class).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum PluginKind {
    MetadataSource,
    InterfaceAction,
    FileType,
    MetadataReader,
    MetadataWriter,
    ConversionInput,
    ConversionOutput,
    Catalog,
    Preferences,
    LibraryClosed,
    Device,
    Store,
    Viewer,
    EditBookTool,
    Other,
}

impl PluginKind {
    /// Whether Magnitide can run this kind of plugin.
    pub fn supported(self) -> bool {
        matches!(
            self,
            PluginKind::MetadataSource
                | PluginKind::InterfaceAction
                | PluginKind::FileType
                | PluginKind::MetadataReader
                | PluginKind::MetadataWriter
                | PluginKind::Preferences
                | PluginKind::LibraryClosed
        )
    }

    /// Why a kind isn't supported, for Settings.
    pub fn why_unsupported(self) -> Option<&'static str> {
        Some(match self {
            PluginKind::ConversionInput | PluginKind::ConversionOutput => {
                "Conversion plugins need calibre's conversion pipeline, which Magnitide doesn't run."
            }
            PluginKind::Catalog => "Magnitide has no catalog generator yet.",
            PluginKind::Device => "Magnitide has its own device support.",
            PluginKind::Store => "Magnitide has no book store.",
            PluginKind::Viewer => "Magnitide has no viewer.",
            PluginKind::EditBookTool => "Magnitide has no book editor.",
            PluginKind::Other => "Magnitide doesn't know this kind of plugin.",
            _ => return None,
        })
    }
}

/// What a file-type, reader or writer plugin handles, known without
/// starting the runtime.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct Hooks {
    /// Lower-case extensions; `*` for every book.
    pub file_types: Vec<String>,
    pub on_import: bool,
    pub on_postimport: bool,
    pub on_postadd: bool,
    pub on_postconvert: bool,
    pub on_postdelete: bool,
    /// Conversion-time hooks Magnitide doesn't run.
    pub on_preprocess: bool,
    pub on_postprocess: bool,
    /// calibre's plugin priority (its own readers and writers are 1).
    pub priority: i64,
}

/// An installed plugin.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct Installed {
    /// The plugin's name, as calibre shows it; unique among installed plugins.
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,
    pub kind: PluginKind,
    /// Its zip in the plugins folder.
    pub file: String,
    pub enabled: bool,
    /// What a metadata source can do: "identify", "cover".
    #[serde(default)]
    pub capabilities: Vec<String>,
    pub minimum_calibre_version: String,
    /// What a file-type, reader or writer plugin handles; None for other kinds.
    #[serde(default)]
    pub hooks: Option<Hooks>,
    /// Whether it has settings of its own (Settings shows Configure…).
    #[serde(default)]
    pub customizable: bool,
    /// calibre features it uses that Magnitide doesn't provide (from a scan at install).
    #[serde(default)]
    pub coverage: Vec<String>,
}

/// calibre plugins: the installed list, the runtime, and the host that runs them.
pub struct Plugins {
    /// `<app config>/calibre-plugins`: `plugins/*.zip`, `plugins.json`,
    /// `settings.json`, `compat/<hash>/` (the compat package) and `config/`
    /// (calibre's config folder for the plugins).
    dir: PathBuf,
    /// Where the runtime is installed (`<base>/<runtime::VERSION>/`).
    runtime_base: PathBuf,
    installing: AtomicBool,
    installing_calibre: AtomicBool,
    host: Mutex<Option<Arc<dyn Transport>>>,
    list: Mutex<()>,
    on_event: Mutex<Option<EventHandler>>,
    /// The last error of each plugin, by name, for Settings.
    errors: Mutex<BTreeMap<String, String>>,
    hook_failure: filetype::HookFailure,
}

/// Where the host runs.
#[derive(Debug, Clone)]
pub enum Launcher {
    Calibre(CalibreInstall),
    Runtime(Runtime),
}

pub const CALIBRE_MISSING: &str =
    "No installed calibre was found. Install calibre, or choose Magnitide's plugin runtime in Settings › Plugins.";
pub const CALIBRE_DOWNLOAD_MISSING: &str =
    "Magnitide hasn't downloaded calibre yet. Download it in Settings › Plugins, or choose another host there.";
pub const RUNTIME_OR_CALIBRE_MISSING: &str =
    "calibre plugins need an installed calibre or Magnitide's plugin runtime. Install either in Settings › Plugins.";

/// The runtime (and the host choice), for Settings.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct RuntimeStatus {
    /// Settings' choice of host.
    pub host_choice: HostChoice,
    /// What that choice comes to now: "calibre", "runtime", or None when
    /// neither is available (then `host_problem` says why).
    pub host: Option<String>,
    pub host_problem: Option<String>,
    /// The installed calibre, when one was found.
    pub calibre: Option<CalibreInstall>,
    /// The calibre Magnitide downloads.
    pub calibre_download: CalibreDownloadStatus,
    pub installed: bool,
    pub installing: bool,
    /// Whether a runtime is published for this computer.
    pub available: bool,
    pub version: Option<String>,
    pub python: Option<String>,
    pub qt: Option<String>,
    /// The calibre API level the compat package provides.
    pub compat: String,
    pub path: Option<String>,
    /// Bytes on disk, when installed.
    pub size: Option<u64>,
    /// The archive's size in bytes, when not installed and available.
    pub download_size: Option<u64>,
    /// The published archive's file name and URL for this computer, when
    /// available (for installing from a file downloaded by hand).
    pub download_file: Option<String>,
    pub download_url: Option<String>,
    /// Why it can't run (a missing system library, say), when installed.
    pub problem: Option<String>,
}

impl Plugins {
    pub fn new(dir: PathBuf, runtime_base: PathBuf) -> Self {
        Self {
            dir,
            runtime_base,
            installing: AtomicBool::new(false),
            installing_calibre: AtomicBool::new(false),
            host: Mutex::new(None),
            list: Mutex::new(()),
            on_event: Mutex::new(None),
            errors: Mutex::new(BTreeMap::new()),
            hook_failure: Default::default(),
        }
    }

    /// Note a plugin's latest error (shown in Settings).
    pub fn note_error(&self, plugin: &str, error: &str) {
        self.errors.lock().unwrap_or_else(|e| e.into_inner()).insert(plugin.to_owned(), error.to_owned());
    }

    /// Each plugin's latest error, by name.
    pub fn errors(&self) -> BTreeMap<String, String> {
        self.errors.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// A `Plugins` talking to `transport` instead of a real host (tests).
    pub fn with_transport(dir: PathBuf, transport: Arc<dyn Transport>) -> Self {
        let p = Self::new(dir.clone(), dir.join("runtime"));
        *p.host.lock().unwrap_or_else(|e| e.into_inner()) = Some(transport);
        p
    }

    /// Where the host's events go (GUI plugins' status messages, errors, and
    /// that they're done). Applies to hosts started from now on.
    pub fn set_event_handler(&self, f: EventHandler) {
        *self.on_event.lock().unwrap_or_else(|e| e.into_inner()) = Some(f);
    }

    fn plugins_dir(&self) -> PathBuf {
        self.dir.join("plugins")
    }

    fn list_path(&self) -> PathBuf {
        self.dir.join("plugins.json")
    }

    /// The runtime to run plugins in, if there is one.
    pub fn runtime(&self) -> Option<Runtime> {
        Runtime::locate(&self.runtime_base)
    }

    /// The compat package on disk: the override, or the embedded one
    /// extracted to `compat/<hash>/` (once; older copies are removed).
    fn compat_dir(&self) -> Result<PathBuf, String> {
        if let Some(dir) = std::env::var_os(COMPAT_ENV_OVERRIDE).filter(|v| !v.is_empty()) {
            let dir = PathBuf::from(dir);
            if !dir.join("host").join("magnitide_host").is_dir() {
                return Err(format!("{COMPAT_ENV_OVERRIDE} ({}) holds no magnitide_host package", dir.display()));
            }
            return Ok(dir);
        }
        self.extract_compat()
    }

    /// The embedded compat package, unpacked under `<dir>/compat/<hash>/`
    /// once (earlier hashes are removed).
    fn extract_compat(&self) -> Result<PathBuf, String> {
        let root = self.dir.join("compat");
        let dir = root.join(COMPAT_HASH);
        if !dir.join(".complete").is_file() {
            let tmp = root.join(format!(".{COMPAT_HASH}.tmp"));
            let _ = std::fs::remove_dir_all(&tmp);
            std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
            COMPAT.extract(&tmp).map_err(|e| format!("couldn't unpack the plugin host: {e}"))?;
            std::fs::write(tmp.join(".complete"), b"").map_err(|e| e.to_string())?;
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::rename(&tmp, &dir).map_err(|e| e.to_string())?;
            if let Ok(entries) = std::fs::read_dir(&root) {
                for e in entries.flatten() {
                    if e.file_name() != COMPAT_HASH.as_ref() as &std::ffi::OsStr {
                        let _ = std::fs::remove_dir_all(e.path());
                    }
                }
            }
        }
        Ok(dir)
    }

    /// The running host, started (or restarted, if it stopped) as needed.
    pub fn host(&self) -> Result<Arc<dyn Transport>, String> {
        let mut slot = self.host.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(h) = slot.as_ref().filter(|h| h.is_alive()) {
            return Ok(h.clone());
        }
        let compat = self.compat_dir()?;
        std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        let on_event = self.on_event.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let config = self.dir.join("config");
        let h: Arc<dyn Transport> = match self.launcher()? {
            Launcher::Calibre(install) => {
                Arc::new(Host::start_calibre(Path::new(&install.calibre_debug), &compat, &config, on_event)?)
            }
            Launcher::Runtime(runtime) => Arc::new(Host::start(&runtime, &compat, &config, on_event)?),
        };
        *slot = Some(h.clone());
        drop(slot);
        // The host's own registry (what calibre's `find_plugin` and friends see).
        self.sync_registry(&h);
        Ok(h)
    }

    /// The host to start, by Settings' choice: an installed calibre (found
    /// now, so a calibre installed since is picked up), or the runtime.
    pub fn launcher(&self) -> Result<Launcher, String> {
        let choice = self.host_choice();
        let usable = |c: Option<CalibreInstall>| c.filter(|c| c.problem.is_none());
        match choice {
            HostChoice::Runtime => self.runtime().map(Launcher::Runtime).ok_or_else(|| RUNTIME_MISSING.to_owned()),
            HostChoice::Calibre => match calibre::find_installed() {
                Some(install) => match install.problem {
                    Some(problem) => Err(problem),
                    None => Ok(Launcher::Calibre(install)),
                },
                None => Err(CALIBRE_MISSING.to_owned()),
            },
            HostChoice::Downloaded => calibre::locate_downloaded(&self.runtime_base)
                .map(Launcher::Calibre)
                .ok_or_else(|| CALIBRE_DOWNLOAD_MISSING.to_owned()),
            HostChoice::Auto => {
                // An installed calibre, else the one Magnitide downloaded, else the runtime.
                if let Some(install) = usable(calibre::find_installed()) {
                    return Ok(Launcher::Calibre(install));
                }
                if let Some(install) = calibre::locate_downloaded(&self.runtime_base) {
                    return Ok(Launcher::Calibre(install));
                }
                self.runtime().map(Launcher::Runtime).ok_or_else(|| RUNTIME_OR_CALIBRE_MISSING.to_owned())
            }
        }
    }

    /// The calibre Magnitide downloads, for Settings.
    pub fn calibre_download_status(&self) -> CalibreDownloadStatus {
        let release = calibre::calibre_release();
        let installed = calibre::locate_downloaded(&self.runtime_base);
        CalibreDownloadStatus {
            version: calibre::CALIBRE_VERSION.into(),
            available: release.is_some(),
            installed: installed.is_some(),
            installing: self.installing_calibre.load(Ordering::SeqCst),
            path: installed.as_ref().map(|i| {
                Path::new(&i.calibre_debug)
                    .ancestors()
                    .nth(if cfg!(target_os = "macos") { 3 } else { 1 })
                    .map_or_else(|| i.calibre_debug.clone(), |p| p.display().to_string())
            }),
            size: installed.as_ref().map(|_| calibre::downloaded_size(&self.runtime_base)),
            download_size: if installed.is_none() { release.map(|r| r.size) } else { None },
        }
    }

    /// Download and install calibre's own build as a plugin host.
    pub fn install_calibre(
        &self,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<RuntimeStatus, String> {
        if self.installing_calibre.swap(true, Ordering::SeqCst) {
            return Err("calibre is already being downloaded.".into());
        }
        let result = calibre::install_downloaded(&self.runtime_base, cancel, progress);
        self.installing_calibre.store(false, Ordering::SeqCst);
        result?;
        Ok(self.runtime_status())
    }

    /// Remove the downloaded calibre. Refused while a plugin is working.
    pub fn remove_calibre(&self) -> Result<(), String> {
        if self.gui_busy() {
            return Err("A plugin is still working. Finish in its window first, then try again.".into());
        }
        *self.host.lock().unwrap_or_else(|e| e.into_inner()) = None;
        calibre::remove_downloaded(&self.runtime_base)
    }

    /// The installed plugins as the host's registry wants them.
    fn registry(&self) -> Value {
        let plugins: Vec<Value> = self
            .list()
            .iter()
            .map(|p| json!({ "path": self.path_of(p), "name": p.name, "enabled": p.enabled, "kind": p.kind }))
            .collect();
        json!({ "plugins": plugins })
    }

    /// Tell a running host which plugins are installed and on.
    fn sync_registry(&self, host: &Arc<dyn Transport>) {
        if let Err(e) = host.call("plugins.set", self.registry(), Duration::from_secs(60)) {
            eprintln!("[plugins] couldn't sync the plugin registry: {e}");
        }
    }

    /// After the list changed: the running host, if any, learns of it.
    fn registry_changed(&self) {
        if let Some(h) = self.running_host() {
            self.sync_registry(&h);
        }
    }

    /// The host if it's running; never starts one.
    pub(crate) fn running_host(&self) -> Option<Arc<dyn Transport>> {
        self.host.lock().unwrap_or_else(|e| e.into_inner()).as_ref().filter(|h| h.is_alive()).cloned()
    }

    /// GUI plugins can't be unloaded from a running host, so a change to
    /// which are on restarts the host (it starts again when next needed).
    /// Refused while one is working, rather than closing its windows.
    fn restart_for_gui_change(&self, kind: PluginKind) -> Result<(), String> {
        if kind != PluginKind::InterfaceAction || self.running_host().is_none() {
            return Ok(());
        }
        if self.gui_busy() {
            return Err("A plugin is still working. Finish in its window first, then try again.".into());
        }
        *self.host.lock().unwrap_or_else(|e| e.into_inner()) = None;
        Ok(())
    }

    /// The runtime's state, for Settings (never starts a host).
    pub fn runtime_status(&self) -> RuntimeStatus {
        let release = runtime::release();
        let installing = self.installing.load(Ordering::SeqCst);
        let host_choice = self.host_choice();
        let calibre = calibre::find_installed();
        let (host, host_problem) = match self.launcher() {
            Ok(Launcher::Calibre(_)) => (Some("calibre".to_owned()), None),
            Ok(Launcher::Runtime(_)) => (Some("runtime".to_owned()), None),
            Err(e) => (None, Some(e)),
        };
        match self.runtime() {
            Some(rt) => RuntimeStatus {
                host_choice,
                host,
                host_problem,
                calibre,
                calibre_download: self.calibre_download_status(),
                installed: true,
                installing,
                available: release.is_some(),
                version: Some(rt.info.version.clone()),
                python: rt.info.python.clone(),
                qt: rt.info.qt.clone(),
                compat: CALIBRE_API_VERSION.into(),
                path: Some(rt.dir.display().to_string()),
                size: Some(rt.size_on_disk()),
                download_size: None,
                download_file: release.map(|r| r.file.to_owned()),
                download_url: release.map(|r| r.url()),
                problem: None,
            },
            None => RuntimeStatus {
                host_choice,
                host,
                host_problem,
                calibre,
                calibre_download: self.calibre_download_status(),
                installed: false,
                installing,
                available: release.is_some(),
                version: None,
                python: None,
                qt: None,
                compat: CALIBRE_API_VERSION.into(),
                path: None,
                size: None,
                download_size: release.map(|r| r.size),
                download_file: release.map(|r| r.file.to_owned()),
                download_url: release.map(|r| r.url()),
                problem: None,
            },
        }
    }

    /// Download and install the runtime, reporting progress. One at a time.
    pub fn install_runtime(
        &self,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<RuntimeStatus, String> {
        if self.installing.swap(true, Ordering::SeqCst) {
            return Err("The plugin runtime is already being installed.".into());
        }
        let result = Runtime::install(&self.runtime_base, cancel, progress);
        self.installing.store(false, Ordering::SeqCst);
        result?;
        Ok(self.runtime_status())
    }

    /// Install the runtime from an archive on disk (the published one for
    /// this computer, downloaded by hand).
    pub fn install_runtime_from_file(
        &self,
        archive: &Path,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<RuntimeStatus, String> {
        if self.installing.swap(true, Ordering::SeqCst) {
            return Err("The plugin runtime is already being installed.".into());
        }
        let result = Runtime::install_file(&self.runtime_base, archive, cancel, progress);
        self.installing.store(false, Ordering::SeqCst);
        result?;
        Ok(self.runtime_status())
    }

    /// Remove the installed runtime. Refused while a plugin is working.
    pub fn remove_runtime(&self) -> Result<(), String> {
        if self.gui_busy() {
            return Err("A plugin is still working. Finish in its window first, then try again.".into());
        }
        *self.host.lock().unwrap_or_else(|e| e.into_inner()) = None;
        Runtime::remove(&self.runtime_base)
    }

    /// Check the installed runtime starts (Qt included), noting the problem
    /// in the status if not.
    pub fn check_runtime(&self) -> RuntimeStatus {
        let mut status = self.runtime_status();
        if let Some(rt) = self.runtime() {
            match rt.check() {
                Ok((python, qt)) => {
                    status.python = Some(python);
                    status.qt = Some(qt);
                }
                Err(e) => status.problem = Some(e),
            }
        }
        status
    }

    /// The host's recent output, for troubleshooting.
    pub fn log(&self) -> Vec<String> {
        self.host.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|h| h.log()).unwrap_or_default()
    }

    pub fn list(&self) -> Vec<Installed> {
        std::fs::read(self.list_path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    fn save(&self, list: &[Installed]) -> Result<(), String> {
        std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        let tmp = self.dir.join(".plugins.json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(list).map_err(|e| e.to_string())?)
            .and_then(|()| std::fs::rename(&tmp, self.list_path()))
            .map_err(|e| e.to_string())
    }

    /// The zip of an installed plugin.
    pub fn path_of(&self, p: &Installed) -> PathBuf {
        self.plugins_dir().join(&p.file)
    }

    /// Install a calibre plugin zip (a newer version replaces the installed one
    /// of the same name). The host loads it to check it's a calibre plugin
    /// that can run here.
    pub fn install(&self, zip: &Path) -> Result<Installed, String> {
        let _guard = self.list.lock().unwrap_or_else(|e| e.into_inner());
        let dir = self.plugins_dir();
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        // Checked from its new home, so what's inspected is what's kept.
        let incoming = dir.join(format!(".incoming-{}.zip", std::process::id()));
        std::fs::copy(zip, &incoming).map_err(|e| format!("couldn't copy the plugin: {e}"))?;
        let info = match self.host().and_then(|h| h.call("inspect", json!({ "path": incoming }), INSPECT_TIMEOUT)) {
            Ok(v) => v,
            Err(e) => {
                let _ = std::fs::remove_file(&incoming);
                return Err(if e == RUNTIME_MISSING {
                    e
                } else {
                    format!("That isn't a calibre plugin Magnitide can load: {e}")
                });
            }
        };
        let text = |k: &str| info.get(k).and_then(Value::as_str).unwrap_or_default().to_owned();
        let name = text("name");
        if name.trim().is_empty() {
            let _ = std::fs::remove_file(&incoming);
            return Err("The plugin has no name.".into());
        }
        let kind: PluginKind =
            serde_json::from_value(info.get("kind").cloned().unwrap_or_default()).unwrap_or(PluginKind::Other);
        let mut list = self.list();
        let previous = list.iter().position(|p| p.name == name);
        let file = previous.map(|i| list[i].file.clone()).unwrap_or_else(|| unique_file(&slug(&name), &list));
        std::fs::rename(&incoming, dir.join(&file)).map_err(|e| e.to_string())?;
        let strings = |k: &str| -> Vec<String> {
            info.get(k)
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|c| c.as_str().map(str::to_owned)).collect())
                .unwrap_or_default()
        };
        let flag = |k: &str| info.get(k).and_then(Value::as_bool).unwrap_or(false);
        let hooks = matches!(kind, PluginKind::FileType | PluginKind::MetadataReader | PluginKind::MetadataWriter)
            .then(|| Hooks {
                file_types: strings("fileTypes").into_iter().map(|f| f.to_lowercase()).collect(),
                on_import: flag("onImport"),
                on_postimport: flag("onPostimport"),
                on_postadd: flag("onPostadd"),
                on_postconvert: flag("onPostconvert"),
                on_postdelete: flag("onPostdelete"),
                on_preprocess: flag("onPreprocess"),
                on_postprocess: flag("onPostprocess"),
                priority: info.get("priority").and_then(Value::as_i64).unwrap_or(1),
            });
        let installed = Installed {
            name,
            version: text("version"),
            author: text("author"),
            description: text("description"),
            kind,
            file,
            // An update keeps the user's choice; a new plugin starts on if it can run.
            enabled: previous.map_or(kind.supported(), |i| list[i].enabled),
            capabilities: strings("capabilities"),
            minimum_calibre_version: text("minimumCalibreVersion"),
            hooks,
            customizable: flag("customizable"),
            coverage: strings("coverage"),
        };
        let replaced = previous.is_some();
        match previous {
            Some(i) => list[i] = installed.clone(),
            None => list.push(installed.clone()),
        }
        list.sort_by_key(|p| p.name.to_lowercase());
        self.save(&list)?;
        if replaced || installed.enabled {
            self.restart_for_gui_change(kind)?;
        }
        self.registry_changed();
        Ok(installed)
    }

    pub fn remove(&self, name: &str) -> Result<(), String> {
        let _guard = self.list.lock().unwrap_or_else(|e| e.into_inner());
        let mut list = self.list();
        let Some(i) = list.iter().position(|p| p.name == name) else { return Ok(()) };
        if list[i].enabled {
            self.restart_for_gui_change(list[i].kind)?;
        }
        let p = list.remove(i);
        self.save(&list)?;
        let removed = match std::fs::remove_file(self.path_of(&p)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        };
        self.registry_changed();
        removed
    }

    pub fn set_enabled(&self, name: &str, enabled: bool) -> Result<(), String> {
        let _guard = self.list.lock().unwrap_or_else(|e| e.into_inner());
        let mut list = self.list();
        let p = list.iter_mut().find(|p| p.name == name).ok_or("That plugin isn't installed.")?;
        if p.enabled != enabled {
            self.restart_for_gui_change(p.kind)?;
        }
        p.enabled = enabled;
        self.save(&list)?;
        self.registry_changed();
        Ok(())
    }
}

/// A file name from a plugin name: lower case, letters, digits and dashes.
fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-');
    if out.is_empty() { "plugin".into() } else { out.chars().take(60).collect() }
}

fn unique_file(slug: &str, list: &[Installed]) -> String {
    let taken = |f: &str| list.iter().any(|p| p.file == f);
    let mut file = format!("{slug}.zip");
    let mut n = 2;
    while taken(&file) {
        file = format!("{slug}-{n}.zip");
        n += 1;
    }
    file
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names() {
        assert_eq!(slug("Barnes & Noble"), "barnes-noble");
        assert_eq!(slug("  Ébook Translator!!"), "book-translator");
        assert_eq!(slug("***"), "plugin");
        let taken = vec![Installed {
            name: "X".into(),
            version: String::new(),
            author: String::new(),
            description: String::new(),
            kind: PluginKind::Other,
            file: "goodreads.zip".into(),
            enabled: true,
            capabilities: vec![],
            minimum_calibre_version: String::new(),
            hooks: None,
            customizable: false,
            coverage: vec![],
        }];
        assert_eq!(unique_file("goodreads", &taken), "goodreads-2.zip");
        assert_eq!(unique_file("kobo", &taken), "kobo.zip");
    }

    #[test]
    fn the_compat_package_is_embedded() {
        assert!(COMPAT.get_file("host/magnitide_host/__init__.py").is_some());
        assert!(COMPAT.get_file("compat/calibre/__init__.py").is_some());
        assert_eq!(COMPAT_HASH.len(), 16);
        let dir = tempfile::tempdir().unwrap();
        let plugins = Plugins::new(dir.path().into(), dir.path().join("runtime"));
        // The embedded copy itself, whatever MAGNITIDE_COMPAT_DIR says (CI sets it).
        let compat = plugins.extract_compat().unwrap();
        assert!(compat.join("host/magnitide_host/__init__.py").is_file());
        assert!(compat.ends_with(COMPAT_HASH));
        let text = std::fs::read_to_string(compat.join("host/magnitide_host/__init__.py")).unwrap();
        assert!(text.contains(&format!("COMPAT_VERSION = '{CALIBRE_API_VERSION}'")));
    }

    #[test]
    fn without_a_runtime() {
        let dir = tempfile::tempdir().unwrap();
        let plugins = Plugins::new(dir.path().into(), dir.path().join("runtime"));
        // The development runtime may exist on this machine; a runtime that
        // isn't there is what's being tested.
        let status = plugins.runtime_status();
        assert_eq!(status.compat, CALIBRE_API_VERSION);
        if !status.installed {
            let err = plugins.host().err().unwrap();
            assert_eq!(err, RUNTIME_MISSING);
            assert!(plugins.install(Path::new("nope.zip")).is_err());
        }
        assert!(plugins.list().is_empty());
        Plugins::remove_runtime(&plugins).unwrap();
    }
}
