//! Magnitide's own plugins: WebAssembly components on the `magnitide:plugin`
//! API (crates/plugin-sdk/wit), run in wasmtime with no ambient filesystem
//! or network. What a plugin may do is declared in its manifest
//! (`capabilities`) and enforced by which host functions do anything.
//!
//! A plugin is a `.mgp` zip holding `manifest.toml` and the component it
//! names, installed into `<dir>/<slug>/`. Each call gets a fresh store with
//! a memory cap and a deadline, so a misbehaving plugin is an error for
//! that call, not a crash.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use magnitide_sources::{Lookup, SourceRecord};
use serde::{Deserialize, Serialize};
use wasmtime::component::{Component, HasSelf, Linker, ResourceTable};
use wasmtime::{Engine, Store, StoreLimits, StoreLimitsBuilder};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

mod bindings {
    wasmtime::component::bindgen!({
        world: "metadata-source-plugin",
        path: "../plugin-sdk/wit",
    });
}

use bindings::MetadataSourcePlugin;
use bindings::magnitide::plugin::{http, log, types};

/// The API version this host provides; a plugin's manifest names the one it targets.
pub const API_VERSION: &str = "0.1";

/// Memory a plugin may use per call.
const MEMORY_LIMIT: usize = 256 * 1024 * 1024;
/// Wall-clock allowed per call unless set otherwise (epoch ticks are every 100 ms).
const CALL_TIMEOUT: Duration = Duration::from_secs(60);
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
}

/// What kind of plugin, by the world it implements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, rename = "NativeKind"))]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    MetadataSource,
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

/// What a plugin sees of the host during one call.
struct State {
    wasi: WasiCtx,
    table: ResourceTable,
    limits: StoreLimits,
    plugin: String,
    capabilities: Vec<Capability>,
    log: Vec<String>,
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
        // It must implement the world: instantiate once, with no call.
        self.instantiate(&component, &manifest.name, &[]).map(|_| ())?;

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
        let (mut store, instance) =
            self.instantiate(&component, &plugin.manifest.name, &plugin.manifest.capabilities)?;
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
        let (mut store, instance) =
            self.instantiate(&component, &plugin.manifest.name, &plugin.manifest.capabilities)?;
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

    fn instantiate(
        &self,
        component: &Component,
        name: &str,
        capabilities: &[Capability],
    ) -> Result<(Store<State>, MetadataSourcePlugin), String> {
        let mut linker = Linker::new(&self.engine);
        wasmtime_wasi::p2::add_to_linker_sync(&mut linker).map_err(|e| e.to_string())?;
        MetadataSourcePlugin::add_to_linker::<_, HasSelf<_>>(&mut linker, |s: &mut State| s)
            .map_err(|e| e.to_string())?;
        let state = State {
            // No preopened directories, no network, no environment: the sandbox.
            wasi: WasiCtxBuilder::new().build(),
            table: ResourceTable::new(),
            limits: StoreLimitsBuilder::new().memory_size(MEMORY_LIMIT).build(),
            plugin: name.to_owned(),
            capabilities: capabilities.to_vec(),
            log: Vec::new(),
        };
        let mut store = Store::new(&self.engine, state);
        store.limiter(|s| &mut s.limits);
        store.set_epoch_deadline((self.call_timeout.as_millis() / EPOCH_TICK.as_millis()).max(1) as u64);
        let instance = MetadataSourcePlugin::instantiate(&mut store, component, &linker)
            .map_err(|e| format!("{name} doesn't implement the metadata-source API: {}", first_line(&e.to_string())))?;
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
        has_details: false,
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
            },
            "S",
        );
        assert_eq!((r.source_name.as_str(), r.published.as_deref(), r.pages), ("S", Some("1965-01-01"), Some(412)));
        assert_eq!(r.identifiers.get("isbn").map(String::as_str), Some("9780441013593"));
    }
}
