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
