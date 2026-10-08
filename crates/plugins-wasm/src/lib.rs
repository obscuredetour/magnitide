//! Magnitide's own plugins: WebAssembly components on the `magnitide:plugin`
//! API (crates/plugin-sdk/wit), run in wasmtime with no ambient filesystem
//! or network. What a plugin may do is declared in its manifest
//! (`capabilities`) and enforced by which host functions do anything.
//!
//! A plugin is a `.mgp` zip holding `manifest.toml` and the component it
//! names, installed into `<dir>/<slug>/`. Each call gets a fresh store with
//! a memory cap and a deadline, so a misbehaving plugin is an error for
//! that call, not a crash.
//!
//! Three kinds, one world each: metadata sources (asked in Find Metadata),
//! import hooks and file-metadata plugins (readers and writers), the latter
//! two as `core::hooks`' traits. A plugin that works on a file gets it
//! through a per-call sandbox: the file linked into a scratch folder
//! preopened read-only as `/in`, and a writable `/out` for what it makes.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use magnitide_catalog::{Book, BookId, CustomValue};
use magnitide_core::custom::{CustomField, FieldKind, FieldValue};
use magnitide_core::edit::BookEdit;
use magnitide_core::hooks::{ImportHooks, ImportOutcome, MetadataPlugins, Precedence};
use magnitide_core::metadata::{Author, BookMetadata};
use magnitide_sources::{Lookup, SourceRecord};
use serde::{Deserialize, Serialize};
use wasmtime::component::{Component, HasSelf, Linker, ResourceTable};
use wasmtime::{Engine, Store, StoreLimits, StoreLimitsBuilder, UpdateDeadline};
use wasmtime_wasi::{FsPerms, WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

mod bindings {
    wasmtime::component::bindgen!({
        world: "metadata-source-plugin",
        path: "../plugin-sdk/wit",
    });
}

/// The other worlds share the first one's imports (types, log, http).
mod import_hook_bindings {
    wasmtime::component::bindgen!({
        world: "import-hook-plugin",
        path: "../plugin-sdk/wit",
        with: {
            "magnitide:plugin/log": crate::bindings::magnitide::plugin::log,
            "magnitide:plugin/http": crate::bindings::magnitide::plugin::http,
        },
    });
}

mod file_metadata_bindings {
    wasmtime::component::bindgen!({
        world: "file-metadata-plugin",
        path: "../plugin-sdk/wit",
        with: {
            "magnitide:plugin/types": crate::bindings::magnitide::plugin::types,
            "magnitide:plugin/log": crate::bindings::magnitide::plugin::log,
            "magnitide:plugin/http": crate::bindings::magnitide::plugin::http,
        },
    });
}

mod library_action_bindings {
    wasmtime::component::bindgen!({
        world: "library-action-plugin",
        path: "../plugin-sdk/wit",
        with: {
            "magnitide:plugin/types": crate::bindings::magnitide::plugin::types,
            "magnitide:plugin/log": crate::bindings::magnitide::plugin::log,
            "magnitide:plugin/http": crate::bindings::magnitide::plugin::http,
        },
    });
}

use bindings::MetadataSourcePlugin;
use bindings::magnitide::plugin::{http, log, types};
use file_metadata_bindings::FileMetadataPlugin;
use file_metadata_bindings::exports::magnitide::plugin::file_metadata as wit_file_metadata;
use import_hook_bindings::ImportHookPlugin;
use import_hook_bindings::exports::magnitide::plugin::import_hook as wit_import_hook;
use library_action_bindings::LibraryActionPlugin;
use library_action_bindings::magnitide::plugin::{library, ui};

/// The API version this host provides; a plugin's manifest names the one it targets.
pub const API_VERSION: &str = "0.1";

/// Memory a plugin may use per call.
const MEMORY_LIMIT: usize = 256 * 1024 * 1024;
/// Wall-clock allowed per call unless set otherwise (epoch ticks are every 100 ms).
const CALL_TIMEOUT: Duration = Duration::from_secs(60);
/// An import hook may rewrite a whole book: this many call timeouts.
const ON_IMPORT_TIMEOUTS: u32 = 5;
/// A library action may work through a whole library: this many.
const ACTION_TIMEOUTS: u32 = 60;
const EPOCH_TICK: Duration = Duration::from_millis(100);
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
const HTTP_BODY_LIMIT: u64 = 20 * 1024 * 1024;
const LOG_LINES: usize = 400;

/// What a plugin may do, declared in its manifest and granted at install.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, rename = "NativeCapability"))]
#[serde(rename_all = "kebab-case")]
pub enum Capability {
    /// HTTP through Magnitide.
    Network,
    /// Editing books (library actions).
    WriteLibrary,
    /// Reading book files (library actions).
    ReadFiles,
}

/// What kind of plugin, by the world it implements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, rename = "NativeKind"))]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    /// Asked in Find Metadata (world `metadata-source-plugin`).
    MetadataSource,
    /// Runs on files being added (world `import-hook-plugin`).
    ImportHook,
    /// Reads and writes metadata in book files (world `file-metadata-plugin`).
    FileMetadata,
    /// Menu items on the selected books (world `library-action-plugin`).
    LibraryAction,
}

/// `manifest.toml` in a `.mgp`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, rename = "NativeManifest"))]
#[serde(rename_all = "kebab-case")]
pub struct Manifest {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub description: String,
    /// The `magnitide:plugin` API version the component was built for ("0.1").
    pub api: String,
    pub kind: Kind,
    #[serde(default)]
    pub capabilities: Vec<Capability>,
    /// The component file inside the zip ("plugin.wasm").
    #[serde(default = "default_component")]
    pub component: String,
}

fn default_component() -> String {
    "plugin.wasm".into()
}

/// An installed plugin, for Settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, rename = "NativePlugin"))]
#[serde(rename_all = "camelCase")]
pub struct Installed {
    pub manifest: Manifest,
    /// Folder name under the plugins folder.
    pub slug: String,
    pub enabled: bool,
    /// What the component said it handles when it was installed.
    #[serde(default)]
    pub handles: Handles,
}

/// What an import-hook or file-metadata plugin handles, asked of the
/// component at install so no plugin runs just to know.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, rename = "NativeHandles"))]
#[serde(rename_all = "camelCase")]
pub struct Handles {
    /// Lower-case extensions an import hook runs on; "*" for every file.
    #[serde(default)]
    pub file_types: Vec<String>,
    /// Upper-case formats a file-metadata plugin reads, with whether it
    /// beats Magnitide's own reader.
    #[serde(default)]
    pub reads: Vec<ReadsFormat>,
    /// Upper-case formats it writes.
    #[serde(default)]
    pub writes: Vec<String>,
    /// A library action's menu.
    #[serde(default)]
    pub menu: Vec<MenuEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, rename = "NativeMenuItem"))]
#[serde(rename_all = "camelCase")]
pub struct MenuEntry {
    pub id: String,
    pub text: String,
    pub needs_selection: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, rename = "NativeReadsFormat"))]
#[serde(rename_all = "camelCase")]
pub struct ReadsFormat {
    pub format: String,
    /// True when the plugin's reader is used before Magnitide's own.
    pub overrides: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Registry {
    #[serde(default)]
    plugins: Vec<Installed>,
}

/// The plugins folder and the engine that runs them.
pub struct WasmPlugins {
    dir: PathBuf,
    engine: Engine,
    list: Mutex<()>,
    /// Compiled components by slug and the component file's modification time.
    compiled: Mutex<BTreeMap<String, (std::time::SystemTime, Component)>>,
    /// Each plugin's recent log lines, by name.
    logs: Mutex<BTreeMap<String, Vec<String>>>,
    /// Each plugin's last error, by name.
    errors: Mutex<BTreeMap<String, String>>,
    ticking: Arc<AtomicBool>,
    call_timeout: Duration,
}

impl Drop for WasmPlugins {
    fn drop(&mut self) {
        self.ticking.store(false, Ordering::SeqCst);
    }
}

/// A call's scratch folders: `in/` (the file, read-only in the sandbox as
/// `/in`) and `out/` (writable, `/out`). Removed when the call is done,
/// but for a file the plugin made that the caller takes over.
struct Sandbox {
    dir: PathBuf,
}

impl Sandbox {
    fn input(&self) -> PathBuf {
        self.dir.join("in")
    }

    fn output(&self) -> PathBuf {
        self.dir.join("out")
    }

    /// The host path of a `/out/...` path the plugin named, if it's one.
    fn host_path(&self, guest: &str) -> Option<PathBuf> {
        out_path(&self.dir, guest)
    }
}

/// The host path under a call's folder of a `/out/...` path the plugin
/// named, if it's one.
fn out_path(dir: &Path, guest: &str) -> Option<PathBuf> {
    let rest = guest.strip_prefix("/out/")?;
    if rest.is_empty() || rest.split('/').any(|c| c.is_empty() || c == "." || c == "..") {
        return None;
    }
    Some(dir.join("out").join(rest))
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// What a plugin sees of the host during one call.
struct State {
    wasi: WasiCtx,
    table: ResourceTable,
    limits: StoreLimits,
    plugin: String,
    capabilities: Vec<Capability>,
    log: Vec<String>,
    /// The call's scratch folder, when it has one.
    sandbox: Option<PathBuf>,
    /// The library, for a library action.
    library: Option<Arc<dyn LibraryAccess>>,
    /// Books the action edited.
    touched: Vec<BookId>,
    /// Epoch ticks spent waiting on the user (questions): given back to the
    /// deadline, so waiting isn't the plugin's time.
    waited: u64,
}

impl WasiView for State {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView { ctx: &mut self.wasi, table: &mut self.table }
    }
}

impl types::Host for State {}

impl log::Host for State {
    fn log(&mut self, level: log::Level, text: String) {
        let level = match level {
            log::Level::Debug => "debug",
            log::Level::Info => "info",
            log::Level::Warn => "warn",
            log::Level::Error => "error",
        };
        self.log.push(format!("[{}] {level}: {text}", self.plugin));
    }
}

impl http::Host for State {
    fn fetch(&mut self, req: http::Request) -> Result<http::Response, String> {
        if !self.capabilities.contains(&Capability::Network) {
            return Err(format!(
                "{} asked for the network, which its manifest doesn't declare (capabilities = [\"network\"])",
                self.plugin
            ));
        }
        if !(req.url.starts_with("https://") || req.url.starts_with("http://")) {
            return Err(format!("only http(s) URLs can be fetched, not {}", req.url));
        }
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(HTTP_TIMEOUT))
            .user_agent(format!("Magnitide plugin {}", self.plugin))
            .build()
            .new_agent();
        let method = req.method.to_ascii_uppercase();
        let with_headers = |mut r: ureq::RequestBuilder<ureq::typestate::WithoutBody>| {
            for h in &req.headers {
                r = r.header(&h.name, &h.value);
            }
            r
        };
        let with_headers_body = |mut r: ureq::RequestBuilder<ureq::typestate::WithBody>| {
            for h in &req.headers {
                r = r.header(&h.name, &h.value);
            }
            r
        };
        let body = req.body.clone().unwrap_or_default();
        let mut resp = match method.as_str() {
            "GET" => with_headers(agent.get(&req.url)).call(),
            "HEAD" => with_headers(agent.head(&req.url)).call(),
            "DELETE" => with_headers(agent.delete(&req.url)).call(),
            "POST" => with_headers_body(agent.post(&req.url)).send(&body[..]),
            "PUT" => with_headers_body(agent.put(&req.url)).send(&body[..]),
            "PATCH" => with_headers_body(agent.patch(&req.url)).send(&body[..]),
            other => return Err(format!("unsupported HTTP method {other}")),
        }
        .map_err(|e| e.to_string())?;
        let status = resp.status().as_u16();
        let headers = resp
            .headers()
            .iter()
            .map(|(n, v)| http::Header { name: n.to_string(), value: v.to_str().unwrap_or_default().to_owned() })
            .collect();
        let body = resp
            .body_mut()
            .with_config()
            .limit(HTTP_BODY_LIMIT)
            .read_to_vec()
            .map_err(|e| format!("couldn't read the response from {}: {e}", req.url))?;
        // So a plugin's log shows what the network gave it.
        self.log.push(format!("[{}] http: {method} {} → {status}, {} bytes", self.plugin, req.url, body.len()));
        Ok(http::Response { status, headers, body })
    }
}

impl WasmPlugins {
    /// Plugins live in `dir` (`<app config>/plugins`).
    pub fn new(dir: PathBuf) -> Result<Self, String> {
        let mut config = wasmtime::Config::new();
        config.wasm_component_model(true);
        config.epoch_interruption(true);
        let engine = Engine::new(&config).map_err(|e| format!("couldn't start the plugin engine: {e}"))?;
        // Scratch folders of calls that never finished (a crash mid-import).
        let _ = std::fs::remove_dir_all(dir.join("tmp"));
        let ticking = Arc::new(AtomicBool::new(true));
        let (e, t) = (engine.clone(), ticking.clone());
        std::thread::Builder::new()
            .name("plugin-epochs".into())
            .spawn(move || {
                while t.load(Ordering::SeqCst) {
                    std::thread::sleep(EPOCH_TICK);
                    e.increment_epoch();
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            dir,
            engine,
            list: Mutex::new(()),
            compiled: Mutex::new(BTreeMap::new()),
            logs: Mutex::new(BTreeMap::new()),
            errors: Mutex::new(BTreeMap::new()),
            ticking,
            call_timeout: CALL_TIMEOUT,
        })
    }

    /// How long one call into a plugin may take (60 s unless set; tests shorten it).
    pub fn set_call_timeout(&mut self, timeout: Duration) {
        self.call_timeout = timeout;
    }

    fn registry_path(&self) -> PathBuf {
        self.dir.join("plugins.json")
    }

    fn read_registry(&self) -> Registry {
        std::fs::read(self.registry_path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    fn write_registry(&self, reg: &Registry) -> Result<(), String> {
        std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        let tmp = self.dir.join(".plugins.json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(reg).map_err(|e| e.to_string())?)
            .and_then(|()| std::fs::rename(&tmp, self.registry_path()))
            .map_err(|e| e.to_string())
    }

    /// The installed plugins.
    pub fn list(&self) -> Vec<Installed> {
        let _guard = self.list.lock().unwrap_or_else(|e| e.into_inner());
        self.read_registry().plugins
    }

    /// A plugin's recent log lines.
    pub fn log(&self, name: &str) -> Vec<String> {
        self.logs.lock().unwrap_or_else(|e| e.into_inner()).get(name).cloned().unwrap_or_default()
    }

    /// Each plugin's last error, by name.
    pub fn errors(&self) -> BTreeMap<String, String> {
        self.errors.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn note(&self, name: &str, lines: Vec<String>, error: Option<&str>) {
        if !lines.is_empty() {
            let mut logs = self.logs.lock().unwrap_or_else(|e| e.into_inner());
            let log = logs.entry(name.to_owned()).or_default();
            log.extend(lines);
            if log.len() > LOG_LINES {
                let drop = log.len() - LOG_LINES;
                log.drain(..drop);
            }
        }
        if let Some(e) = error {
            self.errors.lock().unwrap_or_else(|e| e.into_inner()).insert(name.to_owned(), e.to_owned());
        }
    }

    /// Install a `.mgp` (or update the plugin of that name). The component
    /// must compile and implement its kind's world before anything is kept.
    pub fn install(&self, mgp: &Path) -> Result<Installed, String> {
        let file = std::fs::File::open(mgp).map_err(|e| format!("couldn't open {}: {e}", mgp.display()))?;
        let mut zip =
            zip::ZipArchive::new(file).map_err(|_| "That isn't a Magnitide plugin (not a zip).".to_owned())?;
        let manifest_text = read_zip_text(&mut zip, "manifest.toml")
            .ok_or("That isn't a Magnitide plugin: no manifest.toml in it.".to_owned())?;
        let manifest: Manifest =
            toml::from_str(&manifest_text).map_err(|e| format!("The plugin's manifest.toml is invalid: {e}"))?;
        validate(&manifest)?;
        let component_bytes = {
            let mut f = zip
                .by_name(&manifest.component)
                .map_err(|_| format!("The plugin's manifest names {}, which isn't in the zip.", manifest.component))?;
            let mut v = Vec::new();
            std::io::Read::read_to_end(&mut f, &mut v).map_err(|e| e.to_string())?;
            v
        };
        let component = Component::new(&self.engine, &component_bytes)
            .map_err(|e| format!("The plugin's component can't be loaded: {}", first_line(&e.to_string())))?;
        // It must implement its kind's world, and say what it handles.
        let handles = self.handles(&component, &manifest)?;

        let _guard = self.list.lock().unwrap_or_else(|e| e.into_inner());
        let mut reg = self.read_registry();
        let previous = reg.plugins.iter().position(|p| p.manifest.name == manifest.name);
        let slug = match previous {
            Some(i) => reg.plugins[i].slug.clone(),
            None => unique_slug(&manifest.name, &reg.plugins.iter().map(|p| p.slug.as_str()).collect::<Vec<_>>()),
        };
        let folder = self.dir.join(&slug);
        let tmp = self.dir.join(format!(".{slug}.tmp"));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
        std::fs::write(tmp.join("manifest.toml"), manifest_text.as_bytes()).map_err(|e| e.to_string())?;
        std::fs::write(tmp.join(&manifest.component), &component_bytes).map_err(|e| e.to_string())?;
        // Resources beside the component (icons, data), if any.
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
            let Some(name) = entry.enclosed_name() else { continue };
            if entry.is_dir() || name == Path::new("manifest.toml") || name == Path::new(&manifest.component) {
                continue;
            }
            let dest = tmp.join(&name);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let mut out = std::fs::File::create(&dest).map_err(|e| e.to_string())?;
            std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
        }
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::rename(&tmp, &folder).map_err(|e| e.to_string())?;
        self.compiled.lock().unwrap_or_else(|e| e.into_inner()).remove(&slug);
        let installed = Installed {
            manifest,
            slug,
            // An update keeps the user's choice.
            enabled: previous.is_none_or(|i| reg.plugins[i].enabled),
            handles,
        };
        match previous {
            Some(i) => reg.plugins[i] = installed.clone(),
            None => reg.plugins.push(installed.clone()),
        }
        self.write_registry(&reg)?;
        Ok(installed)
    }

    pub fn remove(&self, name: &str) -> Result<(), String> {
        let _guard = self.list.lock().unwrap_or_else(|e| e.into_inner());
        let mut reg = self.read_registry();
        let Some(i) = reg.plugins.iter().position(|p| p.manifest.name == name) else {
            return Err(format!("{name} isn't installed."));
        };
        let slug = reg.plugins.remove(i).slug;
        let _ = std::fs::remove_dir_all(self.dir.join(&slug));
        self.compiled.lock().unwrap_or_else(|e| e.into_inner()).remove(&slug);
        self.write_registry(&reg)
    }

    pub fn set_enabled(&self, name: &str, enabled: bool) -> Result<(), String> {
        let _guard = self.list.lock().unwrap_or_else(|e| e.into_inner());
        let mut reg = self.read_registry();
        let p = reg
            .plugins
            .iter_mut()
            .find(|p| p.manifest.name == name)
            .ok_or_else(|| format!("{name} isn't installed."))?;
        p.enabled = enabled;
        self.write_registry(&reg)
    }

    /// Enabled metadata sources.
    pub fn metadata_sources(&self) -> Vec<Installed> {
        self.list().into_iter().filter(|p| p.enabled && p.manifest.kind == Kind::MetadataSource).collect()
    }

    /// Ask every enabled metadata source, at once: (name, its records or its error).
    pub fn identify(&self, lookup: &Lookup) -> Vec<(String, Result<Vec<SourceRecord>, String>)> {
        let sources = self.metadata_sources();
        std::thread::scope(|scope| {
            let handles: Vec<_> = sources
                .iter()
                .map(|p| scope.spawn(move || (p.manifest.name.clone(), self.identify_with(p, lookup))))
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().unwrap_or_else(|_| ("plugin".into(), Err("the plugin crashed".into()))))
                .collect()
        })
    }

    fn identify_with(&self, plugin: &Installed, lookup: &Lookup) -> Result<Vec<SourceRecord>, String> {
        let component = self.component(plugin)?;
        let (mut store, instance) = self.source(&component, &plugin.manifest.name, self.call_timeout)?;
        let wit_lookup =
            types::Lookup { title: lookup.title.clone(), authors: lookup.authors.clone(), isbn: lookup.isbn.clone() };
        let answer = instance.magnitide_plugin_metadata_source().call_identify(&mut store, &wit_lookup);
        let lines = std::mem::take(&mut store.data_mut().log);
        let result = match answer {
            Ok(Ok(records)) => Ok(records.into_iter().map(|r| to_record(r, &plugin.manifest.name)).collect()),
            Ok(Err(e)) => Err(e),
            Err(e) => Err(describe_trap(&e)),
        };
        self.note(&plugin.manifest.name, lines, result.as_ref().err().map(String::as_str));
        result
    }

    /// The plugin's info as the component reports it (a check that it answers).
    pub fn info(&self, plugin: &Installed) -> Result<(String, String, String), String> {
        let component = self.component(plugin)?;
        let (mut store, instance) = self.source(&component, &plugin.manifest.name, self.call_timeout)?;
        let info = instance.magnitide_plugin_metadata_source().call_info(&mut store).map_err(|e| describe_trap(&e))?;
        Ok((info.name, info.description, info.version))
    }

    fn component(&self, plugin: &Installed) -> Result<Component, String> {
        let path = self.dir.join(&plugin.slug).join(&plugin.manifest.component);
        let modified =
            std::fs::metadata(&path).and_then(|m| m.modified()).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut cache = self.compiled.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((when, c)) = cache.get(&plugin.slug)
            && *when == modified
        {
            return Ok(c.clone());
        }
        let c = Component::from_file(&self.engine, &path)
            .map_err(|e| format!("{} can't be loaded: {}", plugin.manifest.name, first_line(&e.to_string())))?;
        cache.insert(plugin.slug.clone(), (modified, c.clone()));
        Ok(c)
    }

    /// What a component handles, by asking it (and so that it implements
    /// its kind's world).
    fn handles(&self, component: &Component, m: &Manifest) -> Result<Handles, String> {
        let timeout = self.call_timeout;
        match m.kind {
            Kind::MetadataSource => {
                self.source(component, &m.name, timeout)?;
                Ok(Handles::default())
            }
            Kind::ImportHook => {
                let (mut store, hook) = self.import_hook(component, &m.name, None, timeout)?;
                let types =
                    hook.magnitide_plugin_import_hook().call_file_types(&mut store).map_err(|e| describe_trap(&e))?;
                let file_types: Vec<String> = types
                    .iter()
                    .map(|t| t.trim().trim_start_matches('.').to_lowercase())
                    .filter(|t| !t.is_empty())
                    .collect();
                if file_types.is_empty() {
                    return Err(format!("{} names no file types to run on.", m.name));
                }
                Ok(Handles { file_types, ..Default::default() })
            }
            Kind::FileMetadata => {
                let (mut store, plugin) = self.file_metadata(component, &m.name, None, timeout)?;
                let api = plugin.magnitide_plugin_file_metadata();
                let reads = api
                    .call_reads(&mut store)
                    .map_err(|e| describe_trap(&e))?
                    .into_iter()
                    .filter(|r| !r.format.trim().is_empty())
                    .map(|r| ReadsFormat {
                        format: r.format.trim().to_uppercase(),
                        overrides: r.precedence == wit_file_metadata::Precedence::Override,
                    })
                    .collect();
                let writes = api
                    .call_writes(&mut store)
                    .map_err(|e| describe_trap(&e))?
                    .into_iter()
                    .map(|f| f.trim().to_uppercase())
                    .filter(|f| !f.is_empty())
                    .collect();
                let handles = Handles { reads, writes, ..Default::default() };
                if handles.reads.is_empty() && handles.writes.is_empty() {
                    return Err(format!("{} names no formats it reads or writes.", m.name));
                }
                Ok(handles)
            }
            Kind::LibraryAction => {
                let (mut store, plugin) = self.library_action(component, &m.name, None, None, timeout)?;
                let menu: Vec<MenuEntry> = plugin
                    .magnitide_plugin_library_action()
                    .call_menu(&mut store)
                    .map_err(|e| describe_trap(&e))?
                    .into_iter()
                    .filter(|i| !i.id.trim().is_empty() && !i.text.trim().is_empty())
                    .map(|i| MenuEntry { id: i.id, text: i.text, needs_selection: i.needs_selection })
                    .collect();
                if menu.is_empty() {
                    return Err(format!("{} has no menu items.", m.name));
                }
                Ok(Handles { menu, ..Default::default() })
            }
        }
    }

    /// A fresh store for one call: the sandbox (a memory cap, a deadline,
    /// no environment; the scratch folders when the call works on a file).
    fn store(
        &self,
        name: &str,
        capabilities: &[Capability],
        sandbox: Option<&Sandbox>,
        library: Option<Arc<dyn LibraryAccess>>,
        timeout: Duration,
    ) -> Result<Store<State>, String> {
        let mut wasi = WasiCtxBuilder::new();
        if let Some(s) = sandbox {
            std::fs::create_dir_all(s.input())
                .and_then(|()| std::fs::create_dir_all(s.output()))
                .map_err(|e| e.to_string())?;
            wasi.preopened_dir(s.input(), "/in", FsPerms::ReadOnly).map_err(|e| e.to_string())?;
            wasi.preopened_dir(s.output(), "/out", FsPerms::ReadWrite).map_err(|e| e.to_string())?;
        }
        let state = State {
            wasi: wasi.build(),
            table: ResourceTable::new(),
            limits: StoreLimitsBuilder::new().memory_size(MEMORY_LIMIT).build(),
            plugin: name.to_owned(),
            capabilities: capabilities.to_vec(),
            log: Vec::new(),
            sandbox: sandbox.map(|s| s.dir.clone()),
            library,
            touched: Vec::new(),
            waited: 0,
        };
        let mut store = Store::new(&self.engine, state);
        store.limiter(|s| &mut s.limits);
        store.set_epoch_deadline((timeout.as_millis() / EPOCH_TICK.as_millis()).max(1) as u64);
        // At the deadline: time spent waiting on the user is given back; else stop.
        store.epoch_deadline_callback(|mut ctx| {
            let waited = std::mem::take(&mut ctx.data_mut().waited);
            Ok(if waited > 0 { UpdateDeadline::Continue(waited) } else { UpdateDeadline::Interrupt })
        });
        Ok(store)
    }

    fn linker(&self) -> Result<Linker<State>, String> {
        let mut linker = Linker::new(&self.engine);
        wasmtime_wasi::p2::add_to_linker_sync(&mut linker).map_err(|e| e.to_string())?;
        Ok(linker)
    }

    fn source(
        &self,
        component: &Component,
        name: &str,
        timeout: Duration,
    ) -> Result<(Store<State>, MetadataSourcePlugin), String> {
        let mut linker = self.linker()?;
        MetadataSourcePlugin::add_to_linker::<_, HasSelf<_>>(&mut linker, |s: &mut State| s)
            .map_err(|e| e.to_string())?;
        let mut store = self.store(name, &self.capabilities_of(name), None, None, timeout)?;
        let instance = MetadataSourcePlugin::instantiate(&mut store, component, &linker)
            .map_err(|e| format!("{name} doesn't implement the metadata-source API: {}", first_line(&e.to_string())))?;
        Ok((store, instance))
    }

    fn import_hook(
        &self,
        component: &Component,
        name: &str,
        sandbox: Option<&Sandbox>,
        timeout: Duration,
    ) -> Result<(Store<State>, ImportHookPlugin), String> {
        let mut linker = self.linker()?;
        ImportHookPlugin::add_to_linker::<_, HasSelf<_>>(&mut linker, |s: &mut State| s).map_err(|e| e.to_string())?;
        let mut store = self.store(name, &self.capabilities_of(name), sandbox, None, timeout)?;
        let instance = ImportHookPlugin::instantiate(&mut store, component, &linker)
            .map_err(|e| format!("{name} doesn't implement the import-hook API: {}", first_line(&e.to_string())))?;
        Ok((store, instance))
    }

    fn file_metadata(
        &self,
        component: &Component,
        name: &str,
        sandbox: Option<&Sandbox>,
        timeout: Duration,
    ) -> Result<(Store<State>, FileMetadataPlugin), String> {
        let mut linker = self.linker()?;
        FileMetadataPlugin::add_to_linker::<_, HasSelf<_>>(&mut linker, |s: &mut State| s)
            .map_err(|e| e.to_string())?;
        let mut store = self.store(name, &self.capabilities_of(name), sandbox, None, timeout)?;
        let instance = FileMetadataPlugin::instantiate(&mut store, component, &linker)
            .map_err(|e| format!("{name} doesn't implement the file-metadata API: {}", first_line(&e.to_string())))?;
        Ok((store, instance))
    }

    /// The capabilities an installed plugin was granted; none while it's
    /// being installed (the install-time calls get no capability).
    fn capabilities_of(&self, name: &str) -> Vec<Capability> {
        self.read_registry()
            .plugins
            .into_iter()
            .find(|p| p.manifest.name == name)
            .map(|p| p.manifest.capabilities)
            .unwrap_or_default()
    }

    /// A scratch folder for one call, under `<dir>/tmp`.
    fn sandbox(&self) -> Result<Sandbox, String> {
        use std::sync::atomic::AtomicU64;
        static N: AtomicU64 = AtomicU64::new(0);
        let dir = self.dir.join("tmp").join(format!("{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        std::fs::create_dir_all(&dir).map_err(|e| format!("couldn't make a scratch folder for the plugin: {e}"))?;
        Ok(Sandbox { dir })
    }

    /// Put `path` in the sandbox's `in/` (a hard link, or a copy across
    /// volumes); the plugin sees `/in/<name>`.
    fn stage(sandbox: &Sandbox, path: &Path) -> Result<String, String> {
        let name = file_name(path);
        let dest = sandbox.input().join(&name);
        std::fs::create_dir_all(sandbox.input()).map_err(|e| e.to_string())?;
        if std::fs::hard_link(path, &dest).is_err() {
            std::fs::copy(path, &dest).map_err(|e| format!("couldn't give the plugin {}: {e}", path.display()))?;
        }
        Ok(format!("/in/{name}"))
    }

    /// Enabled import hooks that run on `format` (upper case).
    fn hooks_for(&self, format: &str) -> Vec<Installed> {
        let ext = format.to_lowercase();
        self.list()
            .into_iter()
            .filter(|p| p.enabled && p.manifest.kind == Kind::ImportHook)
            .filter(|p| p.handles.file_types.iter().any(|t| t == "*" || *t == ext))
            .collect()
    }

    /// Enabled file-metadata plugins reading `format`, overriding ones first.
    fn readers_for(&self, format: &str) -> Vec<(Installed, bool)> {
        let mut ans: Vec<(Installed, bool)> = self
            .list()
            .into_iter()
            .filter(|p| p.enabled && p.manifest.kind == Kind::FileMetadata)
            .filter_map(|p| {
                let overrides = p.handles.reads.iter().find(|r| r.format == format)?.overrides;
                Some((p, overrides))
            })
            .collect();
        ans.sort_by_key(|(_, overrides)| !overrides);
        ans
    }

    fn writers_for(&self, format: &str) -> Vec<Installed> {
        self.list()
            .into_iter()
            .filter(|p| p.enabled && p.manifest.kind == Kind::FileMetadata)
            .filter(|p| p.handles.writes.iter().any(|w| w == format))
            .collect()
    }

    /// Run one import hook on `path`: unchanged, or a file it wrote (moved
    /// out of the sandbox, to be deleted by the caller once stored).
    fn run_hook(&self, plugin: &Installed, path: &Path, format: &str) -> Result<Option<PathBuf>, String> {
        let component = self.component(plugin)?;
        let sandbox = self.sandbox()?;
        let guest = Self::stage(&sandbox, path)?;
        let timeout = self.call_timeout * ON_IMPORT_TIMEOUTS;
        let (mut store, hook) = self.import_hook(&component, &plugin.manifest.name, Some(&sandbox), timeout)?;
        let answer = hook.magnitide_plugin_import_hook().call_on_import(&mut store, &guest, format);
        let lines = std::mem::take(&mut store.data_mut().log);
        drop(store);
        let result = match answer {
            Ok(Ok(wit_import_hook::Outcome::Unchanged)) => Ok(None),
            Ok(Ok(wit_import_hook::Outcome::Replaced(out))) => sandbox.take(&out).map(Some),
            Ok(Err(e)) => Err(e),
            Err(e) => Err(describe_trap(&e)),
        };
        self.note(&plugin.manifest.name, lines, result.as_ref().err().map(String::as_str));
        result
    }
}

impl Sandbox {
    /// Move the file the plugin named (`/out/...`) beside the call's
    /// folder, so the folder can go while the file lives on.
    fn take(&self, out: &str) -> Result<PathBuf, String> {
        let made = self.host_path(out).ok_or_else(|| format!("the plugin named {out}, which isn't under /out/"))?;
        if !made.is_file() {
            return Err(format!("the plugin named {out}, which it didn't write"));
        }
        let call = self.dir.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let kept = self.dir.with_file_name(format!("{call}-{}", file_name(&made)));
        std::fs::rename(&made, &kept).map_err(|e| e.to_string())?;
        Ok(kept)
    }
}

impl ImportHooks for WasmPlugins {
    fn file_types(&self) -> Vec<String> {
        let mut types: Vec<String> = self
            .list()
            .into_iter()
            .filter(|p| p.enabled && p.manifest.kind == Kind::ImportHook)
            .flat_map(|p| p.handles.file_types)
            .filter(|t| t != "*")
            .collect();
        types.sort();
        types.dedup();
        types
    }

    /// The hooks in turn, each on what the one before made of the file.
    fn on_import(&self, path: &Path, format: &str) -> ImportOutcome {
        let mut current = path.to_path_buf();
        let mut format = format.to_owned();
        let mut temporary: Option<PathBuf> = None;
        let mut failures = Vec::new();
        for plugin in self.hooks_for(&format) {
            match self.run_hook(&plugin, &current, &format) {
                Ok(Some(new)) => {
                    if let Some(old) = temporary.replace(new.clone()) {
                        let _ = std::fs::remove_file(old);
                    }
                    if let Some(ext) = new.extension().and_then(|e| e.to_str()) {
                        format = ext.to_uppercase();
                    }
                    current = new;
                }
                Ok(None) => {}
                Err(e) => failures.push(format!("{}: {e}", plugin.manifest.name)),
            }
        }
        match (temporary, failures.is_empty()) {
            (Some(path), _) => ImportOutcome::Replaced { path, temporary: true },
            (None, true) => ImportOutcome::Unchanged,
            (None, false) => ImportOutcome::Failed(failures.join("; ")),
        }
    }
}

impl MetadataPlugins for WasmPlugins {
    fn reads(&self, format: &str) -> Option<Precedence> {
        let readers = self.readers_for(format);
        let overrides = readers.iter().any(|(_, o)| *o);
        (!readers.is_empty()).then_some(if overrides { Precedence::Override } else { Precedence::Fallback })
    }

    /// The readers in turn; the first that answers.
    fn read(&self, path: &Path, format: &str) -> Result<BookMetadata, String> {
        let readers = self.readers_for(format);
        if readers.is_empty() {
            return Err(format!("no plugin reads {format}"));
        }
        let mut last = String::new();
        for (plugin, _) in readers {
            let result = (|| {
                let component = self.component(&plugin)?;
                let sandbox = self.sandbox()?;
                let guest = Self::stage(&sandbox, path)?;
                let (mut store, p) =
                    self.file_metadata(&component, &plugin.manifest.name, Some(&sandbox), self.call_timeout)?;
                let answer = p.magnitide_plugin_file_metadata().call_read(&mut store, &guest, format);
                let lines = std::mem::take(&mut store.data_mut().log);
                let result = match answer {
                    Ok(Ok(meta)) => Ok(from_wit_metadata(meta)),
                    Ok(Err(e)) => Err(e),
                    Err(e) => Err(describe_trap(&e)),
                };
                self.note(&plugin.manifest.name, lines, result.as_ref().err().map(String::as_str));
                result
            })();
            match result {
                Ok(meta) => return Ok(meta),
                Err(e) => last = format!("{}: {e}", plugin.manifest.name),
            }
        }
        Err(last)
    }

    fn writes(&self, format: &str) -> bool {
        !self.writers_for(format).is_empty()
    }

    /// Every writer in turn, each on the file as the one before left it;
    /// the file at `path` is replaced only when all succeeded.
    fn write(&self, path: &Path, format: &str, meta: &BookMetadata) -> Result<(), String> {
        let writers = self.writers_for(format);
        if writers.is_empty() {
            return Err(format!("no plugin writes {format}"));
        }
        let sandbox = self.sandbox()?;
        let name = file_name(path);
        std::fs::create_dir_all(sandbox.output()).map_err(|e| e.to_string())?;
        let copy = sandbox.output().join(&name);
        std::fs::copy(path, &copy).map_err(|e| format!("couldn't give the plugin {}: {e}", path.display()))?;
        let guest = format!("/out/{name}");
        let wit_meta = to_wit_metadata(meta);
        for plugin in writers {
            let component = self.component(&plugin)?;
            let (mut store, p) =
                self.file_metadata(&component, &plugin.manifest.name, Some(&sandbox), self.call_timeout)?;
            let answer = p.magnitide_plugin_file_metadata().call_write(&mut store, &guest, format, &wit_meta);
            let lines = std::mem::take(&mut store.data_mut().log);
            let result = match answer {
                Ok(Ok(())) => Ok(()),
                Ok(Err(e)) => Err(e),
                Err(e) => Err(describe_trap(&e)),
            };
            self.note(&plugin.manifest.name, lines, result.as_ref().err().map(String::as_str));
            result.map_err(|e| format!("{}: {e}", plugin.manifest.name))?;
        }
        // Over the original in one step: beside it, then renamed.
        let beside = path.with_file_name(format!(".{name}.plugin"));
        std::fs::copy(&copy, &beside).and_then(|_| std::fs::rename(&beside, path)).map_err(|e| {
            let _ = std::fs::remove_file(&beside);
            format!("couldn't put the written file back: {e}")
        })
    }
}

/// A file's name, or "book" when the path has none.
fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "book".into())
}

/// The open library as a library action sees it. The app implements this
/// over its open library; tests use a fake. Edits go through the same
/// paths as the app's own (names synced, files renamed, write-back).
pub trait LibraryAccess: Send + Sync {
    fn fields(&self) -> Result<Vec<CustomField>, String>;
    fn book(&self, id: BookId) -> Option<Book>;
    fn all_ids(&self) -> Vec<BookId>;
    /// Magnitide's search syntax.
    fn search(&self, query: &str) -> Result<Vec<BookId>, String>;
    fn book_edit(&self, id: BookId) -> Result<BookEdit, String>;
    fn update_book(&self, id: BookId, edit: &BookEdit) -> Result<(), String>;
    fn set_cover(&self, id: BookId, image: &[u8]) -> Result<(), String>;
    /// The path of a book's file, for linking into the sandbox.
    fn format_path(&self, id: BookId, format: &str) -> Result<PathBuf, String>;
    /// Add a format from `file`, replacing one the book has.
    fn add_format(&self, id: BookId, format: &str, file: &Path) -> Result<(), String>;
    /// A line of progress from the plugin, and how far along it is.
    fn status(&self, _text: &str) {}
    fn progress(&self, _done: u32, _total: u32) {}
    /// A question for the user: the index of the choice taken, or None when
    /// dismissed (or when nobody can answer, as on a server without a policy).
    fn ask(&self, _question: &str, _choices: &[String]) -> Option<u32> {
        None
    }
    fn ask_text(&self, _question: &str, _placeholder: &str) -> Option<String> {
        None
    }
}

/// What a library action did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ActionReport {
    /// The plugin's own line ("Tagged 12 books").
    pub summary: String,
    /// Books it edited, in the order it first touched them.
    pub updated: Vec<BookId>,
}

impl library::Host for State {
    fn fields(&mut self) -> Vec<library::CustomField> {
        let Some(lib) = &self.library else { return Vec::new() };
        lib.fields()
            .unwrap_or_default()
            .into_iter()
            .map(|f| library::CustomField {
                key: f.key,
                name: f.name,
                kind: kind_name(f.kind).into(),
                multiple: f.multiple,
            })
            .collect()
    }

    fn books(&mut self, ids: Vec<u64>) -> Vec<library::Book> {
        let Some(lib) = &self.library else { return Vec::new() };
        ids.into_iter().filter_map(|id| lib.book(id as BookId)).map(to_wit_book).collect()
    }

    fn all_books(&mut self) -> Vec<u64> {
        self.library.as_ref().map(|l| l.all_ids().into_iter().map(|id| id as u64).collect()).unwrap_or_default()
    }

    fn search(&mut self, query: String) -> Vec<u64> {
        let Some(lib) = &self.library else { return Vec::new() };
        match lib.search(&query) {
            Ok(ids) => ids.into_iter().map(|id| id as u64).collect(),
            Err(e) => {
                self.log.push(format!("[{}] warn: search {query:?}: {e}", self.plugin));
                Vec::new()
            }
        }
    }

    fn edit(&mut self, id: u64, changes: library::BookEdit) -> Result<(), String> {
        self.need(Capability::WriteLibrary, "edit the library")?;
        let lib = self.library.clone().ok_or("no library is open")?;
        let id = id as BookId;
        let mut edit = lib.book_edit(id)?;
        apply_edit(&mut edit, changes);
        lib.update_book(id, &edit)?;
        self.touch(id);
        Ok(())
    }

    fn set_cover(&mut self, id: u64, image: Vec<u8>) -> Result<(), String> {
        self.need(Capability::WriteLibrary, "edit the library")?;
        let lib = self.library.clone().ok_or("no library is open")?;
        lib.set_cover(id as BookId, &image)?;
        self.touch(id as BookId);
        Ok(())
    }

    fn open_format(&mut self, id: u64, format: String) -> Result<String, String> {
        self.need(Capability::ReadFiles, "read book files")?;
        let lib = self.library.clone().ok_or("no library is open")?;
        let sandbox = self.sandbox.clone().ok_or("no sandbox for this call")?;
        let path = lib.format_path(id as BookId, &format.to_uppercase())?;
        // Each book's file under its own folder: two books may share a file name.
        let sub = sandbox.join("in").join(id.to_string());
        std::fs::create_dir_all(&sub).map_err(|e| e.to_string())?;
        let name = file_name(&path);
        let dest = sub.join(&name);
        if !dest.exists() && std::fs::hard_link(&path, &dest).is_err() {
            std::fs::copy(&path, &dest).map_err(|e| format!("couldn't give the plugin {}: {e}", path.display()))?;
        }
        Ok(format!("/in/{id}/{name}"))
    }

    fn add_format(&mut self, id: u64, format: String, file: String) -> Result<(), String> {
        self.need(Capability::WriteLibrary, "edit the library")?;
        let lib = self.library.clone().ok_or("no library is open")?;
        let sandbox = self.sandbox.clone().ok_or("no sandbox for this call")?;
        let made = out_path(&sandbox, &file).ok_or_else(|| format!("{file} isn't under /out/"))?;
        if !made.is_file() {
            return Err(format!("{file} wasn't written"));
        }
        lib.add_format(id as BookId, &format.to_uppercase(), &made)?;
        self.touch(id as BookId);
        Ok(())
    }
}

impl ui::Host for State {
    fn status(&mut self, text: String) {
        self.log.push(format!("[{}] status: {text}", self.plugin));
        if let Some(lib) = &self.library {
            lib.status(&text);
        }
    }

    fn progress(&mut self, done: u32, total: u32) {
        if let Some(lib) = &self.library {
            lib.progress(done, total);
        }
    }

    fn ask(&mut self, question: String, choices: Vec<String>) -> Option<u32> {
        let lib = self.library.clone()?;
        let choices: Vec<String> = choices.into_iter().filter(|c| !c.trim().is_empty()).collect();
        if question.trim().is_empty() || choices.is_empty() {
            return None;
        }
        let started = std::time::Instant::now();
        let answer = lib.ask(&question, &choices).filter(|i| (*i as usize) < choices.len());
        self.waited(started);
        self.log.push(format!(
            "[{}] ask: {question} → {}",
            self.plugin,
            answer.map_or("dismissed".to_owned(), |i| choices[i as usize].clone())
        ));
        answer
    }

    fn ask_text(&mut self, question: String, placeholder: String) -> Option<String> {
        let lib = self.library.clone()?;
        if question.trim().is_empty() {
            return None;
        }
        let started = std::time::Instant::now();
        let answer = lib.ask_text(&question, &placeholder);
        self.waited(started);
        self.log.push(format!(
            "[{}] ask: {question} → {}",
            self.plugin,
            if answer.is_some() { "answered" } else { "dismissed" }
        ));
        answer
    }
}

impl State {
    /// Count the time since `started` as waiting, not as the plugin's.
    fn waited(&mut self, started: std::time::Instant) {
        self.waited += (started.elapsed().as_millis() / EPOCH_TICK.as_millis()) as u64 + 1;
    }
}

impl State {
    fn need(&self, capability: Capability, to: &str) -> Result<(), String> {
        if self.capabilities.contains(&capability) {
            Ok(())
        } else {
            let name = match capability {
                Capability::Network => "network",
                Capability::WriteLibrary => "write-library",
                Capability::ReadFiles => "read-files",
            };
            Err(format!(
                "{} asked to {to}, which its manifest doesn't declare (capabilities = [\"{name}\"])",
                self.plugin
            ))
        }
    }

    fn touch(&mut self, id: BookId) {
        if !self.touched.contains(&id) {
            self.touched.push(id);
        }
    }
}

fn kind_name(kind: FieldKind) -> &'static str {
    match kind {
        FieldKind::Text => "text",
        FieldKind::LongText => "long-text",
        FieldKind::Integer => "integer",
        FieldKind::Number => "number",
        FieldKind::Bool => "bool",
        FieldKind::Date => "date",
        FieldKind::Rating => "rating",
        FieldKind::Choice => "choice",
        FieldKind::Series => "series",
    }
}

fn to_wit_book(b: Book) -> library::Book {
    library::Book {
        id: b.id as u64,
        title: b.title,
        authors: b.authors,
        series: b.series,
        series_index: b.series_index,
        publisher: b.publisher,
        published: b.pubdate.map(|d| d.date_naive().to_string()),
        rating: b.rating,
        languages: b.languages,
        identifiers: b.identifiers.into_iter().map(|(key, value)| types::Identifier { key, value }).collect(),
        tags: b.tags,
        description_html: b.comments,
        formats: b.formats.into_iter().map(|f| f.format).collect(),
        has_cover: b.has_cover,
        custom: b
            .custom
            .into_iter()
            .map(|(key, value)| library::CustomValue {
                key: key.trim_start_matches('#').to_owned(),
                value: match value {
                    CustomValue::Text(t) => library::FieldValue::Text(t),
                    CustomValue::TextList(l) => library::FieldValue::TextList(l),
                    CustomValue::Series { name, index } => {
                        library::FieldValue::Series(library::SeriesValue { name, index })
                    }
                    CustomValue::Int(i) => library::FieldValue::Number(i as f64),
                    CustomValue::Float(f) => library::FieldValue::Number(f),
                    CustomValue::Bool(b) => library::FieldValue::Boolean(b),
                    CustomValue::Rating(r) => library::FieldValue::Number(f64::from(r)),
                    CustomValue::Datetime(d) => library::FieldValue::Text(d.date_naive().to_string()),
                },
            })
            .collect(),
    }
}

/// Lay the plugin's changes over the book's current values.
fn apply_edit(edit: &mut BookEdit, c: library::BookEdit) {
    let blank = |s: String| Some(s).filter(|s| !s.trim().is_empty());
    if let Some(t) = c.title.and_then(blank) {
        edit.title = t;
        edit.title_sort = String::new();
    }
    if let Some(a) = c.authors {
        edit.authors = a.into_iter().filter(|a| !a.trim().is_empty()).collect();
    }
    if let Some(s) = c.series {
        if s.name.trim().is_empty() {
            edit.series = None;
            edit.series_index = 1.0;
        } else {
            edit.series = Some(s.name);
            edit.series_index = s.index;
        }
    }
    if let Some(p) = c.publisher {
        edit.publisher = blank(p);
    }
    if let Some(p) = c.published {
        edit.published = blank(p);
    }
    if let Some(r) = c.rating {
        edit.rating = (r > 0).then_some(r.min(10));
    }
    if let Some(t) = c.tags {
        edit.tags = t;
    }
    if let Some(l) = c.languages {
        edit.languages = l;
    }
    if let Some(i) = c.identifiers {
        edit.identifiers = i.into_iter().map(|i| (i.key, i.value)).collect();
    }
    if let Some(d) = c.description_html {
        edit.description_html = blank(d);
    }
    for custom in c.custom {
        let value = custom.value.map(|v| match v {
            library::FieldValue::Boolean(b) => FieldValue::Bool(b),
            library::FieldValue::Number(n) => FieldValue::Number(n),
            library::FieldValue::Text(t) => FieldValue::Text(t),
            library::FieldValue::TextList(l) => FieldValue::List(l),
            library::FieldValue::Series(s) => FieldValue::Series { name: s.name, index: s.index },
        });
        edit.custom.insert(custom.key.trim_start_matches('#').to_owned(), value);
    }
}

impl WasmPlugins {
    /// Enabled library actions.
    pub fn library_actions(&self) -> Vec<Installed> {
        self.list().into_iter().filter(|p| p.enabled && p.manifest.kind == Kind::LibraryAction).collect()
    }

    /// Run a plugin's menu item on the selected books, with the library
    /// behind `lib`. Progress goes to `lib` as it happens.
    pub fn run_action(
        &self,
        name: &str,
        item: &str,
        selection: &[BookId],
        lib: Arc<dyn LibraryAccess>,
    ) -> Result<ActionReport, String> {
        let plugin = self
            .library_actions()
            .into_iter()
            .find(|p| p.manifest.name == name)
            .ok_or_else(|| format!("{name} isn't an enabled plugin."))?;
        if !plugin.handles.menu.iter().any(|m| m.id == item) {
            return Err(format!("{name} has no menu item {item:?}."));
        }
        let component = self.component(&plugin)?;
        let sandbox = self.sandbox()?;
        let timeout = self.call_timeout * ACTION_TIMEOUTS;
        let (mut store, instance) = self.library_action(&component, name, Some(&sandbox), Some(lib), timeout)?;
        let ids: Vec<u64> = selection.iter().map(|&id| id as u64).collect();
        let answer = instance.magnitide_plugin_library_action().call_run(&mut store, item, &ids);
        let state = store.data_mut();
        let lines = std::mem::take(&mut state.log);
        let updated = std::mem::take(&mut state.touched);
        drop(store);
        let result = match answer {
            Ok(Ok(summary)) => Ok(ActionReport { summary, updated }),
            Ok(Err(e)) => Err(e),
            Err(e) => Err(describe_trap(&e)),
        };
        self.note(name, lines, result.as_ref().err().map(String::as_str));
        result
    }

    fn library_action(
        &self,
        component: &Component,
        name: &str,
        sandbox: Option<&Sandbox>,
        library: Option<Arc<dyn LibraryAccess>>,
        timeout: Duration,
    ) -> Result<(Store<State>, LibraryActionPlugin), String> {
        let mut linker = self.linker()?;
        LibraryActionPlugin::add_to_linker::<_, HasSelf<_>>(&mut linker, |s: &mut State| s)
            .map_err(|e| e.to_string())?;
        let mut store = self.store(name, &self.capabilities_of(name), sandbox, library, timeout)?;
        let instance = LibraryActionPlugin::instantiate(&mut store, component, &linker)
            .map_err(|e| format!("{name} doesn't implement the library-action API: {}", first_line(&e.to_string())))?;
        Ok((store, instance))
    }
}

fn validate(m: &Manifest) -> Result<(), String> {
    if m.name.trim().is_empty() {
        return Err("The plugin's manifest has no name.".into());
    }
    if m.api != API_VERSION {
        return Err(format!(
            "{} was built for plugin API {}, and this Magnitide provides {API_VERSION}.",
            m.name, m.api
        ));
    }
    if m.component.contains("..") || m.component.starts_with('/') {
        return Err("The plugin's manifest names a component outside the zip.".into());
    }
    Ok(())
}

fn read_zip_text<R: std::io::Read + std::io::Seek>(zip: &mut zip::ZipArchive<R>, name: &str) -> Option<String> {
    let mut f = zip.by_name(name).ok()?;
    let mut s = String::new();
    std::io::Read::read_to_string(&mut f, &mut s).ok()?;
    Some(s)
}

fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or_default().to_owned()
}

/// A trap, the deadline or a panic, as a message for the user.
fn describe_trap(e: &wasmtime::Error) -> String {
    use wasmtime::Trap;

    match e.downcast_ref::<Trap>() {
        Some(Trap::Interrupt) => "the plugin took too long and was stopped".into(),
        Some(Trap::UnreachableCodeReached) => "the plugin crashed".into(),
        Some(trap) => format!("the plugin crashed: {trap}"),
        None if e.downcast_ref::<wasmtime_wasi::I32Exit>().is_some() => "the plugin exited".into(),
        None => format!("the plugin failed: {}", first_line(&e.to_string())),
    }
}

/// `Open Library!` → `open-library`, made unique among `taken`.
fn unique_slug(name: &str, taken: &[&str]) -> String {
    let mut slug: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if slug.is_empty() {
        slug = "plugin".into();
    }
    let base = slug.clone();
    let mut n = 1;
    while taken.contains(&slug.as_str()) {
        n += 1;
        slug = format!("{base}-{n}");
    }
    slug
}

fn to_record(r: types::BookRecord, source_name: &str) -> SourceRecord {
    SourceRecord {
        source: None,
        source_name: source_name.to_owned(),
        source_key: r.source_key,
        link: r.link,
        title: r.title,
        subtitle: r.subtitle,
        authors: r.authors,
        publisher: r.publisher,
        published: r.published.map(|d| if d.len() == 4 { format!("{d}-01-01") } else { d }),
        languages: r.languages,
        identifiers: r.identifiers.into_iter().map(|i| (i.key, i.value)).collect(),
        tags: r.tags,
        description_html: r.description_html,
        cover_url: r.cover_url,
        pages: r.pages,
        series: r.series.filter(|s| !s.trim().is_empty()),
        series_index: r.series_index.filter(|i| i.is_finite()),
        has_details: false,
    }
}

fn from_wit_metadata(m: types::BookMetadata) -> BookMetadata {
    BookMetadata {
        title: m.title.filter(|t| !t.trim().is_empty()),
        authors: m.authors.into_iter().map(|a| Author { name: a.name, sort: a.sort }).collect(),
        series: m.series.filter(|s| !s.trim().is_empty()).map(|s| (s, m.series_index.unwrap_or(1.0))),
        publisher: m.publisher,
        published: m.published.as_deref().and_then(magnitide_core::metadata::parse_date),
        languages: m.languages,
        identifiers: m.identifiers.into_iter().map(|i| (i.key, i.value)).collect(),
        description: m.description,
        tags: m.tags,
        cover: m.cover.filter(|c| !c.is_empty()),
        uuids: m.uuids.into_iter().map(|u| u.to_lowercase()).collect(),
    }
}

fn to_wit_metadata(m: &BookMetadata) -> types::BookMetadata {
    types::BookMetadata {
        title: m.title.clone(),
        authors: m.authors.iter().map(|a| types::Author { name: a.name.clone(), sort: a.sort.clone() }).collect(),
        series: m.series.as_ref().map(|(s, _)| s.clone()),
        series_index: m.series.as_ref().map(|(_, i)| *i),
        publisher: m.publisher.clone(),
        published: m.published.map(|d| d.to_string()),
        languages: m.languages.clone(),
        identifiers: m
            .identifiers
            .iter()
            .map(|(k, v)| types::Identifier { key: k.clone(), value: v.clone() })
            .collect(),
        description: m.description.clone(),
        tags: m.tags.clone(),
        cover: m.cover.clone(),
        uuids: m.uuids.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs() {
        assert_eq!(unique_slug("Open Library!", &[]), "open-library");
        assert_eq!(unique_slug("Open Library", &["open-library"]), "open-library-2");
        assert_eq!(unique_slug("!!!", &[]), "plugin");
    }

    #[test]
    fn manifests_are_checked() {
        let m: Manifest = toml::from_str(
            r#"name = "X"
version = "1.0"
api = "0.1"
kind = "metadata-source"
capabilities = ["network"]"#,
        )
        .unwrap();
        assert_eq!(
            (m.kind, m.component.as_str(), m.capabilities.as_slice()),
            (Kind::MetadataSource, "plugin.wasm", &[Capability::Network][..])
        );
        validate(&m).unwrap();
        let old = Manifest { api: "0.0".into(), ..m.clone() };
        assert!(validate(&old).unwrap_err().contains("built for plugin API 0.0"));
        let bad = Manifest { component: "../x.wasm".into(), ..m };
        assert!(validate(&bad).is_err());
    }

    #[test]
    fn records_convert() {
        let r = to_record(
            types::BookRecord {
                source_key: "k".into(),
                link: None,
                title: "T".into(),
                subtitle: None,
                authors: vec!["A".into()],
                publisher: None,
                published: Some("1965".into()),
                languages: vec![],
                identifiers: vec![types::Identifier { key: "isbn".into(), value: "9780441013593".into() }],
                tags: vec![],
                description_html: None,
                cover_url: None,
                pages: Some(412),
                series: Some("Dune".into()),
                series_index: Some(1.0),
            },
            "S",
        );
        assert_eq!((r.source_name.as_str(), r.published.as_deref(), r.pages), ("S", Some("1965-01-01"), Some(412)));
        assert_eq!(r.identifiers.get("isbn").map(String::as_str), Some("9780441013593"));
    }
}
