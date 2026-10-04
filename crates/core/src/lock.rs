//! One writer per library.
//!
//! An open [`Library`](crate::Library) holds an exclusive OS file lock on
//! `.magnitide.lock` in its folder. A second process (another Magnitide
//! window, the server, a script) gets [`Error::LibraryInUse`] instead of
//! two writers interleaving changes. The OS releases the lock when the file
//! is closed or the process dies, so a crash never leaves a stale lock.
//!
//! The lock file also records who holds it, for the error message. Network
//! filesystems don't reliably honour these locks, which is one reason
//! libraries on network shares get a warning (see [`crate::fsinfo`]).

use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

pub const LOCK_FILE: &str = ".magnitide.lock";

/// Held for as long as the library is open; dropping it releases the lock.
#[derive(Debug)]
pub struct LibraryLock {
    _file: File,
}

/// Who holds a library, as written into the lock file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockHolder {
    pub app: String,
    pub pid: u32,
    pub host: Option<String>,
    pub since: String,
}

impl LibraryLock {
    pub fn acquire(root: &Path) -> Result<Self> {
        let path = root.join(LOCK_FILE);
        let mut file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&path)?;
        match file.try_lock() {
            Ok(()) => {
                let holder = LockHolder {
                    app: format!("Magnitide {}", env!("CARGO_PKG_VERSION")),
                    pid: std::process::id(),
                    host: hostname(),
                    since: crate::library::now(),
                };
                // Best effort: the lock itself is what matters.
                let _ = file
                    .set_len(0)
                    .and_then(|_| file.write_all(serde_json::to_string(&holder).unwrap_or_default().as_bytes()));
                Ok(Self { _file: file })
            }
            Err(TryLockError::WouldBlock) => {
                // Some platforms won't let us read a locked file; then we just can't say who.
                let holder = fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str(&s).ok());
                Err(Error::LibraryInUse(holder))
            }
            Err(TryLockError::Error(e)) => Err(e.into()),
        }
    }
}

#[cfg(unix)]
fn hostname() -> Option<String> {
    let mut buf = [0u8; 256];
    // SAFETY: the buffer is valid for its length; gethostname NUL-terminates
    // on success (we also cap at the buffer length).
    let rc = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) };
    if rc != 0 {
        return None;
    }
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    Some(String::from_utf8_lossy(&buf[..end]).trim_end_matches(".local").to_owned()).filter(|h| !h.is_empty())
}

#[cfg(not(unix))]
fn hostname() -> Option<String> {
    std::env::var("COMPUTERNAME").ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_lock_fails_until_first_is_released() {
        let dir = tempfile::tempdir().unwrap();
        let first = LibraryLock::acquire(dir.path()).unwrap();
        match LibraryLock::acquire(dir.path()) {
            Err(Error::LibraryInUse(Some(holder))) => assert_eq!(holder.pid, std::process::id()),
            Err(Error::LibraryInUse(None)) => {} // platform wouldn't let us read the holder
            other => panic!("expected LibraryInUse, got {other:?}"),
        }
        drop(first);
        LibraryLock::acquire(dir.path()).unwrap();
    }
}
