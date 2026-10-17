//! The library server (Settings › Library Server): the open library, read-only,
//! on the local network, as a web page for e-reader browsers and an OPDS
//! catalogue for reading apps, plus KOReader's progress sync (reading
//! positions, kept outside the library). Off until the user turns it on; the setting is
//! remembered, and the server follows whichever library is open.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use magnitide_catalog::Catalog;
use magnitide_server::{Config, Served, Server, Source};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use ts_rs::TS;

use crate::commands::CmdResult;
use crate::keys::{KeyStorage, SecretFile, Secrets};
use crate::{AppState, Loaded};

/// Not 8080, which calibre's own server and many dev tools use.
const DEFAULT_PORT: u16 = 8180;

/// The keychain account the password is filed under (see keys.rs).
pub const SERVER_PASSWORD: &str = "server-password";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub enabled: bool,
    pub port: u16,
    /// In the keychain; here only where there's no keychain.
    pub password: Option<String>,
}

impl SecretFile for Settings {
    fn accounts() -> Vec<&'static str> {
        vec![SERVER_PASSWORD]
    }

    fn secrets(&self) -> Vec<(&'static str, String)> {
        self.password.iter().filter(|p| !p.is_empty()).map(|p| (SERVER_PASSWORD, p.clone())).collect()
    }

    fn set_secret(&mut self, account: &str, secret: Option<&str>) {
        if account == SERVER_PASSWORD {
            self.password = secret.map(str::trim).filter(|p| !p.is_empty()).map(str::to_owned);
        }
    }
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
    /// Where the password is kept: the system keychain, or the settings
    /// file when no keychain is reachable.
    password_storage: KeyStorage,
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

/// The server's settings file: on or off, the port, and only where there's
/// no keychain, the password (see keys.rs).
pub fn settings_path(app: &AppHandle) -> PathBuf {
    app.path().app_config_dir().unwrap_or_default().join("server.json")
}

/// The settings, with the password from wherever it's kept. May reach the
/// keychain: call from a blocking task.
fn load_settings(app: &AppHandle) -> Settings {
    app.state::<Secrets>().load(&settings_path(app))
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

/// Start the server at launch if it was left on. Off the main thread, once
/// the keychain has been read (see main).
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
        password_storage: app.state::<Secrets>().storage(),
        qr_svg: url.as_deref().and_then(qr_svg),
        url,
        other_urls,
        vpn: found.tunnel,
        error,
    }
}

#[tauri::command]
pub async fn server_status(app: AppHandle) -> CmdResult<ServerStatus> {
    tauri::async_runtime::spawn_blocking(move || status(&app)).await.map_err(|e| e.to_string())
}

/// Change the server's settings and (re)start or stop it. `password`: None
/// keeps the current one, "" removes it.
#[tauri::command]
pub async fn set_server(app: AppHandle, enabled: bool, port: u16, password: Option<String>) -> CmdResult<ServerStatus> {
    if port < 1024 {
        return Err("Pick a port from 1024 to 65535.".into());
    }
    // Stopping waits for requests in flight (a large download, say).
    tauri::async_runtime::spawn_blocking(move || {
        let secrets = app.state::<Secrets>();
        let path = settings_path(&app);
        let mut s: Settings = secrets.load(&path);
        s.enabled = enabled;
        s.port = port;
        if let Some(p) = password {
            secrets.set_secret(&mut s, SERVER_PASSWORD, &p)?;
        }
        secrets.save(&path, &s)?;
        apply(&app, &s);
        Ok(status(&app))
    })
    .await
    .map_err(|e| e.to_string())?
}
