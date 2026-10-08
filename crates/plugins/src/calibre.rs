//! An installed calibre as the plugin host: plugins run on calibre's own
//! code under `calibre-debug`, with only the host package (the protocol,
//! the window stand-in, the hooks) from Magnitide. The alternative is
//! Magnitide's own runtime ([`crate::runtime`]) with the compat package.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

use crate::runtime::Progress;

/// The oldest calibre the host package is written against (PyQt6, the
/// `calibre.db` API with `new_api`, `InterfaceAction` as plugins use it).
pub const MIN_CALIBRE: (u32, u32) = (6, 0);

/// Which host runs plugins: Settings' choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum HostChoice {
    /// An installed calibre when there is one, else Magnitide's runtime.
    #[default]
    Auto,
    /// Only an installed calibre.
    Calibre,
    /// Only the calibre Magnitide downloaded.
    Downloaded,
    /// Only Magnitide's runtime.
    Runtime,
}

/// An installed calibre, found or given.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct CalibreInstall {
    /// `calibre-debug`, which runs the host inside calibre's Python.
    pub calibre_debug: String,
    /// "9.15.0" as `calibre-debug --version` reports it.
    pub version: String,
    /// Too old for the host package (see [`MIN_CALIBRE`]), with why.
    pub problem: Option<String>,
}

/// `calibre-debug` beside an `ebook-convert`.
pub fn calibre_debug_beside(ebook_convert: &Path) -> PathBuf {
    let name = if cfg!(windows) { "calibre-debug.exe" } else { "calibre-debug" };
    ebook_convert.with_file_name(name)
}

/// The installed calibre, if any: beside the `ebook-convert` conversion
/// already finds (the usual install locations, then PATH). Never runs
/// calibre unless one is found; then only `--version`.
pub fn find_installed() -> Option<CalibreInstall> {
    let converter = magnitide_core::convert::Converter::detect();
    let debug = calibre_debug_beside(converter.calibre()?);
    if !debug.is_file() {
        return None;
    }
    Some(inspect(&debug))
}

/// What a `calibre-debug` is: its version, and whether it's recent enough.
pub fn inspect(calibre_debug: &Path) -> CalibreInstall {
    let version = Command::new(calibre_debug)
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| parse_version(&s))
        .unwrap_or_default();
    let problem = if version.is_empty() {
        Some("calibre-debug didn't report a version; is calibre installed correctly?".into())
    } else if !recent_enough(&version) {
        Some(format!(
            "calibre {version} is too old; Magnitide's plugin host needs calibre {}.{} or later.",
            MIN_CALIBRE.0, MIN_CALIBRE.1
        ))
    } else {
        None
    };
    CalibreInstall { calibre_debug: calibre_debug.display().to_string(), version, problem }
}

/// "calibre-debug (calibre 9.15)" → "9.15"; "9.15.0" when a third part is printed.
fn parse_version(output: &str) -> Option<String> {
    let start = output.find("calibre ")? + "calibre ".len();
    let rest = &output[start..];
    let v: String = rest.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    (!v.is_empty()).then_some(v)
}

fn recent_enough(version: &str) -> bool {
    let mut parts = version.split('.').map(|p| p.parse::<u32>().unwrap_or(0));
    let (major, minor) = (parts.next().unwrap_or(0), parts.next().unwrap_or(0));
    (major, minor) >= MIN_CALIBRE
}

// --- calibre downloaded by Magnitide -------------------------------------------

/// The calibre version Magnitide downloads (scripts/calibre-release/calibre.json).
pub const CALIBRE_VERSION: &str = "9.15.0";

/// One of calibre's official release files, pinned by its published SHA-512.
#[derive(Debug, Clone, Copy)]
pub struct CalibreRelease {
    pub target: &'static str,
    pub file: &'static str,
    pub sha512: &'static str,
    pub size: u64,
}

pub const CALIBRE_RELEASES: &[CalibreRelease] = &[
    CalibreRelease {
        target: "aarch64-apple-darwin",
        file: "calibre-9.15.0.dmg",
        sha512: "6475fa3deadd4cc9cd2d0eac14ef7fb042c0e2d10837cd304151997d699401cd6d37334c189e311198b84e0b4db1f4438d8ae32cebbf758957b0208b7523a4f4",
        size: 350293492,
    },
    CalibreRelease {
        target: "aarch64-unknown-linux-gnu",
        file: "calibre-9.15.0-arm64.txz",
        sha512: "725bae1ccbc5501df9cb92c1df4df9d335cd470dd8681c099bf7c1c7a0c05845f139a066c22dc38374c58fde0c4c2df21e9139e1a25d4917e0fd2183f2fcbafb",
        size: 182488416,
    },
    CalibreRelease {
        target: "x86_64-apple-darwin",
        file: "calibre-9.15.0.dmg",
        sha512: "6475fa3deadd4cc9cd2d0eac14ef7fb042c0e2d10837cd304151997d699401cd6d37334c189e311198b84e0b4db1f4438d8ae32cebbf758957b0208b7523a4f4",
        size: 350293492,
    },
    CalibreRelease {
        target: "x86_64-pc-windows-msvc",
        file: "calibre-64bit-9.15.0.msi",
        sha512: "960ec56105a08952a8c03cd0dccacd82a19e7045c5e10ed1a9977d118a64bcf16491e3e9cedc4a08268dc714547a41910a34f2beb247f8f4a5ce3bcd5d145ebe",
        size: 226549760,
    },
    CalibreRelease {
        target: "x86_64-unknown-linux-gnu",
        file: "calibre-9.15.0-x86_64.txz",
        sha512: "77fb209188906a1be5ca1e8147339bd06242414ffbfe7dbc54e65667c8d6057fdc6e709efff0c415c42fb95ca5f53359ae041d5089cd9927a0d1d74d9fc8f4a5",
        size: 194998300,
    },
];

/// The release file for this computer, if calibre publishes one.
pub fn calibre_release() -> Option<&'static CalibreRelease> {
    CALIBRE_RELEASES.iter().find(|r| r.target == crate::runtime::this_target())
}

impl CalibreRelease {
    /// Where to download from, in order: calibre's GitHub release, then
    /// calibre's own server (its "alternate download" the other way round).
    pub fn urls(&self) -> [String; 2] {
        [
            format!("https://github.com/kovidgoyal/calibre/releases/download/v{CALIBRE_VERSION}/{}", self.file),
            format!("https://download.calibre-ebook.com/{CALIBRE_VERSION}/{}", self.file),
        ]
    }
}

/// What `installed.json` records in a downloaded calibre's folder.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct DownloadedInfo {
    version: String,
    /// `calibre-debug`, relative to the folder.
    calibre_debug: String,
    #[serde(default)]
    sha512: Option<String>,
}

/// The status of the calibre Magnitide downloads, for Settings.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct CalibreDownloadStatus {
    pub version: String,
    /// Whether calibre publishes a build for this computer.
    pub available: bool,
    pub installed: bool,
    pub installing: bool,
    pub path: Option<String>,
    /// Bytes on disk, when installed.
    pub size: Option<u64>,
    /// The download's size, when not installed and available.
    pub download_size: Option<u64>,
}

/// `<base>/calibre/<CALIBRE_VERSION>`.
fn downloaded_dir(base: &Path) -> PathBuf {
    base.join("calibre").join(CALIBRE_VERSION)
}

/// The calibre Magnitide downloaded, if it's installed.
pub fn locate_downloaded(base: &Path) -> Option<CalibreInstall> {
    let dir = downloaded_dir(base);
    let info: DownloadedInfo = serde_json::from_slice(&std::fs::read(dir.join("installed.json")).ok()?).ok()?;
    let debug = dir.join(&info.calibre_debug);
    if !debug.is_file() {
        return None;
    }
    Some(CalibreInstall { calibre_debug: debug.display().to_string(), version: info.version, problem: None })
}

pub fn downloaded_size(base: &Path) -> u64 {
    crate::runtime::dir_size(&downloaded_dir(base))
}

/// Download calibre's official build for this computer and unpack it
/// under `base`, checked against calibre's published SHA-512, atomically.
pub fn install_downloaded(
    base: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(Progress),
) -> Result<CalibreInstall, String> {
    let release = calibre_release()
        .ok_or_else(|| format!("calibre publishes no build for this computer ({}).", crate::runtime::this_target()))?;
    let root = base.join("calibre");
    std::fs::create_dir_all(&root).map_err(|e| format!("couldn't make {}: {e}", root.display()))?;
    let download = root.join(format!("{CALIBRE_VERSION}.download"));
    let tmp = root.join(format!("{CALIBRE_VERSION}.tmp"));
    let dest = downloaded_dir(base);
    let mut last_error = String::new();
    let mut fetched = false;
    for url in release.urls() {
        match crate::runtime::fetch(&url, &download, release.size, cancel, progress, "calibre") {
            Ok(()) => {
                fetched = true;
                break;
            }
            Err(e) if cancel.load(Ordering::SeqCst) => return Err(e),
            Err(e) => last_error = e,
        }
    }
    if !fetched {
        return Err(last_error);
    }
    progress(Progress::Verifying);
    let got = crate::runtime::sha512_file(&download)?;
    if got != release.sha512 {
        let _ = std::fs::remove_file(&download);
        return Err(
            "The downloaded calibre doesn't match what calibre published (SHA-512); it wasn't installed.".into()
        );
    }
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let result = (|| -> Result<PathBuf, String> {
        progress(Progress::Extracting { done: 0, total: 1 });
        unpack(&download, release.file, &tmp, cancel)?;
        let debug = find_calibre_debug(&tmp).ok_or("calibre-debug wasn't in the download")?;
        let rel = debug.strip_prefix(&tmp).map_err(|e| e.to_string())?.to_path_buf();
        let info = DownloadedInfo {
            version: CALIBRE_VERSION.into(),
            calibre_debug: rel.to_string_lossy().replace('\\', "/"),
            sha512: Some(got.clone()),
        };
        std::fs::write(tmp.join("installed.json"), serde_json::to_vec_pretty(&info).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        Ok(rel)
    })();
    let rel = match result {
        Ok(rel) => rel,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&tmp);
            return Err(e);
        }
    };
    let _ = std::fs::remove_dir_all(&dest);
    std::fs::rename(&tmp, &dest).map_err(|e| format!("couldn't move calibre into place: {e}"))?;
    let _ = std::fs::remove_file(&download);
    // Older versions go.
    if let Ok(entries) = std::fs::read_dir(&root) {
        for e in entries.flatten() {
            if e.path() != dest {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
    }
    let debug = dest.join(rel);
    let mut install = inspect(&debug);
    if install.version.is_empty() {
        install.version = CALIBRE_VERSION.into();
    }
    Ok(install)
}

/// Remove the downloaded calibre (every version under `base/calibre`).
pub fn remove_downloaded(base: &Path) -> Result<(), String> {
    let root = base.join("calibre");
    if root.exists() {
        std::fs::remove_dir_all(&root).map_err(|e| format!("couldn't remove {}: {e}", root.display()))?;
    }
    Ok(())
}

fn find_calibre_debug(dir: &Path) -> Option<PathBuf> {
    let name = if cfg!(windows) { "calibre-debug.exe" } else { "calibre-debug" };
    fn walk(dir: &Path, name: &str, depth: usize) -> Option<PathBuf> {
        if depth > 6 {
            return None;
        }
        let mut dirs = Vec::new();
        for e in std::fs::read_dir(dir).ok()?.flatten() {
            let p = e.path();
            if p.is_symlink() {
                continue;
            }
            if p.is_file() && p.file_name().is_some_and(|n| n == name) {
                return Some(p);
            }
            if p.is_dir() {
                dirs.push(p);
            }
        }
        dirs.into_iter().find_map(|d| walk(&d, name, depth + 1))
    }
    walk(dir, name, 0)
}

/// Unpack calibre's release file for this platform into `into`.
fn unpack(archive: &Path, file: &str, into: &Path, cancel: &AtomicBool) -> Result<(), String> {
    if file.ends_with(".txz") {
        return unpack_txz(archive, into, cancel);
    }
    if file.ends_with(".dmg") {
        return unpack_dmg(archive, into);
    }
    if file.ends_with(".msi") {
        return unpack_msi(archive, into);
    }
    Err(format!("don't know how to unpack {file}"))
}

/// calibre's Linux build: a tar.xz with calibre-debug at its root.
fn unpack_txz(archive: &Path, into: &Path, cancel: &AtomicBool) -> Result<(), String> {
    let f = std::fs::File::open(archive).map_err(|e| e.to_string())?;
    let decoder = liblzma::read::XzDecoder::new(std::io::BufReader::new(f));
    let mut ar = tar::Archive::new(decoder);
    ar.set_preserve_permissions(true);
    for entry in ar.entries().map_err(|e| format!("couldn't read the calibre archive: {e}"))? {
        if cancel.load(Ordering::SeqCst) {
            return Err("The install was cancelled.".into());
        }
        let mut entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path().map_err(|e| e.to_string())?.into_owned();
        if path.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
            return Err(format!("the calibre archive holds an unsafe path: {}", path.display()));
        }
        entry.unpack_in(into).map_err(|e| format!("couldn't unpack {}: {e}", path.display()))?;
    }
    Ok(())
}

/// calibre's macOS build: a disk image holding calibre.app. Mounted with
/// hdiutil, the app copied out (symlinks and all), unmounted again.
fn unpack_dmg(archive: &Path, into: &Path) -> Result<(), String> {
    let mount = into.join(".mount");
    std::fs::create_dir_all(&mount).map_err(|e| e.to_string())?;
    let out = Command::new("hdiutil")
        .args(["attach", "-nobrowse", "-readonly", "-noautoopen", "-mountpoint"])
        .arg(&mount)
        .arg(archive)
        .output()
        .map_err(|e| format!("couldn't run hdiutil: {e}"))?;
    if !out.status.success() {
        return Err(format!("couldn't open the calibre disk image: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    let result = (|| -> Result<(), String> {
        let app = std::fs::read_dir(&mount)
            .map_err(|e| e.to_string())?
            .flatten()
            .map(|e| e.path())
            .find(|p| p.extension().is_some_and(|x| x == "app"))
            .ok_or("no app in the calibre disk image")?;
        let copy =
            Command::new("cp").arg("-R").arg(&app).arg(into.join("calibre.app")).output().map_err(|e| e.to_string())?;
        if !copy.status.success() {
            return Err(format!("couldn't copy calibre: {}", String::from_utf8_lossy(&copy.stderr).trim()));
        }
        Ok(())
    })();
    let _ = Command::new("hdiutil").args(["detach", "-force"]).arg(&mount).output();
    let _ = std::fs::remove_dir_all(&mount);
    result
}

/// calibre's Windows build: an MSI, unpacked by msiexec's administrative
/// install (no installation, no elevation).
fn unpack_msi(archive: &Path, into: &Path) -> Result<(), String> {
    let target = std::fs::canonicalize(into).unwrap_or_else(|_| into.to_path_buf());
    let out = Command::new("msiexec")
        .arg("/a")
        .arg(archive)
        .arg("/qn")
        .arg(format!("TARGETDIR={}", target.display()))
        .output()
        .map_err(|e| format!("couldn't run msiexec: {e}"))?;
    if !out.status.success() {
        return Err(format!("couldn't unpack the calibre installer (msiexec exit {})", out.status));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_versions() {
        assert_eq!(parse_version("calibre-debug (calibre 9.15)\n").as_deref(), Some("9.15"));
        assert_eq!(parse_version("calibre-debug (calibre 9.15.1)").as_deref(), Some("9.15.1"));
        assert_eq!(parse_version("nothing here"), None);
        assert!(recent_enough("9.15") && recent_enough("6.0.0") && !recent_enough("5.44"));
    }

    #[test]
    fn releases_match_the_pinned_json() {
        let json: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(
                Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/calibre-release/calibre.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(json["version"], CALIBRE_VERSION);
        let files = json["files"].as_object().unwrap();
        assert_eq!(files.len(), CALIBRE_RELEASES.len());
        for r in CALIBRE_RELEASES {
            let f = &files[r.target];
            assert_eq!(
                (f["file"].as_str(), f["sha512"].as_str(), f["size"].as_u64()),
                (Some(r.file), Some(r.sha512), Some(r.size)),
                "{}",
                r.target
            );
            assert!(
                r.urls()[0].contains("github.com/kovidgoyal/calibre")
                    && r.urls()[1].contains("download.calibre-ebook.com")
            );
        }
        assert!(calibre_release().is_some(), "this computer's target should be pinned");
    }

    #[test]
    fn a_downloaded_calibre_is_found_by_its_record() {
        let dir = tempfile::tempdir().unwrap();
        assert!(locate_downloaded(dir.path()).is_none());
        let d = downloaded_dir(dir.path());
        std::fs::create_dir_all(d.join("calibre/bin")).unwrap();
        let debug = d.join("calibre/bin").join(if cfg!(windows) { "calibre-debug.exe" } else { "calibre-debug" });
        std::fs::write(&debug, "").unwrap();
        let info = DownloadedInfo {
            version: CALIBRE_VERSION.into(),
            calibre_debug: "calibre/bin/calibre-debug".into(),
            sha512: None,
        };
        std::fs::write(d.join("installed.json"), serde_json::to_vec(&info).unwrap()).unwrap();
        let found = locate_downloaded(dir.path()).unwrap();
        assert_eq!(found.version, CALIBRE_VERSION);
        assert!(found.calibre_debug.ends_with("calibre-debug") || found.calibre_debug.ends_with("calibre-debug.exe"));
        assert_eq!(find_calibre_debug(&d).unwrap(), debug);
        remove_downloaded(dir.path()).unwrap();
        assert!(locate_downloaded(dir.path()).is_none());
    }

    #[test]
    fn calibre_debug_is_beside_ebook_convert() {
        let p = calibre_debug_beside(Path::new("/Applications/calibre.app/Contents/MacOS/ebook-convert"));
        assert!(p.ends_with(if cfg!(windows) { "calibre-debug.exe" } else { "calibre-debug" }));
        assert_eq!(p.parent(), Path::new("/Applications/calibre.app/Contents/MacOS").into());
    }
}
