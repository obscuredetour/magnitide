//! A comic's pages, for reading it in the app: the images in natural order
//! ("page2" before "page10", the macOS junk and hidden files left out), each
//! readable by its number. A CBZ is read in place; a RAR (through a tool on
//! the computer) or 7z is unpacked once into a scratch folder, removed when
//! the pages are dropped, so turning pages never waits on decompressing.

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::{OptionalExtension, params};
use zip::ZipArchive;

use crate::archive::{NEEDS_RAR_TOOL, RarTool, is_rar};
use crate::convert::TempDir;
use crate::convert::cbz::collect;
use crate::error::Result;
use crate::library::{BookId, LOCAL_USER, Library, now};
use crate::metadata::comic::{is_page, natural_cmp};

/// A page bigger than this isn't a page anyone means to read.
const MAX_PAGE: u64 = 128 << 20;

pub struct Pages {
    names: Vec<String>,
    source: Source,
}

enum Source {
    Zip(Mutex<ZipArchive<BufReader<File>>>),
    /// Unpacked: each page's file (in `names` order), in a folder removed with it.
    Folder {
        _dir: TempDir,
        paths: Vec<PathBuf>,
    },
}

impl Pages {
    /// Open the comic archive at `path` (by its content: a CBR may be a ZIP).
    pub fn open(path: &Path) -> std::result::Result<Self, String> {
        let mut magic = [0u8; 6];
        File::open(path).and_then(|mut f| f.read_exact(&mut magic)).map_err(|e| e.to_string())?;
        if magic.starts_with(b"PK\x03\x04") {
            let zip = ZipArchive::new(BufReader::new(File::open(path).map_err(|e| e.to_string())?))
                .map_err(|e| e.to_string())?;
            let mut names: Vec<String> = zip.file_names().filter(|n| is_page(n)).map(str::to_owned).collect();
            names.sort_by(|a, b| natural_cmp(a, b));
            return Ok(Self { names, source: Source::Zip(Mutex::new(zip)) });
        }
        let dir = TempDir::new()?;
        if is_rar(path) {
            RarTool::find().ok_or(NEEDS_RAR_TOOL)?.extract_all(path, &dir.0)?;
        } else if magic == *b"7z\xbc\xaf\x27\x1c" {
            // Each page written under a number (a safe name, whatever the archive
            // calls it), its own name kept for the order.
            let mut files: Vec<(String, PathBuf)> = Vec::new();
            sevenz_rust2::ArchiveReader::open(path, sevenz_rust2::Password::empty())
                .and_then(|mut r| {
                    r.for_each_entries(|entry, data| {
                        if entry.is_directory() || !is_page(entry.name()) {
                            return Ok(true);
                        }
                        let file = dir.0.join(format!("{:06}", files.len()));
                        std::io::copy(data, &mut File::create(&file)?)?;
                        files.push((entry.name().to_owned(), file));
                        Ok(true)
                    })
                })
                .map_err(|e| e.to_string())?;
            files.sort_by(|a, b| natural_cmp(&a.0, &b.0));
            let (names, paths) = files.into_iter().unzip();
            return Ok(Self { names, source: Source::Folder { _dir: dir, paths } });
        } else {
            return Err("it isn't a comic archive Magnitide can read (ZIP, RAR or 7z)".into());
        }
        let mut files = Vec::new();
        collect(&dir.0, &dir.0, &mut files);
        files.retain(|(name, _)| is_page(name));
        files.sort_by(|a, b| natural_cmp(&a.0, &b.0));
        let (names, paths) = files.into_iter().unzip();
        Ok(Self { names, source: Source::Folder { _dir: dir, paths } })
    }

    pub fn len(&self) -> usize {
        self.names.len()
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// Page `index`'s image and its media type.
    pub fn page(&self, index: usize) -> Option<(Vec<u8>, &'static str)> {
        let name = self.names.get(index)?;
        let bytes = match &self.source {
            Source::Zip(zip) => {
                let mut zip = zip.lock().unwrap_or_else(|e| e.into_inner());
                let entry = zip.by_name(name).ok()?;
                if entry.size() > MAX_PAGE {
                    return None;
                }
                let mut out = Vec::with_capacity(entry.size() as usize);
                entry.take(MAX_PAGE).read_to_end(&mut out).ok()?;
                out
            }
            Source::Folder { paths, .. } => {
                let path = paths.get(index)?;
                if std::fs::metadata(path).ok()?.len() > MAX_PAGE {
                    return None;
                }
                std::fs::read(path).ok()?
            }
        };
        Some((bytes, media_type(name)))
    }
}

fn media_type(name: &str) -> &'static str {
    match name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).as_deref() {
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        _ => "image/jpeg",
    }
}

/// Where you are in a book: its reading status, and how far through (0–1).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ReadingState {
    /// "unread", "reading" or "read".
    pub status: String,
    pub progress: Option<f64>,
    /// When the position was last moved (RFC 3339).
    pub last_read_at: Option<String>,
    /// The e-reader a synced position came from ("Kobo Libra 2"); None for
    /// this app's own reader.
    pub device: Option<String>,
}

impl Library {
    pub fn reading_state(&self, id: BookId) -> Result<ReadingState> {
        let row: Option<ReadingState> = self
            .conn
            .query_row(
                "SELECT status, progress, last_read_at, device FROM user_books WHERE user_id = ? AND book_id = ?",
                params![LOCAL_USER, id],
                |r| {
                    Ok(ReadingState {
                        status: r.get(0)?,
                        progress: r.get(1)?,
                        last_read_at: r.get(2)?,
                        device: r.get(3)?,
                    })
                },
            )
            .optional()?;
        Ok(row.unwrap_or_else(|| ReadingState {
            status: "unread".into(),
            progress: None,
            last_read_at: None,
            device: None,
        }))
    }

    /// Record how far through the book you are (0–1) in this app's reader:
    /// it's being read, or at the end, read. A book already read stays read
    /// when it's reopened.
    pub fn set_reading_progress(&mut self, id: BookId, progress: f64) -> Result<ReadingState> {
        let progress = progress.clamp(0.0, 1.0);
        let finished = progress >= 1.0;
        self.conn.execute(
            "INSERT INTO user_books (user_id, book_id, status, progress, last_read_at, device)
             VALUES (?1, ?2, ?3, ?4, ?5, NULL)
             ON CONFLICT (user_id, book_id) DO UPDATE SET
               status = CASE WHEN ?3 = 'read' OR status = 'read' THEN 'read' ELSE 'reading' END,
               progress = ?4, last_read_at = ?5, device = NULL",
            params![LOCAL_USER, id, if finished { "read" } else { "reading" }, progress, now()],
        )?;
        self.reading_state(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::comic::tests::make_cbz;

    #[test]
    fn pages_of_a_cbz_in_reading_order() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("a.cbr"); // a ZIP named .cbr
        make_cbz(
            &path,
            &[("p10.png", b"ten"), ("p2.jpg", b"two"), ("ComicInfo.xml", b"<x/>"), ("__MACOSX/._p2.jpg", b"junk")],
            None,
        );
        let pages = Pages::open(&path).unwrap();
        assert_eq!(pages.len(), 2);
        assert_eq!(pages.page(0), Some((b"two".to_vec(), "image/jpeg")));
        assert_eq!(pages.page(1), Some((b"ten".to_vec(), "image/png")));
        assert_eq!(pages.page(2), None);
    }

    #[test]
    fn pages_of_a_rar_unpacked_once() {
        if RarTool::find().is_none() {
            eprintln!("no RAR tool on this computer; skipped");
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("a.cbr");
        crate::archive::tests::make_rar(
            &path,
            &[("Saga/p10.jpg", b"ten"), ("Saga/p2.jpg", b"two"), ("ComicInfo.xml", b"<x/>")],
        );
        let pages = Pages::open(&path).unwrap();
        let Source::Folder { _dir: dir, .. } = &pages.source else { panic!("unpacked") };
        let dir = dir.0.clone();
        assert_eq!((pages.len(), pages.page(0).unwrap().0), (2, b"two".to_vec()));
        drop(pages);
        assert!(!dir.exists(), "the scratch folder goes with it");
    }

    #[test]
    fn reading_progress_and_status() {
        let tmp = tempfile::tempdir().unwrap();
        let mut lib = Library::create(tmp.path().join("lib"), "Test").unwrap();
        let id = lib.add_book(&crate::library::NewBook { title: "Saga #1".into(), ..Default::default() }).unwrap();
        assert_eq!(
            lib.reading_state(id).unwrap(),
            ReadingState { status: "unread".into(), progress: None, last_read_at: None, device: None }
        );
        assert_eq!(lib.set_reading_progress(id, 0.25).unwrap().status, "reading");
        let done = lib.set_reading_progress(id, 1.0).unwrap();
        assert_eq!((done.status.as_str(), done.progress), ("read", Some(1.0)));
        // Reread from the start: still read, at the start.
        let again = lib.set_reading_progress(id, 0.0).unwrap();
        assert_eq!((again.status.as_str(), again.progress), ("read", Some(0.0)));
    }
}
