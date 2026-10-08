//! Hashes the compat package (`python/`), which is embedded in the binary and
//! extracted beside the plugins at run time under a folder named by this hash,
//! so an updated Magnitide uses its own copy and never a stale one.

use std::path::Path;

use sha2::{Digest, Sha256};

fn hash_dir(dir: &Path, h: &mut Sha256) {
    let mut entries: Vec<_> = std::fs::read_dir(dir).map(|r| r.flatten().collect()).unwrap_or_default();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let path = e.path();
        let name = e.file_name();
        if name == "__pycache__" || name.to_string_lossy().ends_with(".pyc") {
            continue;
        }
        if path.is_dir() {
            hash_dir(&path, h);
        } else if let Ok(bytes) = std::fs::read(&path) {
            h.update(path.strip_prefix(dir).unwrap_or(&path).to_string_lossy().as_bytes());
            h.update(bytes);
        }
    }
}

fn main() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("python");
    println!("cargo:rerun-if-changed=python");
    let mut h = Sha256::new();
    hash_dir(&dir, &mut h);
    let digest = h.finalize();
    let short: String = digest.iter().take(8).map(|b| format!("{b:02x}")).collect();
    println!("cargo:rustc-env=MAGNITIDE_COMPAT_HASH={short}");
}
