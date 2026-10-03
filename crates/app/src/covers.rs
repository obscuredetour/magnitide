//! The `cover://` protocol: `/thumb/<id>` (cached thumbnail) and `/full/<id>`.
//!
//! Thumbnails live in the app cache, keyed by book id and the cover file's
//! modification time, so a changed cover gets a fresh thumbnail.

use std::fs;
use std::io::Cursor;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::UNIX_EPOCH;

use anyhow::{Context, Result};
use image::ImageReader;
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use tauri::http::{Response, StatusCode, header};
use tauri::{AppHandle, Manager};

use crate::AppState;

/// Big enough for a ~200 CSS px wide tile on a 2x display.
const THUMB_W: u32 = 400;
const THUMB_H: u32 = 600;

pub fn respond(app: &AppHandle, path: &str) -> Response<Vec<u8>> {
    match serve(app, path) {
        Ok(Some(bytes)) => Response::builder()
            .header(header::CONTENT_TYPE, "image/jpeg")
            .header(header::CACHE_CONTROL, "max-age=3600")
            .body(bytes)
            .unwrap_or_else(|_| status(StatusCode::INTERNAL_SERVER_ERROR)),
        Ok(None) => status(StatusCode::NOT_FOUND),
        Err(e) => {
            eprintln!("cover {path}: {e:#}");
            status(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

fn status(code: StatusCode) -> Response<Vec<u8>> {
    let mut r = Response::new(Vec::new());
    *r.status_mut() = code;
    r
}

fn serve(app: &AppHandle, path: &str) -> Result<Option<Vec<u8>>> {
    let mut parts = path.trim_start_matches('/').splitn(2, '/');
    let (Some(size), Some(id)) = (parts.next(), parts.next()) else { return Ok(None) };
    let Ok(id) = id.parse::<i64>() else { return Ok(None) };

    let Some(lib) = app.state::<AppState>().current() else { return Ok(None) };
    // Only paths derived from the database are ever read; the request just picks a book id.
    let Some(src) = lib.snap.get(id).and_then(|b| b.cover_path(&lib.root)) else { return Ok(None) };
    if !src.is_file() {
        return Ok(None);
    }

    match size {
        "full" => Ok(Some(fs::read(&src)?)),
        "thumb" => {
            let mtime = fs::metadata(&src)?.modified()?.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            let cached = lib.thumb_dir.join(format!("{id}-{mtime}.jpg"));
            if let Ok(bytes) = fs::read(&cached) {
                return Ok(Some(bytes));
            }
            let bytes = make_thumbnail(&src)?;
            // Best effort: a failed cache write still serves the thumbnail.
            if let Err(e) = write_atomic(&cached, &bytes) {
                eprintln!("caching {}: {e:#}", cached.display());
            }
            Ok(Some(bytes))
        }
        _ => Ok(None),
    }
}

fn make_thumbnail(src: &Path) -> Result<Vec<u8>> {
    // calibre names covers .jpg but some are really PNG/WebP, so sniff the format.
    let img = ImageReader::open(src)?
        .with_guessed_format()?
        .decode()
        .with_context(|| format!("decoding {}", src.display()))?;
    let thumb = if img.width() > THUMB_W || img.height() > THUMB_H {
        img.resize(THUMB_W, THUMB_H, FilterType::Triangle)
    } else {
        img
    };
    let mut out = Cursor::new(Vec::new());
    JpegEncoder::new_with_quality(&mut out, 84).encode_image(&thumb.to_rgb8())?;
    Ok(out.into_inner())
}

fn write_atomic(dest: &Path, bytes: &[u8]) -> Result<()> {
    let dir = dest.parent().context("thumbnail path has no parent")?;
    fs::create_dir_all(dir)?;
    // Unique per writer, so concurrent requests for one cover can't collide.
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let tmp = dir.join(format!(".{}-{}.tmp", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed)));
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, dest)?;
    Ok(())
}
