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

/// Lets every webview show `cover:` and `page:` (the comic viewer's, see
/// reader.rs) URLs from a page served over `http:`, which is what the dev
/// build is (Vite on `http://localhost:1420`).
///
/// WebKitGTK 2.54 refuses that with "Unsafe attempt to load URL … Domains,
/// protocols and ports must match": a custom scheme no longer passes its
/// cross-origin display check from an `http:` page, though it still does from
/// the packaged build's `tauri://localhost`. The view's CORS allowlist is the
/// narrowest switch that clears it. Harmless in packaged builds.
pub fn allow_on_http<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
    tauri::plugin::Builder::new("cover-allowlist")
        .on_webview_ready(|webview| {
            #[cfg(target_os = "linux")]
            if let Err(e) = webview.with_webview(|platform| {
                use webkit2gtk::WebViewExt;
                // One list for the view: every scheme it needs goes in this call.
                platform.inner().set_cors_allowlist(&["cover://*/*", "page://*/*"]);
            }) {
                eprintln!("allowing cover: and page: in {}: {e}", webview.label());
            }
            #[cfg(not(target_os = "linux"))]
            let _ = webview;
        })
        .build()
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
const COLOR_VERSION: u8 = 5;

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

/// Up to three of the cover's colours: the most common first, then the one
/// that stands out most from what's chosen so far, as long as it stands out
/// enough. "Standing out" is OKLab distance with hue and saturation counting
/// for more than lightness, and saturated colours get a lift, so a red cover
/// gives red, then its gold, then its white, not red and two greys.
fn palette(img: &DynamicImage) -> Vec<String> {
    pick(&clusters(img)).into_iter().map(|lab| hex(srgb(lab))).collect()
}

/// The cover's colours with their share of it, most common first: shades of
/// one colour (a textured foil, a gradient, leaves in light and shade) count
/// as one, at the shade most seen.
fn clusters(img: &DynamicImage) -> Vec<(f32, [f32; 3])> {
    let small = img.thumbnail(48, 72).to_rgb8();
    let total = small.pixels().len().max(1) as f32;
    // Every pixel, quantised to 3 bits per channel: a count and a colour sum per bucket.
    let mut buckets: std::collections::HashMap<u16, (u32, [u64; 3])> = std::collections::HashMap::new();
    for p in small.pixels() {
        let [r, g, b] = p.0;
        let key = (u16::from(r >> 5) << 6) | (u16::from(g >> 5) << 3) | u16::from(b >> 5);
        let e = buckets.entry(key).or_insert((0, [0; 3]));
        e.0 += 1;
        for (sum, c) in e.1.iter_mut().zip([r, g, b]) {
            *sum += u64::from(c);
        }
    }
    let mut ranked: Vec<(f32, [f32; 3])> =
        buckets.into_values().map(|(n, sum)| (n as f32 / total, oklab(sum.map(|x| x as f32 / n as f32)))).collect();
    ranked.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut clusters: Vec<(f32, [f32; 3])> = Vec::new();
    for (share, lab) in ranked {
        match clusters.iter_mut().find(|(_, c)| same_colour(*c, lab)) {
            Some((s, _)) => *s += share,
            None => clusters.push((share, lab)),
        }
    }
    clusters.retain(|(share, _)| *share >= 0.005);
    clusters.sort_by(|a, b| b.0.total_cmp(&a.0));
    clusters
}

fn pick(clusters: &[(f32, [f32; 3])]) -> Vec<[f32; 3]> {
    let Some(&(_, first)) = clusters.first() else { return vec![] };
    let mut out = vec![first];
    while out.len() < 3 {
        let best = clusters
            .iter()
            .map(|&(share, lab)| {
                let apart = out.iter().map(|o| apartness(*o, lab)).fold(f32::INFINITY, f32::min);
                (apart, apart * share.powf(0.25) * (1.0 + 4.0 * chroma(lab)), lab)
            })
            .filter(|&(apart, ..)| apart >= 0.2)
            .max_by(|a, b| a.1.total_cmp(&b.1));
        match best {
            Some((_, _, lab)) => out.push(lab),
            None => break,
        }
    }
    out
}

fn hex(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

/// sRGB (0–255) to OKLab, where distances match how different colours look.
fn oklab(c: [f32; 3]) -> [f32; 3] {
    let lin = |x: f32| {
        let x = x / 255.0;
        if x <= 0.04045 { x / 12.92 } else { ((x + 0.055) / 1.055).powf(2.4) }
    };
    let [r, g, b] = c.map(lin);
    let l = (0.412_221_5 * r + 0.536_332_5 * g + 0.051_445_7 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_9 * b).cbrt();
    let s = (0.088_302_4 * r + 0.281_718_8 * g + 0.629_978_7 * b).cbrt();
    [
        0.210_454_3 * l + 0.793_617_8 * m - 0.004_072_7 * s,
        1.977_998_4 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_0 * l + 0.782_771_8 * m - 0.808_675_8 * s,
    ]
}

fn srgb(lab: [f32; 3]) -> [u8; 3] {
    let [l_, a, b] = lab;
    let l = (l_ + 0.396_337_8 * a + 0.215_803_8 * b).powi(3);
    let m = (l_ - 0.105_561_3 * a - 0.063_854_2 * b).powi(3);
    let s = (l_ - 0.089_484_1 * a - 1.291_485_5 * b).powi(3);
    let rgb = [
        4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_9 * s,
        -1.268_438 * l + 2.609_757_4 * m - 0.341_319_4 * s,
        -0.004_196_1 * l - 0.703_418_6 * m + 1.707_614_7 * s,
    ];
    rgb.map(|x| {
        let x = x.clamp(0.0, 1.0);
        let x = if x <= 0.003_130_8 { x * 12.92 } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 };
        (x * 255.0).round() as u8
    })
}

/// How far apart two colours look, with hue and saturation counting double against lightness.
fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + 4.0 * ((a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2))).sqrt()
}

/// How saturated a colour is (0 for grey, around 0.3 for a vivid one).
fn chroma(lab: [f32; 3]) -> f32 {
    (lab[1].powi(2) + lab[2].powi(2)).sqrt()
}

/// Whether two colours are shades of one: the same hue and saturation, within a stretch of lightness.
fn same_colour(a: [f32; 3], b: [f32; 3]) -> bool {
    ((a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt() < 0.07 && (a[0] - b[0]).abs() < 0.35
}

/// `distance`, but two coloured (not grey) colours of one hue count as half as far
/// apart: a darker gold isn't a new colour beside gold.
fn apartness(a: [f32; 3], b: [f32; 3]) -> f32 {
    let d = distance(a, b);
    if chroma(a) > 0.06 && chroma(b) > 0.06 {
        let hue = |c: [f32; 3]| c[2].atan2(c[1]).to_degrees();
        let diff = (hue(a) - hue(b)).abs();
        if diff.min(360.0 - diff) < 30.0 {
            return d * 0.5;
        }
    }
    d
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

    fn band(img: &mut RgbImage, rows: std::ops::Range<u32>, rgb: [u8; 3]) {
        for y in rows {
            for x in 0..img.width() {
                img.put_pixel(x, y, Rgb(rgb));
            }
        }
    }

    /// Hex differs by a unit or two after the round trip through OKLab, so compare loosely.
    fn close(got: &str, want: &str) -> bool {
        let n = |s: &str, i: usize| i64::from_str_radix(&s[i..i + 2], 16).unwrap();
        (1..6).step_by(2).all(|i| (n(got, i) - n(want, i)).abs() <= 3)
    }
    fn assert_palette(img: RgbImage, want: &[&str]) {
        let got = palette(&DynamicImage::ImageRgb8(img));
        assert!(
            got.len() == want.len() && got.iter().zip(want).all(|(g, w)| close(g, w)),
            "got {got:?}, want {want:?}"
        );
    }

    #[test]
    fn solid_colour_is_itself() {
        assert_palette(solid(20, 30, [255, 0, 0]), &["#ff0000"]);
        assert_palette(solid(20, 30, [128, 128, 128]), &["#808080"]);
    }

    #[test]
    fn most_common_first_then_what_stands_out() {
        // A red cover with a gold emblem and a white title: red, gold, white.
        let mut img = solid(48, 72, [166, 38, 36]);
        band(&mut img, 30..50, [249, 224, 79]);
        band(&mut img, 50..60, [248, 246, 254]);
        band(&mut img, 60..66, [150, 30, 30]); // a darker red: gathered into the red
        assert_palette(img, &["#a62624", "#f9e04f", "#f8f6fe"]);
        // Black, then gold foil in two shades, then white text, with a grey badge: the gold
        // beats the greys even though the lighter grey is farther from black in plain lightness.
        let mut img = solid(48, 72, [3, 3, 2]);
        band(&mut img, 0..16, [200, 170, 40]);
        band(&mut img, 16..26, [150, 125, 30]);
        band(&mut img, 26..34, [245, 245, 245]);
        band(&mut img, 34..40, [175, 175, 174]);
        let got = palette(&DynamicImage::ImageRgb8(img));
        assert!(close(&got[0], "#030302") && close(&got[1], "#c8aa28") && close(&got[2], "#f5f5f5"), "{got:?}");
    }

    #[test]
    fn a_near_twin_does_not_count() {
        let mut img = solid(48, 72, [20, 60, 200]);
        band(&mut img, 40..72, [30, 80, 210]);
        assert_eq!(palette(&DynamicImage::ImageRgb8(img)).len(), 1);
    }

    /// Prints the palette of a cover: MAGNITIDE_COVER=/path/to/cover.jpg cargo test -p magnitide print_palette -- --ignored --nocapture
    #[test]
    #[ignore]
    fn print_palette() {
        for path in std::env::var("MAGNITIDE_COVER").unwrap_or_default().split(':').filter(|p| !p.is_empty()) {
            let img = thumbnail_image(Path::new(path)).unwrap();
            println!("{path}: {:?}", palette(&img));
            if std::env::var("MAGNITIDE_COVER_DEBUG").is_ok() {
                for (share, lab) in clusters(&img).iter().take(12) {
                    println!("  {:5.1}%  {}  chroma {:.2}", share * 100.0, hex(srgb(*lab)), chroma(*lab));
                }
            }
        }
    }
}
