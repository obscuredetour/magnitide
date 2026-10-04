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

    /// Writing metadata into a book file failed; the file is unchanged.
    #[error("couldn't write metadata into {file}: {reason}")]
    WriteFile { file: String, reason: String },

    #[error("import failed: {0}")]
    Import(String),

    /// Another process has the library open for writing.
    #[error("{}", describe_holder(.0.as_ref()))]
    LibraryInUse(Option<crate::lock::LockHolder>),

    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

fn describe_holder(holder: Option<&crate::lock::LockHolder>) -> String {
    match holder {
        Some(h) => format!(
            "This library is already open in {} (process {}{}, since {}). Close it there first.",
            h.app,
            h.pid,
            h.host.as_deref().map(|host| format!(" on {host}")).unwrap_or_default(),
            h.since
        ),
        None => "This library is already open in another Magnitide window or program. Close it there first.".into(),
    }
}
