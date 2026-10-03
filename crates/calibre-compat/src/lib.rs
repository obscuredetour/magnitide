//! Native, read-only access to calibre libraries (`metadata.db` + book folders).

pub mod dates;
pub mod library;
pub mod locate;
pub mod model;
pub mod search;
pub mod sort;
pub mod sqlfuncs;
pub mod synth;

pub use library::{Library, Snapshot};
pub use model::{Book, CustomColumn, CustomValue, Datatype, Format};
pub use search::Query;
pub use sort::{SortKey, sort_books};
