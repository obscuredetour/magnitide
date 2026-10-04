//! MOBI, AZW and AZW3 (KF8) metadata: Amazon's PalmDB container.
//!
//! Record 0 holds the PalmDOC and MOBI headers, followed by an EXTH block of
//! typed metadata records; images are later records, the cover found by an
//! EXTH offset from the first image record. Only metadata is read here, so
//! DRM-protected books still show their details (their text stays
//! unreadable, as it should).

use std::path::Path;

use super::{Author, BookMetadata, language_code, parse_date};

/// Files bigger than this aren't read (no book needs it; a broken one might).
const MAX_FILE: u64 = 256 << 20;

/// EXTH record types worth reading.
mod exth {
    pub const AUTHOR: u32 = 100;
    pub const PUBLISHER: u32 = 101;
    pub const DESCRIPTION: u32 = 103;
    pub const ISBN: u32 = 104;
    pub const SUBJECT: u32 = 105;
    pub const PUBLISHED: u32 = 106;
    pub const SOURCE: u32 = 112;
    pub const ASIN: u32 = 113;
    pub const COVER_OFFSET: u32 = 201;
    pub const THUMB_OFFSET: u32 = 202;
    pub const TITLE: u32 = 503;
    pub const LANGUAGE: u32 = 524;
}

/// Metadata from a MOBI/AZW/AZW3 file, or None if it isn't one.
pub fn read(path: &Path) -> Option<BookMetadata> {
    if std::fs::metadata(path).ok()?.len() > MAX_FILE {
        return None;
    }
    parse(&std::fs::read(path).ok()?)
}

fn u16_at(d: &[u8], at: usize) -> Option<usize> {
    Some(u16::from_be_bytes(d.get(at..at + 2)?.try_into().ok()?).into())
}

fn u32_at(d: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(d.get(at..at + 4)?.try_into().ok()?))
}

pub(crate) fn parse(d: &[u8]) -> Option<BookMetadata> {
    // PalmDB header: name, then type and creator ("BOOK" "MOBI") at 60.
    if d.get(60..68)? != b"BOOKMOBI" {
        return None;
    }
    let count = u16_at(d, 76)?;
    let offsets: Vec<usize> = (0..count).map(|i| u32_at(d, 78 + 8 * i).map(|o| o as usize)).collect::<Option<_>>()?;
    let record = |i: usize| -> Option<&[u8]> {
        let start = *offsets.get(i)?;
        let end = offsets.get(i + 1).copied().unwrap_or(d.len());
        d.get(start..end.max(start))
    };
    let r0 = record(0)?;
    if r0.get(16..20)? != b"MOBI" {
        return None;
    }
    let header_len = u32_at(r0, 20)? as usize;
    let utf8 = u32_at(r0, 28)? == 65001;
    let text = |b: &[u8]| -> String {
        let s = if utf8 { String::from_utf8_lossy(b).into_owned() } else { cp1252(b) };
        s.trim_matches(char::from(0)).trim().to_owned()
    };

    let mut out = BookMetadata::default();
    let mut cover_offset = None;
    let mut thumb_offset = None;
    // EXTH, when flagged, follows the MOBI header.
    if u32_at(r0, 128).is_some_and(|f| f & 0x40 != 0) {
        let e = 16 + header_len;
        if r0.get(e..e + 4) == Some(b"EXTH") {
            let n = u32_at(r0, e + 8)?;
            let mut at = e + 12;
            for _ in 0..n {
                let (kind, len) = (u32_at(r0, at)?, u32_at(r0, at + 4)? as usize);
                let Some(value) = r0.get(at + 8..at + len.max(8)) else { break };
                at += len.max(8);
                let v = text(value);
                match kind {
                    exth::AUTHOR if !v.is_empty() => {
                        // Some writers put several in one record: "A & B" or "A; B".
                        for name in v.split([';', '&']).map(str::trim).filter(|n| !n.is_empty()) {
                            // Kindle files often store "King, Stephen".
                            out.authors.push(match crate::names::from_surname_first(name) {
                                Some((name, sort)) => Author { name, sort: Some(sort) },
                                None => Author { name: name.to_owned(), sort: None },
                            });
                        }
                    }
                    exth::PUBLISHER if !v.is_empty() => out.publisher = Some(v),
                    exth::DESCRIPTION if !v.is_empty() => out.description = Some(v),
                    exth::ISBN => {
                        let isbn: String = v.chars().filter(|c| c.is_ascii_digit() || *c == 'X').collect();
                        if isbn.len() == 10 || isbn.len() == 13 {
                            out.identifiers.insert("isbn".into(), isbn);
                        }
                    }
                    exth::SUBJECT if !v.is_empty() => out.tags.push(v),
                    exth::PUBLISHED => out.published = parse_date(&v),
                    exth::SOURCE => {
                        if let Some(uuid) = v.strip_prefix("calibre:").filter(|u| is_uuid(u)) {
                            out.uuids.push(uuid.to_ascii_lowercase());
                        }
                    }
                    // calibre puts its book uuid here when there's no ASIN.
                    exth::ASIN if is_uuid(&v) => out.uuids.push(v.to_ascii_lowercase()),
                    exth::ASIN if !v.is_empty() => {
                        out.identifiers.insert("mobi-asin".into(), v);
                    }
                    exth::COVER_OFFSET => cover_offset = u32_at(value, 0),
                    exth::THUMB_OFFSET => thumb_offset = u32_at(value, 0),
                    exth::TITLE if !v.is_empty() => out.title = Some(v),
                    exth::LANGUAGE if !v.is_empty() => out.languages.push(language_code(&v)),
                    _ => {}
                }
            }
        }
    }
    if out.title.is_none() {
        // The full name from the MOBI header, else the PalmDB name.
        let (off, len) = (u32_at(r0, 84)? as usize, u32_at(r0, 88)? as usize);
        let full = r0.get(off..off + len).map(text).filter(|t| !t.is_empty());
        out.title = full.or_else(|| Some(text(d.get(..32)?).replace('_', " ")).filter(|t| !t.is_empty()));
    }
    out.uuids.dedup();

    // The cover: an image record counted from the first image.
    let first_image = u32_at(r0, 108)?;
    if first_image != u32::MAX {
        out.cover = [cover_offset, thumb_offset]
            .into_iter()
            .flatten()
            .filter(|o| *o != u32::MAX)
            .filter_map(|o| record(first_image as usize + o as usize))
            .find(|img| is_image(img))
            .map(<[u8]>::to_vec);
    }
    Some(out)
}

fn is_uuid(s: &str) -> bool {
    s.len() == 36 && s.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-') && s.matches('-').count() == 4
}

fn is_image(b: &[u8]) -> bool {
    b.starts_with(&[0xFF, 0xD8]) || b.starts_with(b"\x89PNG") || b.starts_with(b"GIF8")
}

/// Windows-1252, the older MOBI text encoding.
fn cp1252(b: &[u8]) -> String {
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž', '\u{8f}', '\u{90}', '‘',
        '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}', 'ž', 'Ÿ',
    ];
    b.iter().map(|&c| if (0x80..0xA0).contains(&c) { HIGH[usize::from(c - 0x80)] } else { char::from(c) }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
    }

    #[test]
    fn reads_mobi_and_azw3() {
        for name in ["test.mobi", "test.azw3"] {
            let m = read(&fixture(name)).unwrap_or_else(|| panic!("{name}"));
            assert_eq!(m.title.as_deref(), Some("Test Book: A Story"), "{name}");
            let authors: Vec<&str> = m.authors.iter().map(|a| a.name.as_str()).collect();
            assert_eq!(authors, ["Ursula K. Le Guin", "Second Author"], "{name}");
            assert_eq!(m.publisher.as_deref(), Some("Harper & Row"));
            assert_eq!(m.published, chrono::NaiveDate::from_ymd_opt(1974, 5, 1));
            assert_eq!(m.languages, ["eng"]);
            assert_eq!(m.identifiers.get("isbn").map(String::as_str), Some("9780061054884"));
            assert_eq!(m.tags, ["Science Fiction", "Classics"]);
            assert_eq!(m.description.as_deref(), Some("<p>An <b>ambiguous</b> utopia.</p>"));
            assert_eq!(m.uuids.len(), 1, "{name}: calibre's book id");
            assert!(m.cover.as_deref().is_some_and(|c| c.starts_with(&[0xFF, 0xD8])), "{name}: cover");
            assert!(!m.identifiers.contains_key("mobi-asin"), "a calibre uuid isn't an ASIN");
        }
    }

    #[test]
    fn other_files_are_none() {
        assert!(parse(b"not a mobi").is_none());
        let mut fake = vec![0u8; 100];
        fake[60..68].copy_from_slice(b"TEXtREAd");
        assert!(parse(&fake).is_none());
        // Truncated: a header that points past the end.
        let mut short = std::fs::read(fixture("test.mobi")).unwrap();
        short.truncate(200);
        assert!(parse(&short).is_none() || parse(&short).is_some_and(|m| m.cover.is_none()));
    }

    #[test]
    fn decodes_cp1252() {
        assert_eq!(cp1252(b"caf\xe9 \x93quoted\x94 \x80"), "café “quoted” €");
    }
}
