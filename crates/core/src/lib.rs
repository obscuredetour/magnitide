//! The Magnitide library: native schema and migrations, queries, edits and
//! (soon) the import pipeline. No UI or Tauri code lives here, so the
//! desktop app and the planned server share it. See docs/architecture.md.

pub mod archive;
pub mod bulk;
pub mod catalog;
pub mod comics;
pub mod convert;
pub mod custom;
pub mod db;
pub mod delete;
pub mod devices;
pub mod edit;
pub mod error;
pub mod external;
pub mod fingerprint;
pub mod fsinfo;
pub mod hooks;
pub mod html;
mod images;
pub mod import;
pub mod library;
pub mod lock;
pub mod merge;
pub mod metadata;
pub mod names;
pub mod pages;
pub mod paths;
pub mod positions;
pub mod shelves;
pub mod tidy;
pub mod values;
pub mod writeback;

pub use custom::{CustomField, FieldKind, FieldValue, NewField};
pub use edit::{AuthorSort, BookEdit, EditOutcome};
pub use error::{Error, Result};
pub use library::{Book, BookId, LOCAL_USER, Library, LibraryInfo, NewBook, UserId};
pub use magnitide_catalog::Catalog;
