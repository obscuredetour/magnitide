//! The Magnitide library: native schema and migrations, queries, edits and
//! (soon) the import pipeline. No UI or Tauri code lives here, so the
//! desktop app and the planned server share it. See docs/architecture.md.

pub mod catalog;
pub mod db;
pub mod devices;
pub mod edit;
pub mod error;
pub mod fsinfo;
pub mod html;
mod images;
pub mod import;
pub mod library;
pub mod lock;
pub mod metadata;
pub mod names;
pub mod paths;
pub mod tidy;
pub mod writeback;

pub use edit::{BookEdit, EditOutcome};
pub use error::{Error, Result};
pub use library::{Book, BookId, LOCAL_USER, Library, LibraryInfo, NewBook, UserId};
pub use magnitide_catalog::Catalog;
