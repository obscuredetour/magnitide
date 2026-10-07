//! GUI plugins (calibre's "interface actions"): their menus, and running
//! them on the shadow library ([`crate::shadow`]). The host gives them a
//! stand-in for calibre's main window; their dialogs open as their own
//! windows, as in calibre. When a plugin is done the host sends a
//! `gui-idle` event, and the app reads back what it changed.

use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{Installed, PluginKind, Plugins};

/// Starting Qt and loading plugins the first time can take a while.
const OPEN_TIMEOUT: Duration = Duration::from_secs(120);
const CALL_TIMEOUT: Duration = Duration::from_secs(30);

/// A GUI plugin's menu, as calibre would show it on its toolbar button.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct PluginMenu {
    /// The installed plugin's name.
    pub plugin: String,
    /// The toolbar button's text.
    pub text: String,
    /// The button's icon, as a data: URL.
    #[serde(default)]
    pub icon: Option<String>,
    /// Whether the button does something itself (beyond showing the menu).
    #[serde(default)]
    pub main: bool,
    #[serde(default)]
    pub items: Vec<MenuItem>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct MenuItem {
    #[serde(default)]
    pub separator: bool,
    /// Where it is in the menu: indexes, from the top level down.
    #[serde(default)]
    pub path: Vec<u32>,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub checkable: bool,
    #[serde(default)]
    pub checked: bool,
    #[serde(default)]
    pub children: Vec<MenuItem>,
}

/// The menus of the GUI plugins that loaded, and why any others didn't.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct PluginMenus {
    pub menus: Vec<PluginMenu>,
    /// "Plugin name: reason".
    pub errors: Vec<String>,
}

impl Plugins {
    /// The GUI plugins that are on.
    pub fn gui_plugins(&self) -> Vec<Installed> {
        self.list().into_iter().filter(|p| p.enabled && p.kind == PluginKind::InterfaceAction).collect()
    }

    /// Open the shadow library at `library` in the host (reopening it if it
    /// was rebuilt) and load the GUI plugins that are on.
    pub fn gui_open(&self, library: &Path) -> Result<PluginMenus, String> {
        let plugins = self.gui_plugins();
        let paths: Vec<_> = plugins.iter().map(|p| self.path_of(p)).collect();
        let reply = self.host()?.call("gui_open", json!({ "library": library, "plugins": paths }), OPEN_TIMEOUT)?;
        let menus: Vec<PluginMenu> =
            serde_json::from_value(reply.get("actions").cloned().unwrap_or_default()).map_err(|e| e.to_string())?;
        let errors = reply
            .get("errors")
            .and_then(Value::as_object)
            .map(|errors| {
                errors
                    .iter()
                    .map(|(path, why)| {
                        let name = plugins
                            .iter()
                            .find(|p| self.path_of(p).to_string_lossy() == path.as_str())
                            .map_or(path.as_str(), |p| p.name.as_str());
                        format!("{name}: {}", why.as_str().unwrap_or_default())
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(PluginMenus { menus, errors })
    }

    /// Close the shadow library so it can be rebuilt. False while a plugin is
    /// working (it stays open).
    pub fn gui_close(&self) -> Result<bool, String> {
        let Some(host) = self.running_host() else { return Ok(true) };
        let reply = host.call("gui_close", json!({}), CALL_TIMEOUT)?;
        Ok(reply.get("closed").and_then(Value::as_bool).unwrap_or(false))
    }

    /// Whether a GUI plugin is working: in an action, a window open, or a job running.
    pub fn gui_busy(&self) -> bool {
        self.running_host()
            .and_then(|h| h.call("gui_busy", json!({}), CALL_TIMEOUT).ok())
            .and_then(|r| r.get("busy").and_then(Value::as_bool))
            .unwrap_or(false)
    }

    /// Run a plugin's menu item (`path` and its `texts`, from [`MenuItem`];
    /// empty for the toolbar button itself) on the books `selection`. Returns
    /// once it has started; a `gui-idle` event follows when it's done.
    pub fn gui_trigger(&self, plugin: &str, path: &[u32], texts: &[String], selection: &[i64]) -> Result<(), String> {
        self.host()?
            .call(
                "gui_trigger",
                json!({ "plugin": plugin, "path": path, "texts": texts, "selection": selection }),
                CALL_TIMEOUT,
            )
            .map(|_| ())
    }
}

impl Plugins {
    /// Answer a plugin waiting on Magnitide (a `gui-ask` event, for calibre's
    /// Edit Metadata or Remove Books): `answer` holds the books as saved
    /// (`{"books": {shadow id: book}}`, a book as Magnitide's catalog has it,
    /// plus `coverPath`) or those deleted (`{"removed": [shadow ids]}`). The
    /// host changes the shadow to match before this returns; the plugin
    /// waits on until [`Plugins::gui_resume`].
    pub fn gui_answer(&self, token: u64, mut answer: Value) -> Result<(), String> {
        if !answer.is_object() {
            answer = json!({});
        }
        answer["token"] = json!(token);
        self.host()?.call("gui_answer", answer, CALL_TIMEOUT).map(|_| ())
    }

    /// Let a plugin waiting on Magnitide go on.
    pub fn gui_resume(&self, token: u64) -> Result<(), String> {
        self.host()?.call("gui_resume", json!({ "token": token }), CALL_TIMEOUT).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_hosts_menus() {
        let menu: PluginMenu = serde_json::from_value(json!({
            "plugin": "EpubMerge", "file": "epubmerge.zip", "text": "EpubMerge", "icon": null, "main": true,
            "items": [
                {"path": [0], "text": "Merge Epubs", "enabled": true, "checkable": false, "checked": false},
                {"separator": true},
                {"path": [2], "text": "More", "enabled": true, "checkable": false, "checked": false,
                 "children": [{"path": [2, 0], "text": "Help", "enabled": false, "checkable": false, "checked": false}]}
            ]
        }))
        .unwrap();
        assert_eq!(menu.items.len(), 3);
        assert!(menu.items[1].separator);
        assert_eq!(menu.items[2].children[0].path, [2, 0]);
        assert!(!menu.items[2].children[0].enabled);
    }
}
