//! The plugin runtime: a Python (with Qt and the libraries calibre plugins
//! use) that Magnitide downloads once, so plugins don't need calibre
//! installed. Built per platform by `scripts/plugin-runtime/build.py` and
//! published as a release asset; only an archive whose sha256 is in
//! [`RELEASES`] (compiled in) is ever installed.
//!
//! Installed into `<base>/<VERSION>/` atomically: downloaded to
//! `<base>/<VERSION>.download` (resumable), verified, unpacked into
//! `<base>/<VERSION>.tmp`, precompiled, then renamed into place.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The runtime's version: bumped when the archive changes (another Python,
/// Qt or set of wheels). Matches `scripts/plugin-runtime/VERSION`.
pub const VERSION: &str = "2";

/// Where the archives are published: `<RELEASE_BASE><VERSION>/<file>`.
pub const RELEASE_BASE: &str = "https://github.com/obscuredetour/magnitide/releases/download/plugin-runtime-v";

/// A published archive for one target.
#[derive(Debug, Clone, Copy)]
pub struct Release {
    pub target: &'static str,
    pub file: &'static str,
    pub sha256: &'static str,
    /// The archive's size in bytes.
    pub size: u64,
    /// The unpacked runtime's size in bytes.
    pub installed_size: u64,
}

/// Printed by `build.py --rust-table manifest.json` after a release; must
/// match `scripts/plugin-runtime/sha256sums.txt` (a test checks).
pub const RELEASES: &[Release] = &[
    Release {
        target: "aarch64-apple-darwin",
        file: "magnitide-plugin-runtime-2-aarch64-apple-darwin.tar.zst",
        sha256: "b5963354de4998f8bcbcad5ba8dfdd8843b382caa275aec7bcfe098124fb4a66",
        size: 40463033,
        installed_size: 155897713,
    },
    Release {
        target: "aarch64-unknown-linux-gnu",
        file: "magnitide-plugin-runtime-2-aarch64-unknown-linux-gnu.tar.zst",
        sha256: "3673534473220770e5d38782f71d67a9aea3f063620c7b2ffc213a8e52b2f602",
        size: 61513755,
        installed_size: 246102062,
    },
    Release {
        target: "x86_64-apple-darwin",
        file: "magnitide-plugin-runtime-2-x86_64-apple-darwin.tar.zst",
        sha256: "189a8d2664afd913b4ffeee67527e48001fbab98cf8ca9ff7463bbe5863440c0",
        size: 43108178,
        installed_size: 155932394,
    },
    Release {
        target: "x86_64-pc-windows-msvc",
        file: "magnitide-plugin-runtime-2-x86_64-pc-windows-msvc.tar.zst",
        sha256: "41cdc69faf6c200d291776e051b665d160d286b6acaad1af1afa398fa65ad4c1",
        size: 34489796,
        installed_size: 130779676,
    },
    Release {
        target: "x86_64-unknown-linux-gnu",
        file: "magnitide-plugin-runtime-2-x86_64-unknown-linux-gnu.tar.zst",
        sha256: "2c3f1d3264dbe3887ba156a98009da16d5a5f7905cb5d059d00eeec781717d34",
        size: 67491562,
        installed_size: 249207746,
    },
];

/// This build's target triple, as the archives are named.
pub const fn this_target() -> &'static str {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "aarch64-apple-darwin"
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        "x86_64-apple-darwin"
    } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        "x86_64-pc-windows-msvc"
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        "x86_64-unknown-linux-gnu"
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        "aarch64-unknown-linux-gnu"
    } else {
        "unknown"
    }
}

/// The archive for this computer, if one is published.
pub fn release() -> Option<&'static Release> {
    RELEASES.iter().find(|r| r.target == this_target())
}

impl Release {
    pub fn url(&self) -> String {
        format!("{RELEASE_BASE}{VERSION}/{}", self.file)
    }
}

/// What `installed.json` records about an installed runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledInfo {
    pub version: String,
    #[serde(default)]
    pub python: Option<String>,
    #[serde(default)]
    pub qt: Option<String>,
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub installed_at: Option<String>,
}

/// An installed runtime.
#[derive(Debug, Clone)]
pub struct Runtime {
    /// The folder holding `python/` and `installed.json`.
    pub dir: PathBuf,
    /// The interpreter.
    pub python: PathBuf,
    pub info: InstalledInfo,
}

/// Where an install is up to.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, rename = "RuntimeProgress"))]
#[serde(tag = "phase", rename_all = "camelCase")]
pub enum Progress {
    Downloading { done: u64, total: u64 },
    Verifying,
    Extracting { done: u64, total: u64 },
    Compiling,
}

/// Where an archive comes from.
pub enum Source {
    Url(String),
    File(PathBuf),
}

/// Name of the environment variable that points at a runtime folder (one
/// holding `python/`), for development and tests.
pub const ENV_OVERRIDE: &str = "MAGNITIDE_PLUGIN_RUNTIME";

fn interpreter(dir: &Path) -> PathBuf {
    if cfg!(windows) { dir.join("python").join("python.exe") } else { dir.join("python").join("bin").join("python3") }
}

impl Runtime {
    /// The runtime at `dir`, if it holds one.
    pub fn from_dir(dir: &Path) -> Option<Runtime> {
        let python = interpreter(dir);
        if !python.is_file() {
            return None;
        }
        let info = std::fs::read(dir.join("installed.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .or_else(|| {
                // A development build: only python/magnitide-runtime.json.
                let b = std::fs::read(dir.join("python").join("magnitide-runtime.json")).ok()?;
                let v: serde_json::Value = serde_json::from_slice(&b).ok()?;
                Some(InstalledInfo {
                    version: v.get("runtime").and_then(|x| x.as_str()).unwrap_or(VERSION).to_owned(),
                    python: v.get("python").and_then(|x| x.as_str()).map(str::to_owned),
                    qt: v.get("qt").and_then(|x| x.as_str()).map(str::to_owned),
                    sha256: None,
                    installed_at: None,
                })
            })
            .unwrap_or(InstalledInfo {
                version: VERSION.into(),
                python: None,
                qt: None,
                sha256: None,
                installed_at: None,
            });
        Some(Runtime { dir: dir.to_owned(), python, info })
    }

    /// The runtime to use: [`ENV_OVERRIDE`] if set, else the one installed in
    /// `base` at this version, else (in debug builds) the development build in
    /// `target/plugin-runtime/current`.
    pub fn locate(base: &Path) -> Option<Runtime> {
        if let Some(dir) = std::env::var_os(ENV_OVERRIDE).filter(|v| !v.is_empty()) {
            return Runtime::from_dir(Path::new(&dir));
        }
        if let Some(r) = Runtime::installed(base) {
            return Some(r);
        }
        #[cfg(debug_assertions)]
        {
            let dev = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/plugin-runtime/current");
            if let Some(r) = Runtime::from_dir(&dev) {
                return Some(r);
            }
        }
        None
    }

    /// The runtime installed in `base` at this version.
    pub fn installed(base: &Path) -> Option<Runtime> {
        Runtime::from_dir(&base.join(VERSION)).filter(|r| r.info.version == VERSION)
    }

    /// Download (or copy), verify, unpack and precompile this version's
    /// runtime into `base`, reporting progress. `cancel` stops it between
    /// chunks, leaving the partial download for next time.
    pub fn install(base: &Path, cancel: &AtomicBool, progress: &mut dyn FnMut(Progress)) -> Result<Runtime, String> {
        let release =
            release().ok_or_else(|| format!("There's no plugin runtime for this computer ({}) yet.", this_target()))?;
        Runtime::install_from(base, &Source::Url(release.url()), release.sha256, release.size, cancel, progress)
    }

    /// [`Runtime::install`] from an archive already on disk (downloaded by
    /// hand, say): it must be this computer's published archive, by hash.
    pub fn install_file(
        base: &Path,
        archive: &Path,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<Runtime, String> {
        let release =
            release().ok_or_else(|| format!("There's no plugin runtime for this computer ({}) yet.", this_target()))?;
        Runtime::install_from(
            base,
            &Source::File(archive.to_path_buf()),
            release.sha256,
            release.size,
            cancel,
            progress,
        )
    }

    /// [`Runtime::install`] from any source, checked against `sha256` (and
    /// `size`, when known).
    pub fn install_from(
        base: &Path,
        source: &Source,
        sha256: &str,
        size: u64,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<Runtime, String> {
        std::fs::create_dir_all(base).map_err(|e| format!("couldn't make {}: {e}", base.display()))?;
        let download = base.join(format!("{VERSION}.download"));
        let tmp = base.join(format!("{VERSION}.tmp"));
        let dest = base.join(VERSION);
        let archive = match source {
            Source::Url(url) => {
                fetch(url, &download, size, cancel, progress)?;
                download.clone()
            }
            Source::File(path) => path.clone(),
        };
        progress(Progress::Verifying);
        let got = sha256_file(&archive)?;
        if got != sha256 {
            let _ = std::fs::remove_file(&download);
            return Err(match source {
                Source::Url(_) => format!(
                    "The downloaded runtime doesn't match what Magnitide expects (sha256 {got}); it wasn't installed."
                ),
                Source::File(path) => format!(
                    "{} isn't the plugin runtime this Magnitide expects for this computer (sha256 {got}); it wasn't installed.",
                    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
                ),
            });
        }
        let _ = std::fs::remove_dir_all(&tmp);
        let result = (|| -> Result<(), String> {
            extract(&archive, &tmp, size, progress)?;
            let python = interpreter(&tmp);
            if !python.is_file() {
                return Err("the runtime archive has no interpreter".into());
            }
            progress(Progress::Compiling);
            precompile(&python, &tmp.join("python"))?;
            let manifest: serde_json::Value = std::fs::read(tmp.join("python").join("magnitide-runtime.json"))
                .ok()
                .and_then(|b| serde_json::from_slice(&b).ok())
                .unwrap_or_default();
            let info = InstalledInfo {
                version: VERSION.into(),
                python: manifest.get("python").and_then(|v| v.as_str()).map(str::to_owned),
                qt: manifest.get("qt").and_then(|v| v.as_str()).map(str::to_owned),
                sha256: Some(sha256.to_owned()),
                installed_at: Some(now_iso()),
            };
            std::fs::write(tmp.join("installed.json"), serde_json::to_vec_pretty(&info).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            Ok(())
        })();
        if let Err(e) = result {
            let _ = std::fs::remove_dir_all(&tmp);
            return Err(e);
        }
        let _ = std::fs::remove_dir_all(&dest);
        std::fs::rename(&tmp, &dest).map_err(|e| format!("couldn't move the runtime into place: {e}"))?;
        let _ = std::fs::remove_file(&download);
        // Other versions are no longer needed.
        if let Ok(entries) = std::fs::read_dir(base) {
            for e in entries.flatten() {
                if e.file_name() != VERSION.as_ref() as &std::ffi::OsStr {
                    let _ = if e.path().is_dir() {
                        std::fs::remove_dir_all(e.path())
                    } else {
                        std::fs::remove_file(e.path())
                    };
                }
            }
        }
        Runtime::from_dir(&dest).ok_or_else(|| "the runtime was installed but can't be read back".into())
    }

    /// Remove every installed version (and partial downloads) from `base`.
    pub fn remove(base: &Path) -> Result<(), String> {
        match std::fs::remove_dir_all(base) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("couldn't remove the runtime: {e}")),
        }
    }

    /// Bytes on disk.
    pub fn size_on_disk(&self) -> u64 {
        dir_size(&self.dir)
    }

    /// Run the interpreter briefly to check it (and Qt) can start: the
    /// versions it reports, or what went wrong (on Linux, usually a missing
    /// system library).
    pub fn check(&self) -> Result<(String, String), String> {
        let out = Command::new(&self.python)
            .args(["-I", "-c", "import sys; from PyQt6 import QtCore, QtWidgets; print(sys.version.split()[0]); print(QtCore.qVersion())"])
            .env("QT_QPA_PLATFORM", "offscreen")
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .output()
            .map_err(|e| format!("couldn't run the plugin runtime ({}): {e}", self.python.display()))?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            let last = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("it didn't start");
            return Err(last.trim().to_owned());
        }
        let text = String::from_utf8_lossy(&out.stdout);
        let mut lines = text.lines();
        let python = lines.next().unwrap_or_default().trim().to_owned();
        let qt = lines.next().unwrap_or_default().trim().to_owned();
        Ok((python, qt))
    }
}

fn now_iso() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    // Days since the epoch to a civil date (Howard Hinnant's algorithm).
    let days = (secs / 86_400) as i64;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    let t = secs % 86_400;
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", t / 3600, (t % 3600) / 60, t % 60)
}

fn dir_size(dir: &Path) -> u64 {
    let mut total = 0;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let Ok(meta) = e.metadata() else { continue };
            if meta.is_dir() {
                total += dir_size(&e.path());
            } else if meta.is_file() {
                total += meta.len();
            }
        }
    }
    total
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut f = std::fs::File::open(path).map_err(|e| format!("couldn't read {}: {e}", path.display()))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(format!("{:x}", h.finalize()))
}

/// Download `url` to `dest`, resuming a partial file there if the server
/// allows (GitHub does), reporting progress against `expected` bytes.
fn fetch(
    url: &str,
    dest: &Path,
    expected: u64,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(Progress),
) -> Result<(), String> {
    let mut done = std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0);
    if expected > 0 && done >= expected {
        return Ok(());
    }
    let mut req = ureq::get(url);
    if done > 0 {
        req = req.header("Range", &format!("bytes={done}-"));
    }
    let mut resp = req.call().map_err(|e| format!("couldn't download the plugin runtime: {e}"))?;
    let status = resp.status().as_u16();
    let mut file = match status {
        206 => std::fs::OpenOptions::new().append(true).open(dest),
        200 => {
            done = 0;
            std::fs::File::create(dest)
        }
        s => return Err(format!("couldn't download the plugin runtime: HTTP {s}")),
    }
    .map_err(|e| format!("couldn't write the download: {e}"))?;
    let total = if expected > 0 {
        expected
    } else {
        resp.headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .map_or(0, |len| len + done)
    };
    let mut reader = resp.body_mut().as_reader();
    let mut buf = vec![0u8; 256 * 1024];
    progress(Progress::Downloading { done, total });
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err("The download was cancelled.".into());
        }
        let n = reader.read(&mut buf).map_err(|e| format!("the download stopped: {e}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| format!("couldn't write the download: {e}"))?;
        done += n as u64;
        progress(Progress::Downloading { done, total });
    }
    file.flush().map_err(|e| e.to_string())?;
    if expected > 0 && done != expected {
        return Err(format!("The download is incomplete ({done} of {expected} bytes); try again."));
    }
    Ok(())
}

/// Unpack a `.tar.zst` into `into` (which must not exist), refusing paths that
/// escape it. `size` is the archive's size, for progress.
fn extract(archive: &Path, into: &Path, size: u64, progress: &mut dyn FnMut(Progress)) -> Result<(), String> {
    std::fs::create_dir_all(into).map_err(|e| e.to_string())?;
    let file = std::fs::File::open(archive).map_err(|e| e.to_string())?;
    let total = if size > 0 { size } else { file.metadata().map(|m| m.len()).unwrap_or(0) };
    let counted = Counting { inner: std::io::BufReader::new(file), read: Default::default() };
    let counter = counted.shared();
    let decoder = zstd::Decoder::new(counted).map_err(|e| format!("couldn't read the archive: {e}"))?;
    let mut tar = tar::Archive::new(decoder);
    tar.set_preserve_permissions(true);
    tar.set_unpack_xattrs(false);
    let entries = tar.entries().map_err(|e| format!("couldn't read the archive: {e}"))?;
    let mut n = 0u64;
    for entry in entries {
        let mut entry = entry.map_err(|e| format!("couldn't read the archive: {e}"))?;
        let path = entry.path().map_err(|e| e.to_string())?.into_owned();
        if path.components().any(|c| matches!(c, std::path::Component::ParentDir | std::path::Component::RootDir)) {
            return Err(format!("the archive holds an unsafe path: {}", path.display()));
        }
        entry.unpack_in(into).map_err(|e| format!("couldn't unpack {}: {e}", path.display()))?;
        n += 1;
        if n.is_multiple_of(200) {
            progress(Progress::Extracting { done: counter.load(Ordering::Relaxed).min(total), total });
        }
    }
    progress(Progress::Extracting { done: total, total });
    Ok(())
}

/// A reader that counts what passed through it (progress for extraction).
struct Counting<R> {
    inner: R,
    read: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

impl<R> Counting<R> {
    fn shared(&self) -> std::sync::Arc<std::sync::atomic::AtomicU64> {
        self.read.clone()
    }
}

impl<R: Read> Read for Counting<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.read.fetch_add(n as u64, Ordering::Relaxed);
        Ok(n)
    }
}

/// Compile every module once (`unchecked-hash` pycs: never re-checked, as the
/// files never change), so the host starts quickly.
fn precompile(python: &Path, dir: &Path) -> Result<(), String> {
    if std::env::var_os("MAGNITIDE_RUNTIME_NO_COMPILE").is_some() {
        return Ok(());
    }
    let out = Command::new(python)
        .args(["-I", "-m", "compileall", "-q", "-j0", "-f", "--invalidation-mode", "unchecked-hash"])
        .arg(dir)
        .output()
        .map_err(|e| format!("couldn't run the runtime's interpreter: {e}"))?;
    // Failures here are individual modules that don't compile; the runtime
    // still works, so they're only noted.
    if !out.status.success() {
        eprintln!("[plugin runtime] compileall: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny archive with a fake interpreter (a script answering compileall).
    fn fixture_archive(dir: &Path) -> (PathBuf, String, u64) {
        let root = dir.join("src");
        let bin = root.join("python").join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("python3"), "#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(bin.join("python3"), std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        std::fs::write(
            root.join("python").join("magnitide-runtime.json"),
            r#"{"runtime":"1","python":"3.14.8","qt":"6.11.2"}"#,
        )
        .unwrap();
        let archive = dir.join("runtime.tar.zst");
        let file = std::fs::File::create(&archive).unwrap();
        let mut enc = zstd::Encoder::new(file, 3).unwrap();
        {
            let mut tar = tar::Builder::new(&mut enc);
            tar.append_dir_all("python", root.join("python")).unwrap();
            tar.finish().unwrap();
        }
        enc.finish().unwrap();
        let size = std::fs::metadata(&archive).unwrap().len();
        (archive.clone(), sha256_file(&archive).unwrap(), size)
    }

    #[test]
    fn installs_atomically_and_reads_back() {
        let dir = tempfile::tempdir().unwrap();
        let (archive, sha, size) = fixture_archive(dir.path());
        let base = dir.path().join("base");
        let mut phases = Vec::new();
        let cancel = AtomicBool::new(false);
        unsafe { std::env::set_var("MAGNITIDE_RUNTIME_NO_COMPILE", "1") };
        let rt =
            Runtime::install_from(&base, &Source::File(archive.clone()), &sha, size, &cancel, &mut |p| phases.push(p))
                .unwrap();
        assert_eq!(rt.dir, base.join(VERSION));
        assert!(rt.python.is_file());
        assert_eq!(rt.info.qt.as_deref(), Some("6.11.2"));
        assert_eq!(rt.info.sha256.as_deref(), Some(sha.as_str()));
        assert!(phases.contains(&Progress::Verifying) && phases.contains(&Progress::Compiling));
        assert!(!base.join(format!("{VERSION}.tmp")).exists());
        assert_eq!(Runtime::installed(&base).unwrap().dir, rt.dir);
        assert!(rt.size_on_disk() > 0);

        // Installing again replaces it; a stale other version is pruned.
        std::fs::create_dir_all(base.join("0")).unwrap();
        Runtime::install_from(&base, &Source::File(archive), &sha, size, &cancel, &mut |_| {}).unwrap();
        assert!(!base.join("0").exists());

        Runtime::remove(&base).unwrap();
        assert!(Runtime::installed(&base).is_none());
        Runtime::remove(&base).unwrap();
    }

    #[test]
    fn a_file_that_isnt_this_computers_runtime_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let junk = dir.path().join("magnitide-plugin-runtime-1-nope.tar.zst");
        std::fs::write(&junk, b"not an archive").unwrap();
        let base = dir.path().join("base");
        let err = Runtime::install_file(&base, &junk, &AtomicBool::new(false), &mut |_| {}).unwrap_err();
        // No release for this target, or the wrong file for it: either way nothing is installed.
        assert!(
            err.contains("isn't the plugin runtime") || err.contains("no plugin runtime for this computer"),
            "{err}"
        );
        assert!(junk.exists(), "the user's file is kept");
        assert!(!base.join(VERSION).exists());
    }

    #[test]
    fn a_wrong_hash_installs_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let (archive, _sha, size) = fixture_archive(dir.path());
        let base = dir.path().join("base");
        let err = Runtime::install_from(
            &base,
            &Source::File(archive),
            "0000000000000000000000000000000000000000000000000000000000000000",
            size,
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap_err();
        assert!(err.contains("isn't the plugin runtime this Magnitide expects"), "{err}");
        assert!(!base.join(VERSION).exists() && !base.join(format!("{VERSION}.tmp")).exists());
    }

    #[test]
    fn unsafe_paths_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("evil.tar.zst");
        let file = std::fs::File::create(&archive).unwrap();
        let mut enc = zstd::Encoder::new(file, 3).unwrap();
        {
            let mut tar = tar::Builder::new(&mut enc);
            let data = b"x";
            let mut header = tar::Header::new_gnu();
            header.set_size(1);
            header.set_mode(0o644);
            // The tar crate refuses to write such a path; forge it.
            let name = b"python/../../escape";
            header.as_gnu_mut().unwrap().name[..name.len()].copy_from_slice(name);
            header.set_cksum();
            tar.append(&header, &data[..]).unwrap();
            tar.finish().unwrap();
        }
        enc.finish().unwrap();
        let err = extract(&archive, &dir.path().join("out"), 0, &mut |_| {}).unwrap_err();
        assert!(err.contains("unsafe path"), "{err}");
    }

    #[test]
    fn releases_match_the_published_sums() {
        let sums = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/plugin-runtime/sha256sums.txt"),
        )
        .unwrap_or_default();
        let published: Vec<(&str, &str)> = sums
            .lines()
            .filter_map(|l| {
                let mut it = l.split_whitespace();
                Some((it.next()?, it.next()?))
            })
            .collect();
        assert_eq!(RELEASES.len(), published.len(), "RELEASES and sha256sums.txt disagree");
        for r in RELEASES {
            assert!(published.contains(&(r.sha256, r.file)), "{} isn't in sha256sums.txt", r.file);
            assert!(r.file.contains(&format!("-{VERSION}-")), "{} isn't version {VERSION}", r.file);
        }
        let version =
            std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/plugin-runtime/VERSION"))
                .unwrap_or_default();
        assert_eq!(version.trim(), VERSION, "scripts/plugin-runtime/VERSION and runtime::VERSION disagree");
    }

    #[test]
    fn target_is_known() {
        assert_ne!(this_target(), "unknown");
        assert!(now_iso().ends_with('Z') && now_iso().len() == 20);
    }
}
