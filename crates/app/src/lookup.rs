//! Metadata lookup ("Find Metadata"): searching online sources (built-in,
//! Magnitide's own metadata-source plugins and calibre ones), previewing
//! their covers, and the
//! per-user source settings. The network is only used
//! from these commands, which only run when the user asks; the webview never
//! contacts the sources itself (covers are proxied through `cover://remote/`).

use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::Cursor;
use std::path::PathBuf;

use magnitide_plugins::Plugins;
use magnitide_plugins::sources::is_plugin_cover;
use magnitide_plugins_wasm::WasmPlugins;
use magnitide_sources::{Lookup, SearchResult, Settings, SourceId, SourceRecord};
use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use ts_rs::TS;

use crate::AppState;
use crate::commands::{CmdResult, sanitize, thumbs_root, write_library};

/// A source as Settings shows it.
#[derive(Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SourceStatus {
    id: SourceId,
    name: String,
    /// The user hasn't turned it off.
    enabled: bool,
    needs_key: bool,
    has_key: bool,
    /// Will be asked: enabled, and keyed if it needs one.
    active: bool,
}

/// A cover candidate, downloaded and ready to show at `cover://localhost/remote/<key>`.
#[derive(Serialize, TS)]
#[ts(export)]
pub struct CoverPreview {
    key: String,
    width: u32,
    height: u32,
}

fn settings_path(app: &AppHandle) -> CmdResult<PathBuf> {
    Ok(app.path().app_config_dir().map_err(|e| e.to_string())?.join("metadata-sources.json"))
}

fn load_settings(app: &AppHandle) -> Settings {
    settings_path(app)
        .ok()
        .and_then(|p| fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn statuses(s: &Settings) -> Vec<SourceStatus> {
    SourceId::ALL
        .into_iter()
        .map(|id| SourceStatus {
            id,
            name: id.name().to_owned(),
            enabled: !s.disabled.contains(&id),
            needs_key: id.needs_key(),
            has_key: s.key(id).is_some(),
            active: s.enabled(id),
        })
        .collect()
}

#[tauri::command]
pub async fn metadata_sources(app: AppHandle) -> CmdResult<Vec<SourceStatus>> {
    Ok(statuses(&load_settings(&app)))
}

/// Turn a source on or off; `api_key` replaces its key when given ("" clears it).
#[tauri::command]
pub async fn set_metadata_source(
    app: AppHandle,
    id: SourceId,
    enabled: bool,
    api_key: Option<String>,
) -> CmdResult<Vec<SourceStatus>> {
    let mut s = load_settings(&app);
    s.disabled.retain(|d| *d != id);
    if !enabled {
        s.disabled.push(id);
    }
    if let Some(key) = api_key {
        let key = Some(key.trim().to_owned()).filter(|k| !k.is_empty());
        match id {
            SourceId::Google => s.google_key = key,
            SourceId::OpenLibrary => {}
        }
    }
    let path = settings_path(&app)?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_vec_pretty(&s).map_err(|e| e.to_string())?;
    fs::write(&path, json).map_err(|e| e.to_string())?;
    Ok(statuses(&s))
}

/// Search every active source at once: the built-in ones, Magnitide's own
/// plugins and calibre plugins. Descriptions come back sanitized.
#[tauri::command]
pub async fn find_metadata(app: AppHandle, lookup: Lookup) -> CmdResult<SearchResult> {
    let settings = load_settings(&app);
    let any_plugin = !app.state::<Plugins>().metadata_sources().is_empty()
        || !app.state::<WasmPlugins>().metadata_sources().is_empty();
    if !SourceId::ALL.iter().any(|s| settings.enabled(*s)) && !any_plugin {
        return Err("Every metadata source is turned off. Turn one on in Settings → Metadata Sources.".into());
    }
    let app2 = app.clone();
    let mut result = tauri::async_runtime::spawn_blocking(move || {
        let plugins = app2.state::<Plugins>();
        let native = app2.state::<WasmPlugins>();
        let ((mut records, mut errors), from_plugins, from_native) = std::thread::scope(|scope| {
            let plugins = scope.spawn(|| plugins.identify(&lookup));
            let native = scope.spawn(|| native.identify(&lookup));
            let built_in = magnitide_sources::search_records(&lookup, &settings);
            (built_in, plugins.join().unwrap_or_default(), native.join().unwrap_or_default())
        });
        for (name, answer) in from_native.into_iter().chain(from_plugins) {
            match answer {
                Ok(r) => records.extend(r),
                Err(e) => errors.push(format!("{name}: {e}")),
            }
        }
        magnitide_sources::rank(&lookup, records, errors)
    })
    .await
    .map_err(|e| e.to_string())?;
    for m in &mut result.matches {
        m.records.iter_mut().for_each(clean);
    }
    Ok(result)
}

/// The rest of a record (Open Library's description), sanitized.
#[tauri::command]
pub async fn metadata_details(record: SourceRecord) -> CmdResult<SourceRecord> {
    let mut full = tauri::async_runtime::spawn_blocking(move || magnitide_sources::details(&record))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    clean(&mut full);
    Ok(full)
}

fn clean(r: &mut SourceRecord) {
    r.description_html = r.description_html.as_deref().map(sanitize).filter(|d| !d.trim().is_empty());
}

/// Download a source's cover into the cache so the UI can show it.
#[tauri::command]
pub async fn cover_preview(app: AppHandle, url: String) -> CmdResult<CoverPreview> {
    let dir = remote_dir(&app)?;
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (key, bytes) = cached_cover(&app, &dir, &url)?;
        let (width, height) = image::ImageReader::new(Cursor::new(&bytes))
            .with_guessed_format()
            .map_err(|e| e.to_string())?
            .into_dimensions()
            .map_err(|_| "The source sent something that isn't an image.".to_owned())?;
        Ok(CoverPreview { key, width, height })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Use a source's cover for a book (applies now, like choosing a file).
#[tauri::command]
pub async fn set_cover_from_url(app: AppHandle, state: State<'_, AppState>, id: i64, url: String) -> CmdResult<()> {
    let dir = remote_dir(&app)?;
    let bytes = tauri::async_runtime::spawn_blocking(move || cached_cover(&app, &dir, &url).map(|(_, b)| b))
        .await
        .map_err(|e| e.to_string())??;
    write_library(&state, move |lib| lib.set_cover(id, &bytes)).await
}

pub fn remote_dir(app: &AppHandle) -> CmdResult<PathBuf> {
    Ok(thumbs_root(app)?.join("remote"))
}

/// The cover at `url`, from the cache or downloaded into it (a plugin's
/// cover, from the plugin). The key is a hash of the URL (only ever hex, so
/// safe as a file name).
fn cached_cover(app: &AppHandle, dir: &std::path::Path, url: &str) -> CmdResult<(String, Vec<u8>)> {
    let mut h = DefaultHasher::new();
    url.hash(&mut h);
    let key = format!("{:016x}", h.finish());
    let path = dir.join(&key);
    if let Ok(bytes) = fs::read(&path) {
        return Ok((key, bytes));
    }
    let bytes = if is_plugin_cover(url) {
        app.state::<Plugins>().cover(url).map_err(|e| format!("Couldn't get the cover: {e}"))?
    } else {
        magnitide_sources::download_cover(url).map_err(|e| format!("Couldn't download the cover: {e}"))?
    };
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let tmp = dir.join(format!(".{key}.tmp"));
    fs::write(&tmp, &bytes).and_then(|()| fs::rename(&tmp, &path)).map_err(|e| e.to_string())?;
    Ok((key, bytes))
}
