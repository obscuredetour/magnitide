//! Facts about the filesystem a library lives on.

use std::path::Path;

/// True if `path` is on a network filesystem (SMB/CIFS, NFS, AFP, …).
///
/// SQLite databases and file locks aren't reliable over network shares, so
/// libraries there get a warning: open them from one computer at a time, or
/// use the server. Unknown cases report false.
///
/// Windows: only UNC paths (`\\server\share`) are detected so far; mapped
/// network drives aren't yet.
pub fn is_network_filesystem(path: &Path) -> bool {
    imp::is_network(path)
}

#[cfg(target_os = "macos")]
mod imp {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    pub fn is_network(path: &Path) -> bool {
        let Ok(c) = CString::new(path.as_os_str().as_bytes()) else { return false };
        let mut st = std::mem::MaybeUninit::<libc::statfs>::uninit();
        // SAFETY: valid NUL-terminated path and a properly sized out-buffer.
        if unsafe { libc::statfs(c.as_ptr(), st.as_mut_ptr()) } != 0 {
            return false;
        }
        // SAFETY: statfs succeeded, so the struct is initialized.
        let st = unsafe { st.assume_init() };
        // MNT_LOCAL is set for volumes stored on this machine.
        st.f_flags & (libc::MNT_LOCAL as u32) == 0
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    // Filesystem magic numbers (linux/magic.h). FUSE is deliberately absent:
    // it's used for both local (ntfs-3g) and remote (sshfs) filesystems.
    const NETWORK: &[i64] = &[
        0x6969,     // NFS
        0x517B,     // SMB
        0xFF534D42, // CIFS
        0xFE534D42, // SMB2
        0x5346414F, // AFS
        0x73757245, // Coda
        0x01021997, // 9P (VM and WSL shares)
    ];

    pub fn is_network(path: &Path) -> bool {
        let Ok(c) = CString::new(path.as_os_str().as_bytes()) else { return false };
        let mut st = std::mem::MaybeUninit::<libc::statfs>::uninit();
        // SAFETY: valid NUL-terminated path and a properly sized out-buffer.
        if unsafe { libc::statfs(c.as_ptr(), st.as_mut_ptr()) } != 0 {
            return false;
        }
        // SAFETY: statfs succeeded, so the struct is initialized.
        let st = unsafe { st.assume_init() };
        NETWORK.contains(&(st.f_type as i64))
    }
}

#[cfg(windows)]
mod imp {
    use std::path::Path;

    pub fn is_network(path: &Path) -> bool {
        let s = path.as_os_str().to_string_lossy();
        s.starts_with(r"\\") && !s.starts_with(r"\\?\") || s.starts_with(r"\\?\UNC\")
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
mod imp {
    pub fn is_network(_: &std::path::Path) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_temp_dir_is_not_network() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_network_filesystem(dir.path()));
    }
}
