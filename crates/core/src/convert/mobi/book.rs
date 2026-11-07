// Derived from calibre's src/calibre/ebooks/mobi/reader/headers.py and the
// text extraction in reader/mobi6.py (calibre 9.18).
// Copyright: 2012, Kovid Goyal <kovid@kovidgoyal.net>. License: GPLv3.
// Ported to Rust for Magnitide, 2026.

//! The PalmDB container, the MOBI headers (MOBI 6 and KF8) and the book's
//! decompressed text.

use super::huffcdic::Huff;
use crate::convert::azw3::palmdoc;

pub(super) const NULL: u32 = 0xFFFF_FFFF;

/// The records of a PalmDB file.
pub(super) struct Records<'a> {
    data: &'a [u8],
    offsets: Vec<usize>,
    /// "BOOKMOBI" or "TEXTREAD".
    pub textread: bool,
}

impl<'a> Records<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self, String> {
        let ident = data.get(60..68).ok_or("not a MOBI book")?.to_ascii_uppercase();
        if ident != b"BOOKMOBI" && ident != b"TEXTREAD" {
            return Err("not a MOBI book".into());
        }
        let count = u16_at(data, 76).ok_or("not a MOBI book")? as usize;
        let offsets: Vec<usize> = (0..count)
            .map(|i| u32_at(data, 78 + 8 * i).map(|o| o as usize))
            .collect::<Option<_>>()
            .ok_or("the book's record list is cut short")?;
        if offsets.len() < 2 {
            return Err("the book has no text".into());
        }
        Ok(Self { data, offsets, textread: ident == b"TEXTREAD" })
    }

    pub fn len(&self) -> usize {
        self.offsets.len()
    }

    /// Record `i`, empty if it's missing or its offsets are broken.
    pub fn get(&self, i: usize) -> &'a [u8] {
        let Some(&start) = self.offsets.get(i) else { return &[] };
        let end = self.offsets.get(i + 1).copied().unwrap_or(self.data.len()).min(self.data.len());
        self.data.get(start..end.max(start)).unwrap_or(&[])
    }
}

pub(super) fn u16_at(d: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(d.get(at..at + 2)?.try_into().ok()?))
}

pub(super) fn u32_at(d: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(d.get(at..at + 4)?.try_into().ok()?))
}

/// What the EXTH block says that converting needs (metadata itself is read
/// by [`crate::metadata::mobi`]).
#[derive(Debug, Default, Clone)]
pub(super) struct Exth {
    pub start_offset: Option<u32>,
    pub kf8_boundary: Option<u32>,
    pub cover_offset: Option<u32>,
    pub page_progression: Option<String>,
    pub title: Option<String>,
}

/// A MOBI header (record 0 of the book, or of its KF8 half).
#[derive(Debug, Clone)]
pub(super) struct Header {
    /// 1 none, 2 PalmDOC, 0x4448 ("DH") HUFF/CDIC.
    pub compression: u16,
    pub text_records: usize,
    pub encryption: u16,
    pub utf8: bool,
    pub version: u32,
    /// Relative to this header's record.
    pub first_image: u32,
    pub extra_flags: u16,
    /// Relative to this header's record.
    pub huff: Option<(u32, u32)>,
    pub fdst: u32,
    pub ncx: u32,
    pub frag: u32,
    pub skel: u32,
    pub guide: u32,
    pub ancient: bool,
    pub title: String,
    pub exth: Exth,
}

impl Header {
    pub fn parse(raw: &[u8], textread: bool) -> Result<Self, String> {
        let short = || "the book's header is cut short".to_owned();
        let compression = u16_at(raw, 0).ok_or_else(short)?;
        let text_records = u16_at(raw, 8).ok_or_else(short)? as usize;
        let encryption = u16_at(raw, 12).ok_or_else(short)?;
        if raw.len() <= 16 {
            return Ok(Self {
                compression,
                text_records,
                encryption,
                utf8: false,
                version: 1,
                first_image: NULL,
                extra_flags: 0,
                huff: None,
                fdst: NULL,
                ncx: NULL,
                frag: NULL,
                skel: NULL,
                guide: NULL,
                ancient: true,
                title: String::new(),
                exth: Exth::default(),
            });
        }
        let length = u32_at(raw, 20).ok_or_else(short)?;
        let utf8 = u32_at(raw, 28).ok_or_else(short)? == 65001;
        // Some KF8 files have a header length of 264; kindlegen's may grow.
        let extra_flags = if textread || !(0xE4..=500).contains(&length) { 0 } else { u16_at(raw, 0xF2).unwrap_or(0) };
        let huff = (compression == 0x4448).then(|| Some((u32_at(raw, 0x70)?, u32_at(raw, 0x74)?))).flatten();
        let decode = |b: &[u8]| decode(b, utf8).trim_matches('\0').trim().to_owned();
        let title = match (u32_at(raw, 0x54), u32_at(raw, 0x58)) {
            (Some(off), Some(len)) => raw.get(off as usize..(off as usize).saturating_add(len as usize)).map(decode),
            _ => None,
        }
        .unwrap_or_default();
        let version = u32_at(raw, 0x68).unwrap_or(1);
        let first_image = u32_at(raw, 0x6C).unwrap_or(NULL);
        let exth_flag = u32_at(raw, 0x80).unwrap_or(0);
        let exth =
            if exth_flag & 0x40 != 0 { parse_exth(raw.get(16 + length as usize..).unwrap_or(&[]), utf8) } else { None };
        let ncx = if raw.len() >= 0xF8 { u32_at(raw, 0xF4).unwrap_or(NULL) } else { NULL };
        let (mut frag, mut skel, mut guide, mut fdst) = (NULL, NULL, NULL, NULL);
        // Ancient PRC files can have anything as a version: be careful.
        if version == 8 && raw.len() >= 0xF8 + 16 {
            frag = u32_at(raw, 0xF8).unwrap_or(NULL);
            skel = u32_at(raw, 0xFC).unwrap_or(NULL);
            guide = u32_at(raw, 0x104).unwrap_or(NULL);
            fdst = u32_at(raw, 0xC0).unwrap_or(NULL);
            // With one flow or none, the FDST record number can be garbage.
            if u32_at(raw, 0xC4).unwrap_or(0) <= 1 {
                fdst = NULL;
            }
        }
        Ok(Self {
            compression,
            text_records,
            encryption,
            utf8,
            version,
            first_image,
            extra_flags,
            huff,
            fdst,
            ncx,
            frag,
            skel,
            guide,
            ancient: false,
            title,
            exth: exth.unwrap_or_default(),
        })
    }

    /// A KF8 book (or KF8 half), as opposed to MOBI 6.
    pub fn is_kf8(&self) -> bool {
        self.version == 8 && self.skel != NULL
    }

    pub fn decode(&self, b: &[u8]) -> String {
        decode(b, self.utf8)
    }
}

/// Text in the book's encoding: UTF-8 or Windows-1252.
pub(super) fn decode(b: &[u8], utf8: bool) -> String {
    if utf8 { String::from_utf8_lossy(b).into_owned() } else { crate::metadata::mobi::cp1252(b) }
}

fn parse_exth(raw: &[u8], utf8: bool) -> Option<Exth> {
    if !raw.starts_with(b"EXTH") {
        return None;
    }
    let count = u32_at(raw, 8)?;
    let mut out = Exth::default();
    let mut at = 12;
    for _ in 0..count {
        let (kind, len) = (u32_at(raw, at)?, u32_at(raw, at + 4)? as usize);
        let Some(value) = raw.get(at + 8..at + len.max(8)) else { break };
        at += len.max(8);
        let text = || decode(value, utf8).trim_matches('\0').trim().to_owned();
        match kind {
            116 => out.start_offset = u32_at(value, 0).filter(|v| *v != NULL),
            121 => out.kf8_boundary = u32_at(value, 0).filter(|v| *v != NULL),
            201 => out.cover_offset = u32_at(value, 0).filter(|v| *v != NULL),
            503 => out.title = Some(text()).filter(|t| !t.is_empty()),
            527 => out.page_progression = Some(text()).filter(|t| !t.is_empty()),
            _ => {}
        }
    }
    Some(out)
}

/// The decompressed text of the book (or of its KF8 half, whose header is
/// record `base`).
pub(super) fn text(records: &Records<'_>, header: &Header, base: usize) -> Result<Vec<u8>, String> {
    let first = base + 1;
    let last = (base + header.text_records).min(records.len().saturating_sub(1));
    let mut out = Vec::new();
    match header.compression {
        1 => {
            for i in first..=last {
                out.extend_from_slice(text_record(records.get(i), header.extra_flags));
            }
        }
        2 => {
            for i in first..=last {
                out.extend(palmdoc::decompress(text_record(records.get(i), header.extra_flags)));
            }
        }
        0x4448 => {
            let (start, count) = header.huff.ok_or("the book's HUFF records are missing")?;
            let start = base + start as usize;
            let tables: Vec<&[u8]> = (start..start + count as usize).map(|i| records.get(i)).collect();
            let mut huff = Huff::new(&tables)?;
            for i in first..=last {
                out.extend(huff.unpack(text_record(records.get(i), header.extra_flags))?);
            }
        }
        c => return Err(format!("unknown compression ({c})")),
    }
    Ok(out)
}

/// A text record without its trailing entries.
fn text_record(data: &[u8], extra_flags: u16) -> &[u8] {
    let size = data.len();
    let mut num = 0usize;
    let mut flags = extra_flags >> 1;
    while flags != 0 {
        if flags & 1 != 0 {
            num += trailing_entry_size(&data[..size.saturating_sub(num)]);
        }
        flags >>= 1;
    }
    if extra_flags & 1 != 0
        && let Some(&b) = size.checked_sub(num + 1).and_then(|i| data.get(i))
    {
        num += usize::from(b & 3) + 1;
    }
    &data[..size.saturating_sub(num)]
}

/// The size of a trailing entry, written backwards at the end of `data`.
fn trailing_entry_size(data: &[u8]) -> usize {
    let (mut bitpos, mut result) = (0, 0usize);
    for &v in data.iter().rev() {
        result |= usize::from(v & 0x7F) << bitpos;
        bitpos += 7;
        if v & 0x80 != 0 || bitpos >= 28 {
            break;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trailing_entries_come_off() {
        // A multibyte overlap of one byte (flag 1) after a TBS entry of 3 bytes (flag 2).
        let rec = b"hello\xC3\x01\x00\x01\x83";
        assert_eq!(text_record(rec, 0b11), b"hello");
        assert_eq!(text_record(b"hi\x00", 0b1), b"hi");
        assert_eq!(text_record(b"plain", 0), b"plain");
        // Broken trailing sizes don't panic.
        assert_eq!(text_record(b"\xFF", 0b11), b"");
    }
}
