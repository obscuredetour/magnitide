//! An installed calibre as the plugin host: plugins run on calibre's own
//! code under `calibre-debug`, with only the host package (the protocol,
//! the window stand-in, the hooks) from Magnitide. The alternative is
//! Magnitide's own runtime ([`crate::runtime`]) with the compat package.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

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
    fn calibre_debug_is_beside_ebook_convert() {
        let p = calibre_debug_beside(Path::new("/Applications/calibre.app/Contents/MacOS/ebook-convert"));
        assert!(p.ends_with(if cfg!(windows) { "calibre-debug.exe" } else { "calibre-debug" }));
        assert_eq!(p.parent(), Path::new("/Applications/calibre.app/Contents/MacOS").into());
    }
}
