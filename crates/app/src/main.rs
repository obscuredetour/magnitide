#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod covers;
mod custom;
mod devices;
mod keys;
mod lookup;
#[cfg(target_os = "macos")]
mod menu;
mod plugins;
mod reader;
mod server;
mod themes;
mod updates;

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
    /// The library's name (a calibre library's is its folder's).
    pub name: String,
    pub kind: LibraryKind,
    /// Every book, loaded once; search, sort and all views read this.
    pub catalog: Catalog,
    /// Magnitide libraries stay open (holding their single-writer lock) for as
    /// long as they're shown; edits go through this. None for calibre libraries.
    pub native: Option<Arc<Mutex<magnitide_core::Library>>>,
    /// Where this library's cover thumbnails are cached (never inside the library).
    pub thumb_dir: PathBuf,
}

impl Loaded {
    /// The same library, showing `catalog` (just reloaded from its database).
    pub fn with_catalog(&self, catalog: Catalog) -> Self {
        Self {
            root: self.root.clone(),
            name: self.name.clone(),
            kind: self.kind,
            catalog,
            native: self.native.clone(),
            thumb_dir: self.thumb_dir.clone(),
        }
    }
}

#[derive(Default)]
pub struct AppState {
    lib: RwLock<Option<Arc<Loaded>>>,
    /// Connected e-readers, as last scanned (see devices.rs).
    devices: RwLock<Vec<Arc<magnitide_core::devices::Scan>>>,
    /// The library server, when it's on (see server.rs).
    server: Mutex<server::ServerState>,
}

impl AppState {
    pub fn current(&self) -> Option<Arc<Loaded>> {
        self.lib.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn replace(&self, loaded: Loaded) {
        *self.lib.write().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(loaded));
    }

    /// Close the open library, returning it (if one was open).
    pub fn close(&self) -> Option<Arc<Loaded>> {
        self.lib.write().unwrap_or_else(|e| e.into_inner()).take()
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
    // GTK names the window after the program (Wayland app_id, X11 WM_CLASS), and desktops find
    // its icon through the desktop entry of that name. The bundles ship "Magnitide.desktop",
    // after productName, so the window takes that name rather than the binary's "magnitide".
    #[cfg(target_os = "linux")]
    glib::set_prgname(Some("Magnitide"));

    let builder = tauri::Builder::default();
    #[cfg(target_os = "macos")]
    let builder = builder.menu(menu::build).on_menu_event(menu::handle);

    builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(covers::allow_on_http())
        .manage(AppState::default())
        .manage(reader::Reader::default())
        .manage(lookup::ReturnedCovers::default())
        .manage(updates::Pending::default())
        .setup(|app| {
            // The sources' keys and the server's password: in the keychain,
            // where any still in their settings files move on this launch
            // (off the main thread: it may ask the user to unlock a
            // keyring). The server starts once its password is known.
            let secrets = keys::Secrets::new(vec![
                keys::Registered::file::<magnitide_sources::Settings>(lookup::settings_path(app.handle())),
                keys::Registered::file::<server::Settings>(server::settings_path(app.handle())),
            ]);
            tauri::Manager::manage(app, secrets);
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                tauri::Manager::state::<keys::Secrets>(&handle).warm();
                server::start_if_enabled(&handle);
            });
            devices::watch(app.handle().clone());
            tauri::Manager::manage(app, plugins::create(app.handle()));
            tauri::Manager::manage(app, plugins::create_native(app.handle()));
            Ok(())
        })
        .register_asynchronous_uri_scheme_protocol("cover", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            let path = request.uri().path().to_owned();
            tauri::async_runtime::spawn_blocking(move || responder.respond(covers::respond(&app, &path)));
        })
        // The comic open in the viewer, a page at a time (see reader.rs).
        .register_asynchronous_uri_scheme_protocol("page", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            let path = request.uri().path().to_owned();
            tauri::async_runtime::spawn_blocking(move || responder.respond(reader::respond(&app, &path)));
        })
        .invoke_handler(tauri::generate_handler![
            reader::open_comic,
            reader::close_comic,
            reader::save_reading_progress,
            commands::open_library,
            commands::close_library,
            commands::create_library,
            commands::library_kind_at,
            commands::scan_drop,
            commands::import_calibre_into_library,
            commands::list_books,
            custom::custom_fields,
            custom::add_custom_field,
            custom::update_custom_field,
            custom::delete_custom_field,
            custom::ensure_comic_fields,
            custom::convert_cbr_on_add,
            custom::set_convert_cbr_on_add,
            custom::rar_tool,
            commands::book_rows,
            commands::book_details,
            commands::open_book,
            commands::reveal_book,
            commands::show_library_folder,
            commands::thumbnail_cache_info,
            commands::clear_thumbnail_cache,
            covers::cover_colors,
            commands::library_stats,
            commands::import_calibre_library,
            commands::rename_library,
            commands::add_books,
            commands::book_edit,
            commands::author_sorts,
            commands::bulk_values,
            commands::bulk_fill,
            commands::bulk_edit,
            commands::save_book_edit,
            commands::set_cover_from_file,
            commands::remove_cover,
            commands::remove_format,
            commands::delete_books,
            commands::list_shelves,
            commands::create_shelf,
            commands::rename_shelf,
            commands::delete_shelf,
            commands::add_to_shelf,
            commands::make_shelves,
            commands::facets,
            commands::merge_values,
            commands::split_tags,
            commands::remove_values,
            commands::remove_from_shelf,
            devices::sync_device_shelves,
            themes::theme_addons,
            themes::add_theme_file,
            themes::open_themes_folder,
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
            commands::convert_books,
            server::server_status,
            server::set_server,
            plugins::plugins_status,
            plugins::install_native_plugin,
            plugins::remove_native_plugin,
            plugins::set_native_plugin_enabled,
            plugins::native_plugin_log,
            plugins::native_actions,
            plugins::run_native_action,
            plugins::answer_native_question,
            plugins::install_plugin,
            plugins::remove_plugin,
            plugins::set_plugin_enabled,
            plugins::plugin_log,
            plugins::gui_plugin_names,
            plugins::plugin_menus,
            plugins::run_plugin,
            plugins::plugin_answer,
            plugins::configure_plugin,
            plugins::set_plugin_output_format,
            plugins::set_plugin_host,
            plugins::install_plugin_calibre,
            plugins::remove_plugin_calibre,
            plugins::estuary_status,
            plugins::install_estuary,
            plugins::install_estuary_from_file,
            plugins::cancel_plugin_host_install,
            plugins::remove_estuary,
            updates::update_status,
            updates::set_auto_update,
            updates::check_for_update,
            updates::install_update,
            updates::open_release_notes
        ])
        .build(tauri::generate_context!())
        .expect("error while running Magnitide")
        .run(|app, event| {
            if let tauri::RunEvent::ExitRequested { .. } = event {
                // Library-closed plugins run, and the plugin host shuts down with us.
                plugins::library_closing(app);
            }
        });
}
