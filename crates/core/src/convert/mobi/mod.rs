// Derived from calibre's src/calibre/ebooks/mobi/reader/ (mobi6.py, mobi8.py)
// (calibre 9.18): how joint MOBI/KF8 files are told apart and read.
// Copyright: 2012, Kovid Goyal <kovid@kovidgoyal.net>. License: GPLv3.
// Ported to Rust for Magnitide, 2026.

//! MOBI, AZW and AZW3 → EPUB, natively: upgrading old Kindle files.
//!
//! Follows calibre's MOBI reader (reader/, GPL-3.0 like Magnitide):
//! * the PalmDB records and MOBI headers ([`book`]), text decompressed from
//!   PalmDOC or HUFF/CDIC ([`huffcdic`]);
//! * KF8 (AZW3, and the KF8 half of joint MOBI files, which is preferred):
//!   files rebuilt from the skeleton and fragment indexes ([`index`],
//!   [`kf8`]), stylesheets, SVG, images and fonts recovered, the NCX and
//!   guide read back;
//! * MOBI 6: one HTML stream made valid, split at its page breaks
//!   ([`mobi6`]);
//! * both: markup tidied into EPUB 3 XHTML ([`html`]) and written with an
//!   OPF, nav and NCX ([`epub`]).
//!
//! DRM-protected books are refused: they can't be read, and Magnitide
//! doesn't remove DRM. Topaz, KFX and Print Replica books aren't MOBI and
//! are refused too.

use std::path::Path;

use crate::metadata::epub::Res;

mod book;
mod epub;
mod html;
mod huffcdic;
mod index;
mod kf8;
mod mobi6;

use book::{Header, NULL, Records};

/// Bigger files aren't read (the biggest real books are around 100 MB).
const MAX_FILE: u64 = 512 << 20;

/// A book no converter can read: DRM-protected, or not really MOBI (Topaz,
/// KFX). calibre isn't worth trying after this.
#[derive(Debug)]
pub struct Unconvertible(pub String);

impl std::fmt::Display for Unconvertible {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Unconvertible {}

/// What converting noticed but didn't stop for.
#[derive(Debug, Default)]
pub struct MobiReport {
    pub warnings: Vec<String>,
    /// "KF8" or "MOBI 6": which part of the file the EPUB was made from.
    pub source: &'static str,
}

/// Convert the MOBI/AZW/AZW3 at `src` to EPUB at `dest` (which must not
/// exist). The EPUB's metadata is the file's own; the library's is written
/// over it afterwards.
pub fn convert(src: &Path, dest: &Path) -> Res<MobiReport> {
    if std::fs::metadata(src)?.len() > MAX_FILE {
        return Err("the file is too big to convert".into());
    }
    let data = std::fs::read(src)?;
    if data.starts_with(b"TPZ") {
        return Err(Unconvertible("it's a Topaz book (an old Amazon format), which can't be converted".into()).into());
    }
    if data.starts_with(b"\xeaDRMION\xee") {
        return Err(Unconvertible("it's a DRM-protected KFX book, which can't be converted".into()).into());
    }
    let records = Records::parse(&data)?;
    let r0 = Header::parse(records.get(0), records.textread)?;
    let mut report = MobiReport::default();
    let meta = crate::metadata::mobi::parse(&data).unwrap_or_default();

    // A joint file: MOBI 6 first, then a BOUNDARY record and the KF8 book.
    let joint = r0
        .exth
        .kf8_boundary
        .map(|k| k as usize)
        .filter(|&k| k > 0 && k < records.len() && records.get(k - 1) == b"BOUNDARY")
        .and_then(|k| Some((k, Header::parse(records.get(k), false).ok()?)));
    for h in std::iter::once(&r0).chain(joint.as_ref().map(|(_, h)| h)) {
        if h.encryption != 0 {
            let msg = "it's protected by DRM, so it can't be converted (Magnitide doesn't remove DRM)";
            return Err(Unconvertible(msg.into()).into());
        }
    }

    let book = if r0.is_kf8() {
        let first = first_resource(r0.first_image, r0.text_records, 1);
        report.source = "KF8";
        kf8::read(&records, 0, &r0, &[(first, records.len())], &mut report.warnings)?
    } else if let Some((k8i, k8)) = joint.as_ref().filter(|(_, h)| h.is_kf8()) {
        let ranges = [
            (first_resource(r0.first_image, r0.text_records, 1), k8i - 1),
            (
                first_resource(k8.first_image.checked_add(*k8i as u32).unwrap_or(NULL), k8.text_records, k8i + 1),
                records.len(),
            ),
        ];
        match kf8::read(&records, *k8i, k8, &ranges, &mut report.warnings) {
            Ok(b) => {
                report.source = "KF8";
                b
            }
            Err(e) => {
                // The MOBI 6 half has the same book, plainer.
                report.warnings.push(format!("its KF8 part couldn't be read ({e}); used the MOBI 6 part"));
                report.source = "MOBI 6";
                mobi6::read(&records, &r0, &mut report.warnings)?
            }
        }
    } else {
        report.source = "MOBI 6";
        mobi6::read(&records, &r0, &mut report.warnings)?
    };

    let mut meta = meta;
    if meta.title.as_deref().is_none_or(|t| t.trim().is_empty()) {
        meta.title = r0.exth.title.clone().or_else(|| Some(r0.title.clone()).filter(|t| !t.is_empty()));
    }
    epub::write(book, &meta, dest)?;
    Ok(report)
}

/// Where a book's images start: its header says, or right after its text.
fn first_resource(first_image: u32, text_records: usize, first_text: usize) -> usize {
    if first_image == NULL { text_records + first_text } else { first_image as usize }
}

#[cfg(test)]
mod tests;
