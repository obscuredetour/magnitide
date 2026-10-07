//! calibre plugins in Magnitide, run on the user's own installed calibre.
//!
//! Plugins load through calibre's own plugin loader inside a helper process
//! (`calibre-debug -e host.py`; see [`host`]), so their imports, Qt,
//! resources, translations and settings behave as in calibre. Magnitide
//! bundles nothing of calibre and works without it; only plugins need it.
//! Plugin settings live in Magnitide's own folder, never calibre's.
//!
//! Supported so far: metadata sources, in Find Metadata ([`sources`]), and
//! GUI plugins ([`gui`]), which work on a calibre-format shadow of the open
//! library ([`shadow`]). Other kinds of plugin can be installed and are
//! kept, marked as not supported yet. See docs/calibre-plugins.md for the plan.

pub mod gui;
pub mod host;
pub mod shadow;
pub mod sources;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub use gui::{MenuItem, PluginMenu, PluginMenus};
pub use host::{EventHandler, Host};

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
    Other,
}

impl PluginKind {
    /// Whether Magnitide can run this kind of plugin yet.
    pub fn supported(self) -> bool {
        matches!(self, PluginKind::MetadataSource | PluginKind::InterfaceAction)
    }
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
}

/// calibre plugins: the installed list, and the host that runs them.
pub struct Plugins {
    /// `<app config>/calibre-plugins`: `plugins/*.zip`, `plugins.json`,
    /// and `config/` (calibre's config folder for the plugins).
    dir: PathBuf,
    /// Finds calibre's `calibre-debug` (looked up again when the host starts,
    /// so installing calibre later works without restarting Magnitide).
    find_calibre: Box<dyn Fn() -> Option<PathBuf> + Send + Sync>,
    host: Mutex<Option<Arc<Host>>>,
    list: Mutex<()>,
    on_event: Mutex<Option<EventHandler>>,
}

/// The calibre host's version, for Settings.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct Calibre {
    pub path: String,
    pub version: Option<String>,
}

impl Plugins {
    pub fn new(dir: PathBuf, find_calibre: impl Fn() -> Option<PathBuf> + Send + Sync + 'static) -> Self {
        Self {
            dir,
            find_calibre: Box::new(find_calibre),
            host: Mutex::new(None),
            list: Mutex::new(()),
            on_event: Mutex::new(None),
        }
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

    pub fn calibre_debug(&self) -> Option<PathBuf> {
        (self.find_calibre)().filter(|p| p.is_file())
    }

    /// The running host, started (or restarted, if it stopped) as needed.
    pub fn host(&self) -> Result<Arc<Host>, String> {
        let mut slot = self.host.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(h) = slot.as_ref().filter(|h| h.is_alive()) {
            return Ok(h.clone());
        }
        let calibre = self.calibre_debug().ok_or("calibre plugins need calibre installed (calibre.com).")?;
        let script = self.dir.join("host.py");
        std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        // Rewritten when Magnitide updates it.
        if std::fs::read_to_string(&script).ok().as_deref() != Some(host::SCRIPT) {
            std::fs::write(&script, host::SCRIPT).map_err(|e| e.to_string())?;
        }
        let on_event = self.on_event.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let h = Arc::new(Host::start(&calibre, &script, &self.dir.join("config"), on_event)?);
        *slot = Some(h.clone());
        Ok(h)
    }

    /// The host if it's running; never starts one.
    pub(crate) fn running_host(&self) -> Option<Arc<Host>> {
        self.host.lock().unwrap_or_else(|e| e.into_inner()).as_ref().filter(|h| h.is_alive()).cloned()
    }

    /// GUI plugins can't be unloaded from a running calibre, so a change to
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

    /// calibre's location and version (starting the host to ask).
    pub fn calibre(&self) -> Option<Calibre> {
        let path = self.calibre_debug()?;
        let version = self
            .host()
            .and_then(|h| h.call("hello", json!({}), Duration::from_secs(60)))
            .ok()
            .and_then(|v| v.get("calibre").and_then(Value::as_str).map(str::to_owned));
        Some(Calibre { path: shown_path(&path), version })
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
    /// that this calibre can run.
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
                return Err(format!("That isn't a calibre plugin this calibre can load: {e}"));
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
        let installed = Installed {
            name,
            version: text("version"),
            author: text("author"),
            description: text("description"),
            kind,
            file,
            // An update keeps the user's choice; a new plugin starts on if it can run.
            enabled: previous.map_or(kind.supported(), |i| list[i].enabled),
            capabilities: info
                .get("capabilities")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|c| c.as_str().map(str::to_owned)).collect())
                .unwrap_or_default(),
            minimum_calibre_version: text("minimumCalibreVersion"),
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
        match std::fs::remove_file(self.path_of(&p)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        }
    }

    pub fn set_enabled(&self, name: &str, enabled: bool) -> Result<(), String> {
        let _guard = self.list.lock().unwrap_or_else(|e| e.into_inner());
        let mut list = self.list();
        let p = list.iter_mut().find(|p| p.name == name).ok_or("That plugin isn't installed.")?;
        if p.enabled != enabled {
            self.restart_for_gui_change(p.kind)?;
        }
        p.enabled = enabled;
        self.save(&list)
    }
}

/// Where calibre is, as people know it: on macOS the app ("/Applications/calibre.app"),
/// elsewhere its folder.
fn shown_path(calibre_debug: &Path) -> String {
    let full = calibre_debug.display().to_string();
    if let Some(i) = full.find(".app/") {
        return full[..i + 4].to_owned();
    }
    calibre_debug.parent().map_or(full.clone(), |p| p.display().to_string())
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
        }];
        assert_eq!(unique_file("goodreads", &taken), "goodreads-2.zip");
        assert_eq!(unique_file("kobo", &taken), "kobo.zip");
    }

    #[test]
    fn calibre_paths() {
        assert_eq!(
            shown_path(Path::new("/Applications/calibre.app/Contents/MacOS/calibre-debug")),
            "/Applications/calibre.app"
        );
        assert_eq!(shown_path(Path::new("/opt/calibre/calibre-debug")), "/opt/calibre");
    }

    #[test]
    fn no_calibre() {
        let dir = tempfile::tempdir().unwrap();
        let plugins = Plugins::new(dir.path().into(), || None);
        assert!(plugins.calibre().is_none());
        let err = plugins.host().err().unwrap();
        assert!(err.contains("need calibre installed"), "{err}");
        assert!(plugins.list().is_empty());
    }
}
