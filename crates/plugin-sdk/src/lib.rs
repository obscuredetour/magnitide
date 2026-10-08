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

/// Export a metadata source: `export_metadata_source!(MyType)`.
#[macro_export]
macro_rules! export_metadata_source {
    ($t:ident) => {
        $crate::bindings::export!($t with_types_in $crate::bindings);
    };
}

/// What a metadata source needs.
pub mod prelude {
    pub use crate::bindings::exports::magnitide::plugin::metadata_source::{self, Guest as MetadataSource, SourceInfo};
    pub use crate::bindings::magnitide::plugin::types::{BookRecord, Identifier, Lookup};
    pub use crate::{http, log};
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
