//! The Magnitide library: native schema and migrations, queries, edits and
//! (soon) the import pipeline. No UI or Tauri code lives here, so the
//! desktop app and the planned server share it. See docs/architecture.md.

pub mod db;
pub mod error;
pub mod library;
pub mod names;
pub mod paths;

pub use error::{Error, Result};
pub use library::{Book, BookId, LOCAL_USER, Library, LibraryInfo, NewBook, UserId};
