//! The library server (Settings › Library Server): the open library, read-only,
//! on the local network, as a web page for e-reader browsers and an OPDS
//! catalogue for reading apps, plus KOReader's progress sync (reading
//! positions, kept outside the library). Off until the user turns it on; the setting is
//! remembered, and the server follows whichever library is open.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use magnitide_catalog::Catalog;
use magnitide_server::{Config, Served, Server, Source};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use ts_rs::TS;

use crate::commands::CmdResult;
use crate::{AppState, Loaded};

/// Not 8080, which calibre's own server and many dev tools use.
const DEFAULT_PORT: u16 = 8180;

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase", default)]
struct Settings {
    enabled: bool,
    port: u16,
    password: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self { enabled: false, port: DEFAULT_PORT, password: None }
    }
}

/// What Settings shows.
#[derive(Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ServerStatus {
    enabled: bool,
    running: bool,
    port: u16,
    has_password: bool,
    /// The web page's address on this network ("http://192.168.1.20:8180/").
    url: Option<String>,
    /// Further addresses, when this computer is on more than one network.
    other_urls: Vec<String>,
    /// The same as a QR code (SVG), for a phone's camera.
    qr_svg: Option<String>,
    /// A VPN is up; it may block devices on the LAN from connecting.
    vpn: bool,
    /// Why it isn't running although enabled (port taken, no network…).
    error: Option<String>,
}

#[derive(Default)]
pub struct ServerState {
    server: Option<Server>,
    error: Option<String>,
}

impl Served for Loaded {
    fn name(&self) -> &str {
        &self.name
    }
    fn root(&self) -> &Path {
        &self.root
    }
    fn catalog(&self) -> &Catalog {
        &self.catalog
    }
}

/// Serves whatever library the app has open at the time of each request.
struct Open(AppHandle);

impl Source for Open {
    fn current(&self) -> Option<Arc<dyn Served>> {
        self.0.state::<AppState>().current().map(|l| l as Arc<dyn Served>)
    }
}

fn settings_path(app: &AppHandle) -> CmdResult<PathBuf> {
    Ok(app.path().app_config_dir().map_err(|e| e.to_string())?.join("server.json"))
}

fn load_settings(app: &AppHandle) -> Settings {
    settings_path(app)
        .ok()
        .and_then(|p| fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save_settings(app: &AppHandle, s: &Settings) -> CmdResult<()> {
    let path = settings_path(app)?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    fs::write(&path, serde_json::to_vec_pretty(s).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

/// Stop the server, then start it again if the settings say so.
fn apply(app: &AppHandle, s: &Settings) {
    let state = app.state::<AppState>();
    let mut server = state.server.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(old) = server.server.take() {
        old.stop();
    }
    server.error = None;
    if !s.enabled {
        return;
    }
    let cache_dir = match app.path().app_cache_dir() {
        Ok(d) => d.join("server"),
        Err(e) => {
            server.error = Some(e.to_string());
            return;
        }
    };
    // Reading positions from KOReader's progress sync: the app's, not any library's.
    let sync_db = app.path().app_data_dir().ok().map(|d| d.join("reading-positions.sqlite"));
    let config = Config { port: s.port, password: s.password.clone(), cache_dir, sync_db };
    match Server::start(config, Arc::new(Open(app.clone()))) {
        Ok(started) => server.server = Some(started),
        Err(e) => server.error = Some(format!("The server couldn't start: {e}.")),
    }
}

/// Start the server at launch if it was left on.
pub fn start_if_enabled(app: &AppHandle) {
    let s = load_settings(app);
    if s.enabled {
        apply(app, &s);
    }
}

fn qr_svg(text: &str) -> Option<String> {
    let code = qrcode::QrCode::new(text.as_bytes()).ok()?;
    Some(
        code.render::<qrcode::render::svg::Color>()
            .min_dimensions(160, 160)
            .dark_color(qrcode::render::svg::Color("#000000"))
            .light_color(qrcode::render::svg::Color("#ffffff"))
            .build(),
    )
}

fn status(app: &AppHandle) -> ServerStatus {
    let s = load_settings(app);
    let state = app.state::<AppState>();
    let server = state.server.lock().unwrap_or_else(|e| e.into_inner());
    let port = server.server.as_ref().map_or(s.port, Server::port);
    let running = server.server.is_some();
    let mut error = server.error.clone();
    let found = if running { magnitide_server::lan_addresses() } else { Default::default() };
    let mut urls = found.lan.iter().map(|ip| format!("http://{ip}:{port}/"));
    let url = urls.next();
    let other_urls = urls.collect();
    if running && url.is_none() {
        error = Some("This computer doesn't seem to be on a local network.".into());
    }
    ServerStatus {
        enabled: s.enabled,
        running,
        port,
        has_password: s.password.as_deref().is_some_and(|p| !p.is_empty()),
        qr_svg: url.as_deref().and_then(qr_svg),
        url,
        other_urls,
        vpn: found.tunnel,
        error,
    }
}

#[tauri::command]
pub async fn server_status(app: AppHandle) -> CmdResult<ServerStatus> {
    Ok(status(&app))
}

/// Change the server's settings and (re)start or stop it. `password`: None
/// keeps the current one, "" removes it.
#[tauri::command]
pub async fn set_server(app: AppHandle, enabled: bool, port: u16, password: Option<String>) -> CmdResult<ServerStatus> {
    if port < 1024 {
        return Err("Pick a port from 1024 to 65535.".into());
    }
    let mut s = load_settings(&app);
    s.enabled = enabled;
    s.port = port;
    if let Some(p) = password {
        s.password = Some(p).filter(|p| !p.is_empty());
    }
    save_settings(&app, &s)?;
    let app2 = app.clone();
    // Stopping waits for requests in flight (a large download, say).
    tauri::async_runtime::spawn_blocking(move || apply(&app2, &s)).await.map_err(|e| e.to_string())?;
    Ok(status(&app))
}
