//! Storing a new book together with its files. Every way of adding books
//! (calibre import, drag and drop) goes through [`store_book`], so the rules
//! for writing book folders live in one place:
//!
//! * the book's row, file rows and anything else the caller records commit
//!   in one transaction;
//! * files are cloned (copy-on-write) where the filesystem supports it;
//! * if anything fails, the transaction rolls back and the folder this call
//!   created is removed. A folder that was already there is never deleted.

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use rusqlite::{Transaction, params};

use crate::error::{Error, Result};
use crate::library::{BookId, Library, NewBook, now};
use crate::paths;

pub(crate) enum Cover<'a> {
    /// An image file to clone or copy as-is (calibre covers are JPEG).
    File(&'a Path),
    /// Image bytes in any common format; converted to JPEG if needed.
    Bytes(&'a [u8]),
}

#[derive(Debug, Default, Clone)]
pub(crate) struct Stored {
    pub id: BookId,
    pub files: usize,
    pub covers: usize,
    pub cloned: usize,
    pub copied: usize,
    pub copied_bytes: u64,
    pub warnings: Vec<String>,
}

/// Add `new` with `files` (upper-case format, source path) and an optional
/// cover. `finish` runs inside the same transaction, for caller-specific
/// rows (custom values, provenance).
pub(crate) fn store_book(
    lib: &mut Library,
    new: &NewBook,
    files: &[(String, PathBuf)],
    cover: Option<Cover<'_>>,
    finish: impl FnOnce(&Transaction<'_>, BookId) -> Result<()>,
) -> Result<Stored> {
    let root = lib.root.clone();
    let tx = lib.conn.transaction()?;
    let (id, rel_dir) = Library::insert_book(&tx, new)?;
    let dir = rel_dir.split('/').fold(root, |p, c| p.join(c));

    let mut created_dir = false;
    let result = (|| -> Result<Stored> {
        if dir.exists() {
            return Err(Error::Import(format!("{} already exists", dir.display())));
        }
        fs::create_dir_all(&dir)?;
        created_dir = true;

        let mut out = Stored { id, ..Default::default() };
        let title = new.title.trim();
        let first_author = new.authors.iter().map(|a| a.trim()).find(|a| !a.is_empty()).unwrap_or("Unknown");
        for (format, src) in files {
            let filename = paths::book_file(title, first_author, format);
            let size = clone_or_copy(src, &dir.join(&filename), &mut out)?;
            tx.prepare_cached("INSERT INTO files (book_id, format, filename, size, added_at) VALUES (?, ?, ?, ?, ?)")?
                .execute(params![id, format, filename, size as i64, now()])?;
            out.files += 1;
        }

        let stored_cover = match cover {
            Some(Cover::File(path)) => {
                clone_or_copy(path, &dir.join("cover.jpg"), &mut out)?;
                true
            }
            Some(Cover::Bytes(bytes)) => match as_jpeg(bytes) {
                Ok(jpeg) => {
                    fs::write(dir.join("cover.jpg"), jpeg)?;
                    true
                }
                Err(e) => {
                    out.warnings.push(format!("“{title}”: cover image couldn't be read ({e}); added without one"));
                    false
                }
            },
            None => false,
        };
        if stored_cover {
            tx.prepare_cached("UPDATE books SET cover_file = 'cover.jpg' WHERE id = ?")?.execute([id])?;
            out.covers += 1;
        }

        finish(&tx, id)?;
        Ok(out)
    })();

    match result.and_then(|out| tx.commit().map(|_| out).map_err(Error::from)) {
        Ok(out) => Ok(out),
        Err(e) => {
            // The transaction rolls back when dropped; tidy the disk too.
            if created_dir {
                let _ = fs::remove_dir_all(&dir);
                if let Some(author_dir) = dir.parent() {
                    let _ = fs::remove_dir(author_dir); // only succeeds if now empty
                }
            }
            Err(Error::Import(format!("“{}”: {e}", new.title.trim())))
        }
    }
}

/// Clone if the filesystem can, else copy. Never overwrites. Returns the size.
fn clone_or_copy(src: &Path, dest: &Path, out: &mut Stored) -> Result<u64> {
    match reflink_copy::reflink_or_copy(src, dest)? {
        None => out.cloned += 1,
        Some(written) => {
            out.copied += 1;
            out.copied_bytes += written;
        }
    }
    Ok(fs::metadata(dest)?.len())
}

/// The library layout names covers `cover.jpg`; EPUB covers are often PNG.
fn as_jpeg(bytes: &[u8]) -> std::result::Result<Vec<u8>, image::ImageError> {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Ok(bytes.to_vec());
    }
    let img = image::load_from_memory(bytes)?;
    let mut out = Cursor::new(Vec::new());
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90).encode_image(&img.to_rgb8())?;
    Ok(out.into_inner())
}
