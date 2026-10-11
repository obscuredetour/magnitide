//! Bringing books into a library: calibre libraries, and book files
//! (drag and drop, "Add Books…"). Both store books through `store`.

pub mod calibre;
pub mod files;
pub(crate) mod store;

pub use calibre::{ImportReport, Progress, import_calibre, import_calibre_into};
pub use files::{
    AddFilter, AddProgress, AddReport, DOCUMENT_FORMATS, DropScan, add_files, add_files_filtered, add_files_with, scan,
};
