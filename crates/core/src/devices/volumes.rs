//! Mounted volumes that could be e-readers, and their free space.

use std::path::{Path, PathBuf};

/// Roots of mounted removable-looking volumes (not the system disk).
pub fn mounted() -> Vec<PathBuf> {
    imp::mounted().into_iter().filter(|p| !crate::fsinfo::is_network_filesystem(p)).collect()
}

/// (total, free) bytes on the volume holding `path`, where known.
pub fn space(path: &Path) -> Option<(u64, u64)> {
    imp::space(path)
}

/// Unmount (eject) the volume at `root` so it can be unplugged safely.
/// Errors carry the system's reason (e.g. a file still open on it).
pub fn eject(root: &Path) -> Result<(), String> {
    imp::eject(root)
}

/// Run a command; its stderr (or stdout) becomes the error.
#[allow(dead_code)] // Not every platform shells out.
fn run(program: &str, args: &[&std::ffi::OsStr]) -> Result<(), String> {
    let out = std::process::Command::new(program).args(args).output().map_err(|e| format!("{program}: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    let text = String::from_utf8_lossy(if out.stderr.is_empty() { &out.stdout } else { &out.stderr }).trim().to_owned();
    Err(if text.is_empty() { format!("{program} failed ({})", out.status) } else { text })
}

#[cfg(unix)]
fn statvfs_space(path: &Path) -> Option<(u64, u64)> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let c = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut st = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: valid NUL-terminated path and a properly sized out-buffer.
    if unsafe { libc::statvfs(c.as_ptr(), st.as_mut_ptr()) } != 0 {
        return None;
    }
    // SAFETY: statvfs succeeded, so the struct is initialized.
    let st = unsafe { st.assume_init() };
    #[allow(clippy::unnecessary_cast)] // The field types differ between platforms.
    let (frsize, blocks, avail) = (st.f_frsize as u64, st.f_blocks as u64, st.f_bavail as u64);
    Some((blocks * frsize, avail * frsize))
}

#[cfg(target_os = "macos")]
mod imp {
    use std::os::unix::fs::MetadataExt;
    use std::path::{Path, PathBuf};

    /// Everything in /Volumes except the boot volume's link there.
    pub fn mounted() -> Vec<PathBuf> {
        let Ok(root_dev) = std::fs::metadata("/").map(|m| m.dev()) else { return Vec::new() };
        let Ok(entries) = std::fs::read_dir("/Volumes") else { return Vec::new() };
        entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| std::fs::metadata(p).is_ok_and(|m| m.is_dir() && m.dev() != root_dev))
            .collect()
    }

    pub fn space(path: &Path) -> Option<(u64, u64)> {
        super::statvfs_space(path)
    }

    /// Ejects the whole disk (every volume on the e-reader), like Finder.
    pub fn eject(root: &Path) -> Result<(), String> {
        super::run("diskutil", &["eject".as_ref(), root.as_os_str()])
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use std::path::{Path, PathBuf};

    /// Mount points under /media, /run/media and /mnt (where desktops and
    /// users mount USB storage).
    pub fn mounted() -> Vec<PathBuf> {
        let Ok(mounts) = std::fs::read_to_string("/proc/self/mounts") else { return Vec::new() };
        mounts
            .lines()
            .filter_map(|l| l.split(' ').nth(1))
            .map(unescape)
            .filter(|m| ["/media/", "/run/media/", "/mnt/"].iter().any(|p| m.starts_with(p)))
            .map(PathBuf::from)
            .collect()
    }

    /// /proc/mounts writes spaces and the like as octal escapes ("\040").
    fn unescape(s: &str) -> String {
        let b = s.as_bytes();
        let mut out = Vec::with_capacity(b.len());
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'\\'
                && i + 3 < b.len()
                && b[i + 1..i + 4].iter().all(|c| (b'0'..=b'7').contains(c))
                && let Ok(v) = u8::from_str_radix(&s[i + 1..i + 4], 8)
            {
                out.push(v);
                i += 4;
            } else {
                out.push(b[i]);
                i += 1;
            }
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    pub fn space(path: &Path) -> Option<(u64, u64)> {
        super::statvfs_space(path)
    }

    /// Desktop eject (unmount and power off) where available, else unmount.
    pub fn eject(root: &Path) -> Result<(), String> {
        super::run("gio", &["mount".as_ref(), "-e".as_ref(), root.as_os_str()])
            .or_else(|_| super::run("udisksctl", &["unmount".as_ref(), "-p".as_ref(), root.as_os_str()]))
            .or_else(|_| super::run("umount", &[root.as_os_str()]))
    }
}

#[cfg(windows)]
mod imp {
    use std::path::{Path, PathBuf};

    /// Drive letters D: onwards (A: and B: can stall on probing, C: is the system).
    pub fn mounted() -> Vec<PathBuf> {
        ('D'..='Z').map(|l| PathBuf::from(format!("{l}:\\"))).filter(|p| p.is_dir()).collect()
    }

    pub fn space(_path: &Path) -> Option<(u64, u64)> {
        None
    }

    /// The shell's "Eject" verb on the drive, as in File Explorer.
    pub fn eject(root: &Path) -> Result<(), String> {
        let drive = root.to_string_lossy().trim_end_matches('\\').to_owned();
        if drive.len() != 2 || !drive.ends_with(':') {
            return Err(format!("{drive} isn't a drive letter"));
        }
        let script =
            format!("(New-Object -ComObject Shell.Application).Namespace(17).ParseName('{drive}').InvokeVerb('Eject')");
        super::run("powershell", &["-NoProfile".as_ref(), "-Command".as_ref(), script.as_ref()])
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
mod imp {
    use std::path::{Path, PathBuf};

    pub fn mounted() -> Vec<PathBuf> {
        Vec::new()
    }

    pub fn space(path: &Path) -> Option<(u64, u64)> {
        super::statvfs_space(path)
    }

    pub fn eject(_root: &Path) -> Result<(), String> {
        Err("ejecting isn't supported on this system".into())
    }
}
