//! The `cover://` protocol: `/thumb/<id>` (cached thumbnail), `/full/<id>`, and
//! `/remote/<key>` (a metadata source's cover, downloaded on request). Also
//! `cover_colors`: a cover's main colours, for the UI to paint with.
//!
//! Thumbnails live in the app cache, keyed by book id and the cover file's
//! modification time, so a changed cover gets a fresh thumbnail. A cover's
//! colours are cached beside its thumbnail under the same key, so they follow
//! the same invalidation and are cleared with the thumbnails.

use std::fs;
use std::io::Cursor;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::UNIX_EPOCH;

use anyhow::{Context, Result};
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, ImageReader};
use serde::{Deserialize, Serialize};
use tauri::http::{Response, StatusCode, header};
use tauri::{AppHandle, Manager, State};
use ts_rs::TS;

use crate::commands::CmdResult;
use crate::{AppState, Loaded};

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
    if size == "remote" {
        // A metadata source's cover, downloaded by `cover_preview`. Keys are hex hashes.
        if id.len() != 16 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Ok(None);
        }
        let dir = crate::lookup::remote_dir(app).map_err(anyhow::Error::msg)?;
        return Ok(fs::read(dir.join(id)).ok());
    }
    let Ok(id) = id.parse::<i64>() else { return Ok(None) };

    let Some(lib) = app.state::<AppState>().current() else { return Ok(None) };
    // Only paths derived from the database are ever read; the request just picks a book id.
    let Some(src) = lib.catalog.get(id).and_then(|b| b.cover_path(&lib.root)) else { return Ok(None) };
    if !src.is_file() {
        return Ok(None);
    }

    match size {
        "full" => Ok(Some(fs::read(&src)?)),
        "thumb" => Ok(Some(cached_thumbnail(&lib, id, &src, cover_mtime(&src)?)?)),
        _ => Ok(None),
    }
}

/// The cover file's modification time, part of its cache key.
fn cover_mtime(src: &Path) -> Result<u64> {
    Ok(fs::metadata(src)?.modified()?.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0))
}

/// The cached thumbnail, made (and cached, best effort) if missing.
fn cached_thumbnail(lib: &Loaded, id: i64, src: &Path, mtime: u64) -> Result<Vec<u8>> {
    let cached = lib.thumb_dir.join(format!("{id}-{mtime}.jpg"));
    if let Ok(bytes) = fs::read(&cached) {
        return Ok(bytes);
    }
    let bytes = encode_jpeg(&thumbnail_image(src)?)?;
    // Best effort: a failed cache write still serves the thumbnail.
    if let Err(e) = write_atomic(&cached, &bytes) {
        eprintln!("caching {}: {e:#}", cached.display());
    }
    Ok(bytes)
}

/// The cover decoded and shrunk to thumbnail size (its proportions kept).
fn thumbnail_image(src: &Path) -> Result<DynamicImage> {
    // calibre names covers .jpg but some are really PNG/WebP, so sniff the format.
    let img = ImageReader::open(src)?
        .with_guessed_format()?
        .decode()
        .with_context(|| format!("decoding {}", src.display()))?;
    Ok(if img.width() > THUMB_W || img.height() > THUMB_H {
        img.resize(THUMB_W, THUMB_H, FilterType::Triangle)
    } else {
        img
    })
}

fn encode_jpeg(img: &DynamicImage) -> Result<Vec<u8>> {
    let mut out = Cursor::new(Vec::new());
    JpegEncoder::new_with_quality(&mut out, 84).encode_image(&img.to_rgb8())?;
    Ok(out.into_inner())
}

// ---------- Cover colours ----------

/// A cover's palette: its most common colours, most common first, for the
/// details panel to blend its background toward. One to three entries.
#[derive(Clone, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct CoverColor {
    pub id: i64,
    /// `#rrggbb` each.
    pub colors: Vec<String>,
}

/// What's cached beside the thumbnail (`{id}-{mtime}.color`). `v` is bumped
/// when the algorithm changes, so older files are recomputed.
#[derive(Serialize, Deserialize)]
struct ColorSidecar {
    v: u8,
    colors: Vec<String>,
}
const COLOR_VERSION: u8 = 3;

/// These books' cover palettes. Books without a cover, or whose cover can't
/// be read, are left out. Computed from the cached thumbnail (made if
/// missing, so the thumbnail cache fills too) and cached beside it. Costs a
/// stat and a tiny read per book when cached, a decode when not: ask for
/// the books on screen, in chunks (see ui/src/coverColors.ts).
#[tauri::command]
pub async fn cover_colors(state: State<'_, AppState>, ids: Vec<i64>) -> CmdResult<Vec<CoverColor>> {
    let lib = state.current().ok_or("No library is open")?;
    tauri::async_runtime::spawn_blocking(move || {
        ids.iter()
            .filter_map(|&id| match cover_color(&lib, id) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("cover colour {id}: {e:#}");
                    None
                }
            })
            .collect()
    })
    .await
    .map_err(|e| e.to_string())
}

fn cover_color(lib: &Loaded, id: i64) -> Result<Option<CoverColor>> {
    // Only paths derived from the database are ever read; the request just picks a book id.
    let Some(src) = lib.catalog.get(id).and_then(|b| b.cover_path(&lib.root)) else { return Ok(None) };
    if !src.is_file() {
        return Ok(None);
    }
    let mtime = cover_mtime(&src)?;
    let sidecar = lib.thumb_dir.join(format!("{id}-{mtime}.color"));
    if let Some(c) = fs::read(&sidecar)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<ColorSidecar>(&bytes).ok())
        .filter(|c| c.v == COLOR_VERSION)
    {
        return Ok(Some(CoverColor { id, colors: c.colors }));
    }
    let bytes = cached_thumbnail(lib, id, &src, mtime)?;
    let img = image::load_from_memory_with_format(&bytes, image::ImageFormat::Jpeg)?;
    let colors = palette(&img);
    let side = ColorSidecar { v: COLOR_VERSION, colors: colors.clone() };
    if let Err(e) = serde_json::to_vec(&side).map_err(anyhow::Error::from).and_then(|b| write_atomic(&sidecar, &b)) {
        eprintln!("caching {}: {e:#}", sidecar.display());
    }
    Ok(Some(CoverColor { id, colors }))
}

/// Up to three of the cover's common colours, the most common first, then
/// ones of a clearly different hue (or, failing that, clearly lighter or
/// darker), so a gradient from them has somewhere to go; for a cover that's
/// all white, grey or black, its mean colour alone.
fn palette(img: &DynamicImage) -> Vec<String> {
    let small = img.thumbnail(32, 48).to_rgb8();
    let total = small.pixels().len().max(1);
    // Saturated pixels, quantised to 4 bits per channel: a count and a colour sum per bucket.
    let mut buckets: std::collections::HashMap<u16, (u32, [u64; 3])> = std::collections::HashMap::new();
    let mut mean = [0u64; 3];
    for p in small.pixels() {
        let [r, g, b] = p.0;
        for (m, c) in mean.iter_mut().zip([r, g, b]) {
            *m += u64::from(c);
        }
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        if max < 40 || u32::from(max - min) * 4 < u32::from(max) {
            continue; // near-black, or too grey to carry a hue
        }
        let key = (u16::from(r >> 4) << 8) | (u16::from(g >> 4) << 4) | u16::from(b >> 4);
        let e = buckets.entry(key).or_insert((0, [0; 3]));
        e.0 += 1;
        for (sum, c) in e.1.iter_mut().zip([r, g, b]) {
            *sum += u64::from(c);
        }
    }
    let rgb = |sum: [u64; 3], n: u64| [sum[0] / n, sum[1] / n, sum[2] / n];
    let hex = |c: [u64; 3]| format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]);
    let mut ranked: Vec<(u32, [u64; 3])> = buckets.into_values().map(|(n, sum)| (n, rgb(sum, u64::from(n)))).collect();
    ranked.sort_by_key(|&(n, _)| std::cmp::Reverse(n));
    // The first needs 3% of the cover; the others, which only add variety, 1.5%.
    let Some(&(_, first)) = ranked.first().filter(|(n, _)| *n as usize * 33 >= total) else {
        return vec![hex(rgb(mean, total as u64))];
    };
    let mut out = vec![first];
    let candidates: Vec<[u64; 3]> = ranked.iter().filter(|(n, _)| *n as usize * 66 >= total).map(|&(_, c)| c).collect();
    while out.len() < 3 {
        // Another hue if there is one (35° away from every colour chosen), else another lightness.
        let far = |c: &[u64; 3], by_hue: bool| {
            out.iter().all(|o| {
                if by_hue { hue_distance(*o, *c) >= 35.0 } else { (lightness(*o) - lightness(*c)).abs() >= 0.3 }
            })
        };
        let next =
            candidates.iter().find(|c| far(c, true)).or_else(|| candidates.iter().find(|c| far(c, false))).copied();
        match next {
            Some(c) => out.push(c),
            None => break,
        }
    }
    out.into_iter().map(hex).collect()
}

/// Hue in degrees, 0–360 (0 for grey).
fn hue(c: [u64; 3]) -> f32 {
    let [r, g, b] = c.map(|x| x as f32 / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let d = max - min;
    if d < 1e-6 {
        return 0.0;
    }
    let h = if max == r {
        ((g - b) / d) % 6.0
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    (h * 60.0).rem_euclid(360.0)
}

/// How far apart two hues are, 0–180.
fn hue_distance(a: [u64; 3], b: [u64; 3]) -> f32 {
    let d = (hue(a) - hue(b)).abs();
    d.min(360.0 - d)
}

/// HSL lightness, 0–1.
fn lightness(c: [u64; 3]) -> f32 {
    let [r, g, b] = c.map(|x| x as f32 / 255.0);
    (r.max(g).max(b) + r.min(g).min(b)) / 2.0
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

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};

    fn solid(w: u32, h: u32, rgb: [u8; 3]) -> RgbImage {
        RgbImage::from_pixel(w, h, Rgb(rgb))
    }

    #[test]
    fn solid_colour_is_itself() {
        assert_eq!(palette(&DynamicImage::ImageRgb8(solid(20, 30, [255, 0, 0]))), ["#ff0000"]);
    }

    #[test]
    fn grey_cover_gives_its_mean() {
        assert_eq!(palette(&DynamicImage::ImageRgb8(solid(20, 30, [128, 128, 128]))), ["#808080"]);
    }

    #[test]
    fn picks_different_hues_over_neighbours() {
        // Mostly one blue with a slightly different blue beside it, plus a little orange:
        // the orange is the second colour, not the other blue.
        let mut img = solid(32, 48, [20, 60, 200]);
        for y in 30..44 {
            for x in 0..32 {
                img.put_pixel(x, y, Rgb([30, 80, 210]));
            }
        }
        for y in 44..48 {
            for x in 0..32 {
                img.put_pixel(x, y, Rgb([230, 120, 20]));
            }
        }
        assert_eq!(palette(&DynamicImage::ImageRgb8(img)), ["#143cc8", "#e67814"]);
        // The same hue in two clearly different lightnesses still gives two colours.
        let mut img = solid(32, 48, [20, 60, 200]);
        for y in 30..48 {
            for x in 0..32 {
                img.put_pixel(x, y, Rgb([150, 170, 240]));
            }
        }
        assert_eq!(palette(&DynamicImage::ImageRgb8(img)), ["#143cc8", "#96aaf0"]);
    }

    #[test]
    fn bands_come_most_common_first() {
        let mut img = solid(32, 48, [255, 255, 255]);
        for y in 0..20 {
            for x in 0..32 {
                img.put_pixel(x, y, Rgb([20, 60, 200]));
            }
        }
        for y in 20..30 {
            for x in 0..32 {
                img.put_pixel(x, y, Rgb([200, 40, 30]));
            }
        }
        assert_eq!(palette(&DynamicImage::ImageRgb8(img)), ["#143cc8", "#c8281e"]);
    }
}
