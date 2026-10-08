//! calibre's `FileTypePlugin`s: run on files being added (`on_import`),
//! and after books or formats are added, converted or removed
//! (`postimport`, `postadd`, `postconvert`, `postdelete`), on the shadow
//! library.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use magnitide_catalog::BookId;
use magnitide_core::hooks::{ImportHooks, ImportOutcome};
use serde_json::{Value, json};

use crate::{Installed, PluginKind, Plugins};

/// A big file through a slow plugin (a large PDF rewritten, say).
const ON_IMPORT_TIMEOUT: Duration = Duration::from_secs(300);
const POST_TIMEOUT: Duration = Duration::from_secs(120);
/// After the host couldn't start, hooks aren't retried for this long, so
/// adding a folder of books doesn't try again for every file.
const RETRY_AFTER: Duration = Duration::from_secs(60);

/// Which hook of a file-type plugin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hook {
    OnImport,
    Postimport,
    Postadd,
    Postconvert,
    Postdelete,
}

impl Hook {
    fn name(self) -> &'static str {
        match self {
            Hook::OnImport => "import",
            Hook::Postimport => "postimport",
            Hook::Postadd => "postadd",
            Hook::Postconvert => "postconvert",
            Hook::Postdelete => "postdelete",
        }
    }
}

/// When the host last failed to start for a hook (see [`RETRY_AFTER`]).
#[derive(Default)]
pub(crate) struct HookFailure(Mutex<Option<Instant>>);

impl Plugins {
    /// Enabled file-type plugins with `hook` on, for `format` (upper case;
    /// a plugin with `*` takes every format), highest priority first.
    pub fn file_type_plugins(&self, hook: Hook, format: &str) -> Vec<Installed> {
        let ext = format.to_lowercase();
        let mut ans: Vec<Installed> = self
            .list()
            .into_iter()
            .filter(|p| p.enabled && p.kind == PluginKind::FileType)
            .filter(|p| {
                let Some(h) = &p.hooks else { return false };
                let on = match hook {
                    Hook::OnImport => h.on_import,
                    Hook::Postimport => h.on_postimport,
                    Hook::Postadd => h.on_postadd,
                    Hook::Postconvert => h.on_postconvert,
                    Hook::Postdelete => h.on_postdelete,
                };
                on && (h.file_types.iter().any(|f| f == "*")
                    || h.file_types.iter().any(|f| f.eq_ignore_ascii_case(&ext)))
            })
            .collect();
        ans.sort_by_key(|p| std::cmp::Reverse(p.hooks.as_ref().map_or(1, |h| h.priority)));
        ans
    }

    /// Whether any enabled plugin wants `hook` for `format` (no host needed).
    pub fn wants_post(&self, hook: Hook, format: &str) -> bool {
        !self.file_type_plugins(hook, format).is_empty()
    }

    fn hooks_blocked(&self) -> bool {
        self.hook_failure.0.lock().unwrap_or_else(|e| e.into_inner()).is_some_and(|t| t.elapsed() < RETRY_AFTER)
    }

    fn note_hook_failure(&self, failed: bool) {
        *self.hook_failure.0.lock().unwrap_or_else(|e| e.into_inner()) = failed.then(Instant::now);
    }

    /// Run the `on_import` plugins on a file (chained, as calibre's are).
    pub fn run_on_import(&self, path: &Path, format: &str) -> ImportOutcome {
        let plugins = self.file_type_plugins(Hook::OnImport, format);
        if plugins.is_empty() {
            return ImportOutcome::Unchanged;
        }
        if self.hooks_blocked() {
            return ImportOutcome::Failed("the plugin runtime couldn't start earlier; not tried again yet".into());
        }
        let host = match self.host() {
            Ok(h) => h,
            Err(e) => {
                self.note_hook_failure(true);
                self.note_error("File type plugins", &e);
                return ImportOutcome::Failed(e);
            }
        };
        self.note_hook_failure(false);
        let paths: Vec<PathBuf> = plugins.iter().map(|p| self.path_of(p)).collect();
        let params = json!({ "path": path, "ext": format.to_lowercase(), "plugins": paths });
        match host.call("filetype.run", params, ON_IMPORT_TIMEOUT) {
            Ok(v) => {
                let new = v.get("path").and_then(Value::as_str).map(PathBuf::from);
                let temporary = v.get("temporary").and_then(Value::as_bool).unwrap_or(false);
                match new {
                    Some(new) if new != path => ImportOutcome::Replaced { path: new, temporary },
                    _ => ImportOutcome::Unchanged,
                }
            }
            Err(e) => {
                for p in &plugins {
                    self.note_error(&p.name, &e);
                }
                ImportOutcome::Failed(e)
            }
        }
    }

    /// Run a post hook for one book of the open shadow library (`library`),
    /// with the formats concerned. Returns the plugins that ran.
    pub fn run_post(
        &self,
        hook: Hook,
        library: &Path,
        book_id: BookId,
        formats: &[String],
    ) -> Result<Vec<String>, String> {
        let mut plugins = Vec::new();
        for f in formats {
            for p in self.file_type_plugins(hook, f) {
                if !plugins.iter().any(|q: &Installed| q.name == p.name) {
                    plugins.push(p);
                }
            }
        }
        if plugins.is_empty() {
            return Ok(Vec::new());
        }
        let host = self.host()?;
        let paths: Vec<PathBuf> = plugins.iter().map(|p| self.path_of(p)).collect();
        let params = json!({
            "hook": hook.name(), "library": library, "bookId": book_id, "formats": formats, "plugins": paths,
        });
        let v = host.call("filetype.post", params, POST_TIMEOUT)?;
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

impl ImportHooks for Plugins {
    fn file_types(&self) -> Vec<String> {
        let mut ans: Vec<String> = Vec::new();
        for p in self.list() {
            if !p.enabled || !matches!(p.kind, PluginKind::FileType | PluginKind::MetadataReader) {
                continue;
            }
            for f in p.hooks.iter().flat_map(|h| h.file_types.iter()) {
                let f = f.to_lowercase();
                if f != "*" && !ans.contains(&f) {
                    ans.push(f);
                }
            }
        }
        ans
    }

    fn on_import(&self, path: &Path, format: &str) -> ImportOutcome {
        self.run_on_import(path, format)
    }
}
