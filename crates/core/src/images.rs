//! Cover images. The library layout names every cover `cover.jpg`.

use std::io::Cursor;

/// `bytes` as JPEG: unchanged if already JPEG, else decoded and re-encoded.
pub(crate) fn as_jpeg(bytes: &[u8]) -> Result<Vec<u8>, image::ImageError> {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Ok(bytes.to_vec());
    }
    let img = image::load_from_memory(bytes)?;
    let mut out = Cursor::new(Vec::new());
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90).encode_image(&img.to_rgb8())?;
    Ok(out.into_inner())
}

/// A cover made small: scaled down (never up) to fit in `max`×`max`, any
/// transparency laid on white, as JPEG at `quality`.
pub(crate) fn thumbnail_jpeg(bytes: &[u8], max: u32, quality: u8) -> Result<Vec<u8>, image::ImageError> {
    let mut img = image::load_from_memory(bytes)?;
    if img.width() > max || img.height() > max {
        img = img.resize(max, max, image::imageops::FilterType::Lanczos3);
    }
    let mut rgba = img.to_rgba8();
    for p in rgba.pixels_mut() {
        let a = u16::from(p[3]);
        for c in &mut p.0[..3] {
            *c = ((u16::from(*c) * a + 255 * (255 - a)) / 255) as u8;
        }
    }
    let rgb = image::DynamicImage::ImageRgba8(rgba).to_rgb8();
    let mut out = Cursor::new(Vec::new());
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality).encode_image(&rgb)?;
    Ok(out.into_inner())
}
