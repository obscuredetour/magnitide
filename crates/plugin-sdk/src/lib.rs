//! The SDK for Magnitide plugins written in Rust.
//!
//! A plugin is a WebAssembly component built for `wasm32-wasip2`. The API is
//! the WIT package in `wit/` (`magnitide:plugin`); this crate holds its Rust
//! bindings and a few helpers. A metadata source:
//!
//! ```ignore
//! use magnitide_plugin_sdk::prelude::*;
//!
//! struct MySource;
//!
//! impl metadata_source::Guest for MySource {
//!     fn info() -> SourceInfo {
//!         SourceInfo { name: "My Source".into(), description: "…".into(), version: "1.0.0".into() }
//!     }
//!     fn identify(lookup: Lookup) -> Result<Vec<BookRecord>, String> {
//!         let body = http::get(&format!("https://example.com/search?q={}", lookup.title))?;
//!         Ok(vec![])
//!     }
//! }
//!
//! magnitide_plugin_sdk::export_metadata_source!(MySource);
//! ```
//!
//! An import hook (`ImportHook`) or a file-metadata plugin (`FileMetadata`)
//! is written the same way, exported with `export_import_hook!` or
//! `export_file_metadata!`. They get the file through the sandbox: read it
//! at the `/in/...` path given, write under `/out/`, with `std::fs`.
//!
//! Build with `cargo build --release --target wasm32-wasip2`, then package
//! the `.wasm` with a `manifest.toml` into a `.mgp` (see docs/plugin-api.md).

#![allow(clippy::all)]

pub mod bindings {
    wit_bindgen::generate!({
        world: "metadata-source-plugin",
        path: "wit",
        pub_export_macro: true,
        default_bindings_module: "magnitide_plugin_sdk::bindings",
    });
}

/// The import-hook world; its imports are the ones above.
pub mod import_hook_bindings {
    wit_bindgen::generate!({
        world: "import-hook-plugin",
        path: "wit",
        pub_export_macro: true,
        export_macro_name: "export_import_hook",
        default_bindings_module: "magnitide_plugin_sdk::import_hook_bindings",
        with: {
            "magnitide:plugin/log@0.1.0": crate::bindings::magnitide::plugin::log,
            "magnitide:plugin/http@0.1.0": crate::bindings::magnitide::plugin::http,
        },
    });
}

/// The file-metadata world; its imports and types are the ones above.
pub mod file_metadata_bindings {
    wit_bindgen::generate!({
        world: "file-metadata-plugin",
        path: "wit",
        pub_export_macro: true,
        export_macro_name: "export_file_metadata",
        default_bindings_module: "magnitide_plugin_sdk::file_metadata_bindings",
        with: {
            "magnitide:plugin/types@0.1.0": crate::bindings::magnitide::plugin::types,
            "magnitide:plugin/log@0.1.0": crate::bindings::magnitide::plugin::log,
            "magnitide:plugin/http@0.1.0": crate::bindings::magnitide::plugin::http,
        },
    });
}

/// The library-action world: the `library` and `ui` imports are its own.
pub mod library_action_bindings {
    wit_bindgen::generate!({
        world: "library-action-plugin",
        path: "wit",
        pub_export_macro: true,
        export_macro_name: "export_library_action",
        default_bindings_module: "magnitide_plugin_sdk::library_action_bindings",
        with: {
            "magnitide:plugin/types@0.1.0": crate::bindings::magnitide::plugin::types,
            "magnitide:plugin/log@0.1.0": crate::bindings::magnitide::plugin::log,
            "magnitide:plugin/http@0.1.0": crate::bindings::magnitide::plugin::http,
        },
    });
}

/// Export a library action: `export_library_action!(MyType)`.
#[macro_export]
macro_rules! export_library_action {
    ($t:ident) => {
        $crate::library_action_bindings::export_library_action!($t with_types_in $crate::library_action_bindings);
    };
}

/// Export a metadata source: `export_metadata_source!(MyType)`.
#[macro_export]
macro_rules! export_metadata_source {
    ($t:ident) => {
        $crate::bindings::export!($t with_types_in $crate::bindings);
    };
}

/// Export an import hook: `export_import_hook!(MyType)`.
#[macro_export]
macro_rules! export_import_hook {
    ($t:ident) => {
        $crate::import_hook_bindings::export_import_hook!($t with_types_in $crate::import_hook_bindings);
    };
}

/// Export a file-metadata plugin: `export_file_metadata!(MyType)`.
#[macro_export]
macro_rules! export_file_metadata {
    ($t:ident) => {
        $crate::file_metadata_bindings::export_file_metadata!($t with_types_in $crate::file_metadata_bindings);
    };
}

/// What a plugin of any kind needs.
pub mod prelude {
    pub use crate::bindings::exports::magnitide::plugin::metadata_source::{self, Guest as MetadataSource, SourceInfo};
    pub use crate::bindings::magnitide::plugin::types::{Author, BookMetadata, BookRecord, Identifier, Lookup};
    pub use crate::file_metadata_bindings::exports::magnitide::plugin::file_metadata::{
        self, Guest as FileMetadata, Precedence, ReadsFormat,
    };
    pub use crate::import_hook_bindings::exports::magnitide::plugin::import_hook::{
        self, Guest as ImportHook, Outcome,
    };
    pub use crate::library_action_bindings::exports::magnitide::plugin::library_action::{
        self, Guest as LibraryAction, MenuItem,
    };
    pub use crate::library_action_bindings::magnitide::plugin::library::{
        self, Book, BookEdit, CustomEdit, CustomField, CustomValue, FieldValue, SeriesValue,
    };
    pub use crate::{http, log, ui};
}

/// What the user sees of a running library action: progress, and questions.
pub mod ui {
    pub use crate::library_action_bindings::magnitide::plugin::ui::{ask, ask_text, progress, status};

    /// A yes-or-no question; false when dismissed.
    pub fn confirm(question: &str) -> bool {
        ask(question, &["Yes".to_owned(), "No".to_owned()]) == Some(0)
    }
}

/// HTTP through Magnitide (the `network` capability).
pub mod http {
    pub use crate::bindings::magnitide::plugin::http::{Header, Request, Response, fetch};

    /// GET `url`; the body when the status is 2xx, else an error with the status.
    pub fn get(url: &str) -> Result<Vec<u8>, String> {
        get_with(url, &[])
    }

    /// GET with extra headers, e.g. `[("Accept", "application/json")]`.
    pub fn get_with(url: &str, headers: &[(&str, &str)]) -> Result<Vec<u8>, String> {
        let req = Request {
            method: "GET".into(),
            url: url.into(),
            headers: headers.iter().map(|(n, v)| Header { name: (*n).into(), value: (*v).into() }).collect(),
            body: None,
        };
        let resp = fetch(&req)?;
        if (200..300).contains(&resp.status) { Ok(resp.body) } else { Err(format!("HTTP {} from {url}", resp.status)) }
    }

    /// GET and decode as UTF-8 text.
    pub fn get_text(url: &str) -> Result<String, String> {
        String::from_utf8(get(url)?).map_err(|e| format!("{url}: not UTF-8 text: {e}"))
    }
}

/// The plugin log (Settings › Plugins shows it).
pub mod log {
    pub use crate::bindings::magnitide::plugin::log::{Level, log};

    pub fn debug(text: impl AsRef<str>) {
        log(Level::Debug, text.as_ref());
    }
    pub fn info(text: impl AsRef<str>) {
        log(Level::Info, text.as_ref());
    }
    pub fn warn(text: impl AsRef<str>) {
        log(Level::Warn, text.as_ref());
    }
    pub fn error(text: impl AsRef<str>) {
        log(Level::Error, text.as_ref());
    }
}
