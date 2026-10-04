//! Native, read-only access to calibre libraries (`metadata.db` + book folders).
//!
//! Libraries load into the shared [`magnitide_catalog::Catalog`]; the model,
//! search and sort are re-exported from there.

pub mod dates;
pub mod library;
pub mod locate;
pub mod sqlfuncs;
pub mod synth;

pub use library::{Library, Snapshot};
pub use magnitide_catalog::{
    Book, BookId, Catalog, CustomColumn, CustomValue, Datatype, Format, Query, SortDir, SortKey, model, search, sort,
    sort_books,
};
