//! Native access to calibre libraries (`metadata.db` + book folders): read-only
//! for existing libraries, and a writer for new ones ([`write`]).
//!
//! Libraries load into the shared [`magnitide_catalog::Catalog`]; the model,
//! search and sort are re-exported from there.

pub mod dates;
pub mod library;
pub mod locate;
pub mod sqlfuncs;
pub mod synth;
pub mod write;

pub use library::{Library, Snapshot};
pub use magnitide_catalog::{
    Book, BookId, Catalog, CustomColumn, CustomValue, Datatype, Format, Query, SortDir, SortKey, model, search, sort,
    sort_books,
};
