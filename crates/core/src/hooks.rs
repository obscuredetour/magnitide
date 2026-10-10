//! Extension points plugins plug into: hooks on files being added, and
//! metadata readers and writers for formats Magnitide doesn't handle itself
//! (or handles, when a plugin says it knows better).
//!
//! These are traits so `core` stays free of any plugin system: the app
//! passes an implementation (calibre plugins today, WebAssembly plugins
//! later), and the tests use fakes. [`NoHooks`] is what everything else
//! uses.

use std::path::{Path, PathBuf};

use crate::metadata::BookMetadata;

/// What a hook made of a file being added.
#[derive(Debug, Clone, PartialEq)]
pub enum ImportOutcome {
    /// Add the file as it is.
    Unchanged,
    /// Add this file instead (a plugin rewrote it). `temporary` files are
    /// deleted once stored.
    Replaced { path: PathBuf, temporary: bool },
    /// The hook failed; the file is added as it is and the reason reported.
    Failed(String),
}

/// Hooks on books being added (calibre's `FileTypePlugin.on_import`).
pub trait ImportHooks: Send + Sync {
    /// Extra lower-case extensions that may be added as books, beyond
    /// Magnitide's own list.
    fn file_types(&self) -> Vec<String> {
        Vec::new()
    }

    /// Run before the file's metadata is read. Must never panic.
    fn on_import(&self, _path: &Path, _format: &str) -> ImportOutcome {
        ImportOutcome::Unchanged
    }
}

/// Whether a plugin reader beats Magnitide's own for a format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Precedence {
    /// Used only for formats Magnitide can't read itself.
    Fallback,
    /// Used first; Magnitide's reader on failure.
    Override,
}

/// Metadata readers and writers from plugins (calibre's
/// `MetadataReaderPlugin` and `MetadataWriterPlugin`).
pub trait MetadataPlugins: Send + Sync {
    /// Whether a plugin reads `format` (upper case), and how it ranks.
    fn reads(&self, _format: &str) -> Option<Precedence> {
        None
    }

    fn read(&self, _path: &Path, _format: &str) -> Result<BookMetadata, String> {
        Err("no plugin reads this format".into())
    }

    /// Whether a plugin writes metadata into `format` (upper case).
    fn writes(&self, _format: &str) -> bool {
        false
    }

    fn write(&self, _path: &Path, _format: &str, _meta: &BookMetadata) -> Result<(), String> {
        Err("no plugin writes this format".into())
    }
}

/// No plugins: the default everywhere.
pub struct NoHooks;

impl ImportHooks for NoHooks {}

impl MetadataPlugins for NoHooks {}
