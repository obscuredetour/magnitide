#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod covers;
mod devices;
mod lookup;
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
    /// Connected e-readers, as last scanned (see devices.rs).
    devices: RwLock<Vec<Arc<magnitide_core::devices::Scan>>>,
}

impl AppState {
    pub fn current(&self) -> Option<Arc<Loaded>> {
        self.lib.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn replace(&self, loaded: Loaded) {
        *self.lib.write().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(loaded));
    }

    pub fn devices(&self) -> Vec<Arc<magnitide_core::devices::Scan>> {
        self.devices.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn set_devices(&self, scans: Vec<Arc<magnitide_core::devices::Scan>>) {
        *self.devices.write().unwrap_or_else(|e| e.into_inner()) = scans;
    }

    /// A connected device by id (its mount point).
    pub fn device(&self, id: &str) -> Result<Arc<magnitide_core::devices::Scan>, String> {
        self.devices()
            .into_iter()
            .find(|s| s.device.root.display().to_string() == id)
            .ok_or_else(|| "That device isn't connected any more.".to_owned())
    }
}

fn main() {
    let builder = tauri::Builder::default();
    #[cfg(target_os = "macos")]
    let builder = builder.menu(menu::build).on_menu_event(menu::handle);

    builder
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .setup(|app| {
            devices::watch(app.handle().clone());
            Ok(())
        })
        .register_asynchronous_uri_scheme_protocol("cover", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            let path = request.uri().path().to_owned();
            tauri::async_runtime::spawn_blocking(move || responder.respond(covers::respond(&app, &path)));
        })
        .invoke_handler(tauri::generate_handler![
            commands::open_library,
            commands::list_books,
            commands::book_rows,
            commands::book_details,
            commands::open_book,
            commands::reveal_book,
            commands::show_library_folder,
            commands::thumbnail_cache_info,
            commands::clear_thumbnail_cache,
            commands::import_calibre_library,
            commands::rename_library,
            commands::add_books,
            commands::book_edit,
            commands::author_sorts,
            commands::bulk_values,
            commands::bulk_edit,
            commands::save_book_edit,
            commands::set_cover_from_file,
            commands::remove_cover,
            commands::tidy_preview,
            commands::tidy_file_names,
            commands::name_lists,
            commands::file_metadata_status,
            commands::write_metadata_to_files,
            commands::writeback_preview,
            commands::write_all_metadata,
            lookup::metadata_sources,
            lookup::set_metadata_source,
            lookup::find_metadata,
            lookup::metadata_details,
            lookup::cover_preview,
            lookup::set_cover_from_url,
            devices::list_devices,
            devices::rescan_device,
            devices::reveal_device,
            devices::eject_device,
            devices::send_to_device,
            devices::remove_from_device,
            devices::import_from_device,
            commands::merge_books,
            commands::conversion_info,
            commands::convert_targets,
            commands::convert_books
        ])
        .run(tauri::generate_context!())
        .expect("error while running Magnitide");
}
