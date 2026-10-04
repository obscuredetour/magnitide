//! macOS menu bar: Tauri's default menu plus "Settings… ⌘," in the app menu,
//! where macOS apps keep it. (Windows and Linux have no menu bar; the UI
//! handles Ctrl+, itself there.)

use tauri::menu::{Menu, MenuEvent, MenuItem, MenuItemKind, PredefinedMenuItem};
use tauri::{AppHandle, Emitter, Runtime};

const SETTINGS_ID: &str = "settings";

/// Event telling the UI to open the settings dialog. The UI listens for this
/// name in `ui/src/api.ts` (`onOpenSettings`).
pub const OPEN_SETTINGS_EVENT: &str = "open-settings";

pub fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let menu = Menu::default(app)?;
    let settings = MenuItem::with_id(app, SETTINGS_ID, "Settings…", true, Some("CmdOrCtrl+,"))?;
    // The default app menu starts: About, separator. Settings goes next, in
    // its own group, as in other macOS apps.
    if let Some(MenuItemKind::Submenu(app_menu)) = menu.items()?.into_iter().next() {
        app_menu.insert(&settings, 2)?;
        app_menu.insert(&PredefinedMenuItem::separator(app)?, 3)?;
    }
    Ok(menu)
}

pub fn handle<R: Runtime>(app: &AppHandle<R>, event: MenuEvent) {
    if event.id() == SETTINGS_ID {
        // Nothing useful to do if the UI isn't listening yet.
        let _ = app.emit(OPEN_SETTINGS_EVENT, ());
    }
}
