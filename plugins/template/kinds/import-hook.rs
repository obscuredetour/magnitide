//! An import hook: runs on files being added, before their metadata is
//! read. The file is at `/in/<name>`, read-only; a rewritten file goes
//! under `/out/`. manifest.toml: kind = "import-hook".

use magnitide_plugin_sdk::prelude::*;

struct MyHook;

impl ImportHook for MyHook {
    /// Lower-case extensions to run on; "*" for every file. Extensions
    /// Magnitide doesn't know become addable as books.
    fn file_types() -> Vec<String> {
        vec!["epub".into()]
    }

    /// `file` is the sandbox path (`/in/book.epub`), `format` upper case.
    /// Return `Unchanged`, or `Replaced` with a path you wrote under
    /// `/out/` (its extension decides the format it's added as). An error
    /// means the file is added as it is and the reason shown.
    fn on_import(file: String, format: String) -> Result<Outcome, String> {
        log::info(format!("{file} ({format})"));
        let bytes = std::fs::read(&file).map_err(|e| format!("couldn't read {file}: {e}"))?;
        if bytes.is_empty() {
            return Err("the file is empty".into());
        }
        // std::fs::write("/out/book.epub", rewritten)?; return Ok(Outcome::Replaced("/out/book.epub".into()));
        Ok(Outcome::Unchanged)
    }
}

magnitide_plugin_sdk::export_import_hook!(MyHook);
