//! CBR and CB7 comics to CBZ, losslessly: the same pages and files, repacked
//! into a ZIP without recompressing a single image. A CBR that's already a ZIP
//! (many are) is copied as it is; a RAR is unpacked with a tool on the computer
//! (see [`crate::archive`]); a 7z is read natively.

use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::Path;

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use super::TempDir;
use crate::archive::{NEEDS_RAR_TOOL, RarTool, is_rar};
use crate::metadata::comic::natural_cmp;

/// Write `dest` (which must not exist) as a CBZ of the comic archive `src`.
pub fn convert(src: &Path, dest: &Path) -> Result<(), String> {
    if dest.exists() {
        return Err(format!("{} already exists", dest.display()));
    }
    let result = write(src, dest);
    if result.is_err() {
        let _ = std::fs::remove_file(dest);
    }
    result
}

fn write(src: &Path, dest: &Path) -> Result<(), String> {
    let mut magic = [0u8; 6];
    File::open(src).and_then(|mut f| f.read_exact(&mut magic)).map_err(|e| e.to_string())?;
    if magic.starts_with(b"PK\x03\x04") {
        // Already a ZIP under another name.
        return std::fs::copy(src, dest).map(|_| ()).map_err(|e| e.to_string());
    }
    if is_rar(src) {
        let tool = RarTool::find().ok_or(NEEDS_RAR_TOOL)?;
        let work = TempDir::new()?;
        tool.extract_all(src, &work.0)?;
        let mut files = Vec::new();
        collect(&work.0, &work.0, &mut files);
        files.sort_by(|a, b| natural_cmp(&a.0, &b.0));
        let mut zip = Zip::create(dest)?;
        for (name, path) in &files {
            let data = std::fs::read(path).map_err(|e| e.to_string())?;
            zip.add(name, &data)?;
        }
        return zip.finish();
    }
    if magic == *b"7z\xbc\xaf\x27\x1c" {
        let mut reader =
            sevenz_rust2::ArchiveReader::open(src, sevenz_rust2::Password::empty()).map_err(|e| e.to_string())?;
        let mut zip = Zip::create(dest)?;
        let mut failed = None;
        reader
            .for_each_entries(|entry, data| {
                if entry.is_directory() {
                    return Ok(true);
                }
                let mut bytes = Vec::with_capacity(entry.size() as usize);
                data.read_to_end(&mut bytes)?;
                if let Err(e) = zip.add(entry.name(), &bytes) {
                    failed = Some(e);
                    return Ok(false);
                }
                Ok(true)
            })
            .map_err(|e| e.to_string())?;
        if let Some(e) = failed {
            return Err(e);
        }
        return zip.finish();
    }
    Err("it isn't a comic archive Magnitide can read (ZIP, RAR or 7z)".into())
}

/// Every plain file under `dir`, named by its path inside `root` with "/"
/// between folders. Links are skipped: an archive could point one anywhere.
fn collect(root: &Path, dir: &Path, out: &mut Vec<(String, std::path::PathBuf)>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        match std::fs::symlink_metadata(&path) {
            Ok(m) if m.is_dir() => collect(root, &path, out),
            Ok(m) if m.is_file() => {
                if let Ok(rel) = path.strip_prefix(root) {
                    let name = rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/");
                    out.push((name, path));
                }
            }
            _ => {}
        }
    }
}

struct Zip(ZipWriter<BufWriter<File>>);

impl Zip {
    fn create(dest: &Path) -> Result<Self, String> {
        Ok(Self(ZipWriter::new(BufWriter::new(File::create(dest).map_err(|e| e.to_string())?))))
    }

    /// Images go in as they are (they're compressed already); the rest deflated.
    fn add(&mut self, name: &str, data: &[u8]) -> Result<(), String> {
        let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
        let method = match ext.as_str() {
            "jpg" | "jpeg" | "png" | "webp" | "gif" | "avif" | "jxl" => CompressionMethod::Stored,
            _ => CompressionMethod::Deflated,
        };
        let options =
            SimpleFileOptions::default().compression_method(method).large_file(data.len() > u32::MAX as usize);
        self.0.start_file(name, options).map_err(|e| e.to_string())?;
        self.0.write_all(data).map_err(|e| e.to_string())
    }

    fn finish(self) -> Result<(), String> {
        self.0.finish().map_err(|e| e.to_string())?.flush().map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::comic::tests::{COMIC_INFO, make_cbz};

    fn entries(cbz: &Path) -> Vec<(String, Vec<u8>)> {
        let mut zip = zip::ZipArchive::new(File::open(cbz).unwrap()).unwrap();
        (0..zip.len())
            .map(|i| {
                let mut f = zip.by_index(i).unwrap();
                let mut data = Vec::new();
                f.read_to_end(&mut data).unwrap();
                (f.name().to_owned(), data)
            })
            .collect()
    }

    #[test]
    fn a_cbr_that_is_a_zip_is_copied() {
        let tmp = tempfile::tempdir().unwrap();
        let (src, dest) = (tmp.path().join("a.cbr"), tmp.path().join("a.cbz"));
        make_cbz(&src, &[("1.jpg", b"one")], None);
        convert(&src, &dest).unwrap();
        assert_eq!(std::fs::read(&src).unwrap(), std::fs::read(&dest).unwrap());
        assert!(convert(&src, &dest).is_err(), "never overwrites");
    }

    #[test]
    fn a_rar_cbr_is_repacked_page_for_page() {
        if RarTool::find().is_none() {
            eprintln!("no RAR tool on this computer; skipped");
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let (src, dest) = (tmp.path().join("saga.cbr"), tmp.path().join("saga.cbz"));
        crate::archive::tests::make_rar(
            &src,
            &[("Saga/p10.jpg", b"ten"), ("Saga/p2.jpg", b"two"), ("ComicInfo.xml", COMIC_INFO.as_bytes())],
        );
        convert(&src, &dest).unwrap();
        let got = entries(&dest);
        let names: Vec<&str> = got.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["ComicInfo.xml", "Saga/p2.jpg", "Saga/p10.jpg"], "pages in natural order");
        assert_eq!(got[1].1, b"two");
        // And it reads as the same comic.
        let m = crate::metadata::comic::read(&dest).unwrap();
        assert_eq!(m.title.as_deref(), Some("Saga #1"));
    }

    #[test]
    fn something_else_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let (src, dest) = (tmp.path().join("x.cbr"), tmp.path().join("x.cbz"));
        std::fs::write(&src, b"not an archive").unwrap();
        assert!(convert(&src, &dest).is_err());
        assert!(!dest.exists(), "nothing left behind");
    }
}
