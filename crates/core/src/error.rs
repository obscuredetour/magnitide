use std::path::PathBuf;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("no Magnitide library at {0}")]
    NotALibrary(PathBuf),

    #[error("{0} already contains a Magnitide library")]
    AlreadyALibrary(PathBuf),

    /// The library was written by a newer Magnitide; opening it could lose data.
    #[error(
        "this library uses schema version {found}, but this version of Magnitide only understands up to {supported}; update Magnitide to open it"
    )]
    SchemaTooNew { found: u32, supported: u32 },

    #[error("no book with id {0}")]
    NoSuchBook(i64),

    #[error("invalid book: {0}")]
    InvalidBook(String),

    #[error("invalid name: {0}")]
    InvalidName(String),

    #[error("import failed: {0}")]
    Import(String),

    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}
