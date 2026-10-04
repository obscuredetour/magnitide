//! E-readers connected as USB storage: recognizing them, listing the books
//! on them, and matching those to library books.
//!
//! Recognized: Kobo (a `.kobo` folder), Kindle (`system` and `documents`
//! folders), and any volume calibre manages (`metadata.calibre` or
//! `driveinfo.calibre`). Newer Kindles that connect over MTP don't mount as
//! storage and aren't seen. Nothing on a device is ever written here.
//!
//! Matching, strongest first: the book's uuid (from calibre's device list
//! or inside the EPUB), its ISBN, an identical file, then title and author.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::hash::{DefaultHasher, Hasher};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use magnitide_catalog::matching::{author_similarity, isbn13, normalize_title};
use magnitide_catalog::{BookId, Catalog};
use serde::Serialize;

use crate::import::files::format_of;
use crate::metadata;

pub mod volumes;

/// Stop listing a device after this many book files.
const MAX_FILES: usize = 50_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum DeviceKind {
    Kobo,
    Kindle,
    /// Another device calibre has managed.
    Generic,
}

/// A recognized device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub root: PathBuf,
    pub kind: DeviceKind,
    pub name: String,
}

/// Every connected device.
pub fn connected() -> Vec<Device> {
    volumes::mounted().into_iter().filter_map(|root| detect(&root)).collect()
}

/// What kind of device is mounted at `root`, if any.
pub fn detect(root: &Path) -> Option<Device> {
    let label = root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let (kind, default) = if root.join(".kobo").is_dir() {
        (DeviceKind::Kobo, "Kobo")
    } else if root.join("system").is_dir() && root.join("documents").is_dir() {
        (DeviceKind::Kindle, "Kindle")
    } else if root.join("metadata.calibre").is_file() || root.join("driveinfo.calibre").is_file() {
        (DeviceKind::Generic, "E-reader")
    } else {
        return None;
    };
    // Keep a name the owner gave the volume; replace the factory ones.
    let generic_label = label.is_empty() || ["koboereader", "kindle"].contains(&label.to_lowercase().as_str());
    let name = if generic_label { default.to_owned() } else { label };
    Some(Device { root: root.to_path_buf(), kind, name })
}

/// A book file on a device, and what it says about itself.
#[derive(Debug)]
pub struct DeviceFile {
    /// Relative to the device root, '/'-separated.
    pub path: String,
    pub format: String,
    pub size: u64,
    pub title: Option<String>,
    pub authors: Vec<String>,
    pub isbn: Option<String>,
    /// Lower case.
    pub uuids: Vec<String>,
    sample: OnceLock<Option<u64>>,
}

/// A device's book files, as last scanned.
#[derive(Debug)]
pub struct Scan {
    pub device: Device,
    pub files: Vec<DeviceFile>,
    /// (total, free) bytes.
    pub space: Option<(u64, u64)>,
}

/// List the books on a device. Reads EPUB metadata; other formats are
/// recognized by file name ("Title - Author.azw3", calibre's naming).
pub fn scan(device: &Device) -> Scan {
    let calibre_uuids = calibre_device_list(&device.root);
    let (start, skip): (PathBuf, &[&str]) = match device.kind {
        DeviceKind::Kindle => (device.root.join("documents"), &["dictionaries"]),
        DeviceKind::Kobo | DeviceKind::Generic => (device.root.clone(), &[]),
    };
    let mut paths = Vec::new();
    walk(&start, skip, 0, &mut paths);

    let files = paths
        .into_iter()
        .filter_map(|(path, format)| {
            let rel = path.strip_prefix(&device.root).ok()?;
            let rel = rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/");
            let size = path.metadata().ok()?.len();
            let meta = metadata::read(&path, &format);
            let mut uuids = meta.uuids;
            uuids.extend(calibre_uuids.get(&rel).cloned());
            Some(DeviceFile {
                format,
                size,
                title: meta.title,
                authors: meta.authors.into_iter().map(|a| a.name).collect(),
                isbn: meta.identifiers.get("isbn").and_then(|i| isbn13(i)),
                uuids,
                sample: OnceLock::new(),
                path: rel,
            })
        })
        .collect();
    Scan { space: volumes::space(&device.root), device: device.clone(), files }
}

fn walk(dir: &Path, skip: &[&str], depth: usize, out: &mut Vec<(PathBuf, String)>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        if out.len() >= MAX_FILES {
            return;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        // Hidden and system folders, macOS "._" sidecar files, Kindle .sdr folders.
        if name.starts_with('.') || name.ends_with(".sdr") || skip.contains(&name.to_lowercase().as_str()) {
            continue;
        }
        let Ok(kind) = entry.file_type() else { continue };
        let path = entry.path();
        if kind.is_dir() && depth < 6 {
            walk(&path, skip, depth + 1, out);
        } else if kind.is_file()
            && let Some(format) = format_of(&path)
        {
            out.push((path, format));
        }
    }
}

/// calibre's list of what it put on the device: path → book uuid.
fn calibre_device_list(root: &Path) -> HashMap<String, String> {
    #[derive(serde::Deserialize)]
    struct Entry {
        lpath: Option<String>,
        uuid: Option<String>,
    }
    let Ok(bytes) = std::fs::read(root.join("metadata.calibre")) else { return HashMap::new() };
    let entries: Vec<Entry> = serde_json::from_slice(&bytes).unwrap_or_default();
    entries.into_iter().filter_map(|e| Some((e.lpath?.replace('\\', "/"), e.uuid?.to_ascii_lowercase()))).collect()
}

/// How a device file was matched to a library book.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum MatchedBy {
    Uuid,
    Isbn,
    /// Identical file.
    File,
    /// Title and author.
    Title,
}

/// The library book each scanned file is, if any (same order as `scan.files`).
pub fn match_books(scan: &Scan, catalog: &Catalog, library_root: &Path) -> Vec<Option<(BookId, MatchedBy)>> {
    let mut by_uuid = HashMap::new();
    let mut by_isbn = HashMap::new();
    let mut by_size: HashMap<(u64, &str), Vec<(BookId, PathBuf)>> = HashMap::new();
    let mut by_title: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, b) in catalog.books.iter().enumerate() {
        if let Some(u) = &b.uuid {
            by_uuid.insert(u.to_ascii_lowercase(), b.id);
        }
        if let Some(isbn) = b.identifiers.get("isbn").and_then(|i| isbn13(i)) {
            by_isbn.entry(isbn).or_insert(b.id);
        }
        for f in &b.formats {
            by_size.entry((f.size, f.format.as_str())).or_default().push((b.id, b.format_path(library_root, f)));
        }
        by_title.entry(normalize_title(&b.title)).or_default().push(i);
    }

    scan.files
        .iter()
        .map(|f| {
            if let Some(id) = f.uuids.iter().find_map(|u| by_uuid.get(u)) {
                return Some((*id, MatchedBy::Uuid));
            }
            if let Some(id) = f.isbn.as_ref().and_then(|i| by_isbn.get(i)) {
                return Some((*id, MatchedBy::Isbn));
            }
            if let Some(same_size) = by_size.get(&(f.size, f.format.as_str()))
                && let Some(mine) = f.sample(&scan.device.root)
                && let Some((id, _)) = same_size.iter().find(|(_, p)| sample(p) == Some(mine))
            {
                return Some((*id, MatchedBy::File));
            }
            let title = normalize_title(f.title.as_deref()?);
            let candidates = by_title.get(&title)?;
            // Without an author, only an unambiguous title will do.
            let found = if f.authors.is_empty() {
                (candidates.len() == 1).then(|| candidates[0])
            } else {
                candidates.iter().copied().find(|&i| author_similarity(&f.authors, &catalog.books[i].authors) >= 1.0)
            };
            found.map(|i| (catalog.books[i].id, MatchedBy::Title))
        })
        .collect()
}

impl DeviceFile {
    fn sample(&self, root: &Path) -> Option<u64> {
        *self.sample.get_or_init(|| sample(&root.join(&self.path)))
    }
}

/// A cheap content fingerprint: the size plus 16 KB from the start, middle
/// and end. Only compared between files already known to be the same size.
fn sample(path: &Path) -> Option<u64> {
    const CHUNK: u64 = 16 << 10;
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let mut h = DefaultHasher::new();
    h.write_u64(len);
    let mut buf = vec![0; CHUNK as usize];
    let offsets: HashSet<u64> = [0, len / 2, len.saturating_sub(CHUNK)].into_iter().collect();
    let mut offsets: Vec<u64> = offsets.into_iter().collect();
    offsets.sort_unstable();
    for at in offsets {
        file.seek(SeekFrom::Start(at)).ok()?;
        let n = file.by_ref().take(CHUNK).read(&mut buf).ok()?;
        h.write(&buf[..n]);
    }
    Some(h.finish())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::Library;
    use crate::import::add_files;
    use crate::metadata::epub::tests::{EPUB2_OPF, make_epub};

    fn dirs(root: &Path, names: &[&str]) {
        for n in names {
            fs::create_dir_all(root.join(n)).unwrap();
        }
    }

    #[test]
    fn recognizes_devices() {
        let tmp = tempfile::tempdir().unwrap();
        let kobo = tmp.path().join("KOBOeReader");
        dirs(&kobo, &[".kobo"]);
        let kindle = tmp.path().join("Jamie's Kindle");
        dirs(&kindle, &["system", "documents"]);
        let other = tmp.path().join("READER");
        dirs(&other, &[""]);
        fs::write(other.join("driveinfo.calibre"), "{}").unwrap();
        let usb = tmp.path().join("USB");
        dirs(&usb, &["documents"]);

        assert_eq!(detect(&kobo).map(|d| (d.kind, d.name)), Some((DeviceKind::Kobo, "Kobo".into())));
        assert_eq!(detect(&kindle).map(|d| (d.kind, d.name)), Some((DeviceKind::Kindle, "Jamie's Kindle".into())));
        assert_eq!(detect(&other).map(|d| d.kind), Some(DeviceKind::Generic));
        assert_eq!(detect(&usb), None);
    }

    /// A library with an EPUB (calibre-style uuid inside), a PDF and an AZW3.
    fn library(tmp: &Path) -> Library {
        let src = tmp.join("src");
        fs::create_dir_all(&src).unwrap();
        make_epub(&src.join("The Dispossessed - Ursula K. Le Guin.epub"), EPUB2_OPF, None);
        fs::write(src.join("Dune - Frank Herbert.pdf"), "%PDF dune bytes").unwrap();
        fs::write(src.join("Hyperion - Dan Simmons.azw3"), "azw3 bytes").unwrap();
        let mut lib = Library::create(tmp.join("lib"), "Test").unwrap();
        let report = add_files(&mut lib, &[src], |_| {}).unwrap();
        assert_eq!(report.added.len(), 3, "{report:?}");
        // The EPUB's uuid is the library book's (as when calibre sent it).
        lib.conn
            .execute(
                "UPDATE books SET uuid = '5c4b5b2c-0000-4000-8000-000000000000' WHERE title = 'The Dispossessed'",
                [],
            )
            .unwrap();
        lib
    }

    #[test]
    fn matches_device_books_to_the_library() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = library(tmp.path());
        let cat = lib.load_catalog().unwrap();
        let id = |title: &str| cat.books.iter().find(|b| b.title == title).unwrap().id;

        let kobo = tmp.path().join("KOBOeReader");
        dirs(&kobo, &[".kobo", "Le Guin", "Herbert", ".kobo-images", "Simmons"]);
        // Renamed, so only the uuid inside can match it.
        make_epub(&kobo.join("Le Guin/renamed.kepub.epub"), EPUB2_OPF, None);
        // An identical copy under another name.
        fs::write(kobo.join("Herbert/whatever.pdf"), "%PDF dune bytes").unwrap();
        // Different bytes, but calibre's naming.
        fs::write(kobo.join("Simmons/Hyperion - Dan Simmons.azw3"), "other azw3 bytes").unwrap();
        // Not in the library; and junk to ignore.
        fs::write(kobo.join("Unknown Book - Nobody.pdf"), "x").unwrap();
        fs::write(kobo.join("._Unknown Book - Nobody.pdf"), "x").unwrap();
        fs::write(kobo.join(".kobo-images/cover.pdf"), "x").unwrap();

        let device = detect(&kobo).unwrap();
        let scan = scan(&device);
        let mut found: Vec<(String, Option<(BookId, MatchedBy)>)> =
            scan.files.iter().map(|f| f.path.clone()).zip(match_books(&scan, &cat, lib.root())).collect();
        found.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(
            found,
            [
                ("Herbert/whatever.pdf".into(), Some((id("Dune"), MatchedBy::File))),
                ("Le Guin/renamed.kepub.epub".into(), Some((id("The Dispossessed"), MatchedBy::Uuid))),
                ("Simmons/Hyperion - Dan Simmons.azw3".into(), Some((id("Hyperion"), MatchedBy::Title))),
                ("Unknown Book - Nobody.pdf".into(), None),
            ]
        );
    }

    #[test]
    fn uses_calibres_device_list_and_kindle_documents() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = library(tmp.path());
        let cat = lib.load_catalog().unwrap();
        let dune = cat.books.iter().find(|b| b.title == "Dune").unwrap();

        let kindle = tmp.path().join("Kindle");
        dirs(&kindle, &["system", "documents/Frank Herbert", "documents/dictionaries", "documents/x.sdr"]);
        fs::write(kindle.join("documents/Frank Herbert/B00XYZ_EBOK.pdf"), "changed bytes").unwrap();
        fs::write(kindle.join("documents/dictionaries/Oxford - Dictionary.pdf"), "d").unwrap();
        fs::write(kindle.join("documents/x.sdr/Dune - Frank Herbert.pdf"), "d").unwrap();
        fs::write(kindle.join("system/Dune - Frank Herbert.pdf"), "d").unwrap();
        let list = format!(
            r#"[{{"lpath": "documents/Frank Herbert/B00XYZ_EBOK.pdf", "uuid": "{}"}}]"#,
            dune.uuid.as_ref().unwrap()
        );
        fs::write(kindle.join("metadata.calibre"), list).unwrap();

        let scan = scan(&detect(&kindle).unwrap());
        assert_eq!(scan.files.len(), 1, "{:?}", scan.files);
        assert_eq!(match_books(&scan, &cat, lib.root()), [Some((dune.id, MatchedBy::Uuid))]);
    }
}
