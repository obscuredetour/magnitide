//! A plugin's own settings dialog, and library-closed plugins.

use std::path::Path;
use std::time::Duration;

use serde::Serialize;
use serde_json::{Value, json};

use crate::{PluginKind, Plugins};

/// What configuring a plugin came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct Configured {
    /// The user saved.
    pub changed: bool,
    /// The plugin asked for a restart (the host restarts before the next run).
    pub restart: bool,
}

impl Plugins {
    /// Open the plugin's settings (its own dialog, in the host) and wait for
    /// it to close.
    pub fn configure(&self, name: &str) -> Result<Configured, String> {
        let p = self.list().into_iter().find(|p| p.name == name).ok_or("That plugin isn't installed.")?;
        let host = self.host()?;
        // The user may take as long as they like.
        let v = host.call("plugin.configure", json!({ "path": self.path_of(&p) }), Duration::from_secs(3600))?;
        let ans = Configured {
            changed: v.get("changed").and_then(Value::as_bool).unwrap_or(false),
            restart: v.get("restart").and_then(Value::as_bool).unwrap_or(false),
        };
        if ans.restart && !self.gui_busy() {
            // Loaded again, with the new settings, when next needed.
            *self.host.lock().unwrap_or_else(|e| e.into_inner()) = None;
        }
        Ok(ans)
    }

    /// Tell library-closed plugins the library (the shadow, as plugins know
    /// it) is closing. Only if the host is running; never starts one.
    pub fn library_closed(&self, library: &Path) -> Result<Vec<String>, String> {
        let plugins: Vec<_> =
            self.list().into_iter().filter(|p| p.enabled && p.kind == PluginKind::LibraryClosed).collect();
        let Some(host) = self.running_host() else { return Ok(Vec::new()) };
        if plugins.is_empty() {
            return Ok(Vec::new());
        }
        let paths: Vec<_> = plugins.iter().map(|p| self.path_of(p)).collect();
        let v =
            host.call("library.closed", json!({ "library": library, "plugins": paths }), Duration::from_secs(60))?;
        if let Some(errors) = v.get("errors").and_then(Value::as_object) {
            for (name, why) in errors {
                self.note_error(name, why.as_str().unwrap_or_default());
            }
        }
        Ok(v.get("ran")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect())
            .unwrap_or_default())
    }
}
