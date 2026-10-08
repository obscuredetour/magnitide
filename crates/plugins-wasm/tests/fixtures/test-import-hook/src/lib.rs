//! An import hook for the tests: runs on `.txt` and `.mgtest` files. A
//! `.mgtest` file is rewritten as upper-case `.txt` (the format changes);
//! a `.txt` file is left alone, unless it says FAIL, LOOP or PANIC.

use magnitide_plugin_sdk::prelude::*;

struct Hook;

impl ImportHook for Hook {
    fn file_types() -> Vec<String> {
        vec!["txt".into(), "mgtest".into()]
    }

    fn on_import(file: String, format: String) -> Result<Outcome, String> {
        log::info(format!("on-import {file} ({format})"));
        let text = std::fs::read_to_string(&file).map_err(|e| format!("couldn't read {file}: {e}"))?;
        // The sandbox: nothing outside /in and /out is there.
        if std::fs::read_dir("/").map(|d| d.count()).unwrap_or(0) > 2 || std::fs::read("/etc/passwd").is_ok() {
            return Err("the sandbox leaks".into());
        }
        if std::fs::write(format!("{file}.x"), b"x").is_ok() {
            return Err("/in is writable".into());
        }
        match text.trim() {
            "FAIL" => return Err("This hook failed on purpose.".into()),
            "PANIC" => panic!("on purpose"),
            "LOOP" => loop {
                std::hint::black_box(0);
            },
            _ => {}
        }
        if format == "MGTEST" {
            let stem = file.rsplit('/').next().unwrap_or("book").trim_end_matches(".mgtest");
            let out = format!("/out/{stem}.txt");
            std::fs::write(&out, text.to_uppercase()).map_err(|e| format!("couldn't write {out}: {e}"))?;
            return Ok(Outcome::Replaced(out));
        }
        Ok(Outcome::Unchanged)
    }
}

magnitide_plugin_sdk::export_import_hook!(Hook);
