//! The in-memory book catalog shared by every kind of library (calibre
//! libraries opened read-only, native Magnitide libraries): the book model,
//! search and sort. Loaders turn a library into a [`Catalog`]; views query it.

pub mod catalog;
pub mod diff;
pub mod languages;
pub mod matching;
pub mod model;
pub mod search;
pub mod sort;

pub use catalog::Catalog;
pub use model::{Book, BookId, CustomColumn, CustomValue, Datatype, Format, SeriesEntry, format_number};
pub use search::Query;
pub use sort::{SortDir, SortKey, sort_books, sort_books_by_field};
