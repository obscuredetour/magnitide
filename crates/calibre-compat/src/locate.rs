//! Find the library calibre was last using, from calibre's own config.

use std::env;
use std::path::PathBuf;

/// calibre's config folder, honouring `CALIBRE_CONFIG_DIRECTORY` like calibre does.
pub fn calibre_config_dir() -> Option<PathBuf> {
    if let Some(dir) = env::var_os("CALIBRE_CONFIG_DIRECTORY") {
        return Some(dir.into());
    }
    #[cfg(target_os = "macos")]
    {
        env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Preferences/calibre"))
    }
    #[cfg(windows)]
    {
        env::var_os("APPDATA").map(|a| PathBuf::from(a).join("calibre"))
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .map(|c| c.join("calibre"))
    }
}

/// The library calibre last used (`library_path` in `global.py.json`),
/// falling back to calibre's default location, `~/Calibre Library`.
pub fn default_library() -> Option<PathBuf> {
    let is_library = |p: &PathBuf| p.join("metadata.db").is_file();
    configured_library().filter(is_library).or_else(|| {
        let home = env::var_os("HOME").or_else(|| env::var_os("USERPROFILE"))?;
        Some(PathBuf::from(home).join("Calibre Library")).filter(is_library)
    })
}

fn configured_library() -> Option<PathBuf> {
    let cfg = std::fs::read_to_string(calibre_config_dir()?.join("global.py.json")).ok()?;
    let json: serde_json::Value = serde_json::from_str(&cfg).ok()?;
    Some(PathBuf::from(json.get("library_path")?.as_str()?))
}
