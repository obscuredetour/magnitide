//! Bringing books into a library. calibre libraries for now; single files
//! (drag and drop) and plugin hooks come next.

pub mod calibre;

pub use calibre::{ImportReport, Progress, import_calibre};
