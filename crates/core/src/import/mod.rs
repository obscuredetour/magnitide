//! Bringing books into a library: calibre libraries, and book files
//! (drag and drop, "Add Books…"). Both store books through `store`.

pub mod calibre;
pub mod files;
pub(crate) mod store;

pub use calibre::{ImportReport, Progress, import_calibre};
pub use files::{AddProgress, AddReport, add_files, add_files_with};
