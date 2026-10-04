#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod covers;
#[cfg(target_os = "macos")]
mod menu;

use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use magnitide_catalog::Catalog;
use serde::Serialize;
use ts_rs::TS;

/// What kind of library is open. calibre libraries are only ever read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "lowercase")]
pub enum LibraryKind {
    Magnitide,
    Calibre,
}

/// The open library. Swapped wholesale when another library is opened.
pub struct Loaded {
    pub root: PathBuf,
    pub kind: LibraryKind,
    /// Every book, loaded once; search, sort and all views read this.
    pub catalog: Catalog,
    /// Magnitide libraries stay open (holding their single-writer lock) for as
    /// long as they're shown; edits go through this. None for calibre libraries.
    pub native: Option<Arc<Mutex<magnitide_core::Library>>>,
    /// Where this library's cover thumbnails are cached (never inside the library).
    pub thumb_dir: PathBuf,
}

#[derive(Default)]
pub struct AppState {
    lib: RwLock<Option<Arc<Loaded>>>,
}

impl AppState {
    pub fn current(&self) -> Option<Arc<Loaded>> {
        self.lib.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn replace(&self, loaded: Loaded) {
        *self.lib.write().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(loaded));
    }
}

fn main() {
    let builder = tauri::Builder::default();
    #[cfg(target_os = "macos")]
    let builder = builder.menu(menu::build).on_menu_event(menu::handle);

    builder
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .register_asynchronous_uri_scheme_protocol("cover", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            let path = request.uri().path().to_owned();
            tauri::async_runtime::spawn_blocking(move || responder.respond(covers::respond(&app, &path)));
        })
        .invoke_handler(tauri::generate_handler![
            commands::open_library,
            commands::list_books,
            commands::book_details,
            commands::open_book,
            commands::reveal_book,
            commands::show_library_folder,
            commands::thumbnail_cache_info,
            commands::clear_thumbnail_cache,
            commands::import_calibre_library,
            commands::rename_library
        ])
        .run(tauri::generate_context!())
        .expect("error while running Magnitide");
}
