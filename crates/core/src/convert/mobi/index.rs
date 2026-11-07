// Derived from calibre's src/calibre/ebooks/mobi/reader/index.py and the
// decint/decode_string helpers in mobi/utils.py (calibre 9.18).
// Copyright: 2012, Kovid Goyal <kovid@kovidgoyal.net>. License: GPLv3.
// Ported to Rust for Magnitide, 2026.

//! Reading INDX indexes: the KF8 skeleton, fragment, NCX and guide tables
//! (the reverse of the writer's `azw3::index`). Each entry has a name and
//! tagged values; strings live in separate CNCX records.

use std::collections::{BTreeMap, HashMap};

use super::book::{Records, decode, u16_at, u32_at};

pub(super) type TagMap = BTreeMap<u8, Vec<u64>>;

pub(super) struct Index {
    pub entries: Vec<(String, TagMap)>,
    /// Strings by offset.
    pub cncx: HashMap<u64, String>,
}

impl Index {
    pub fn string(&self, offset: Option<u64>) -> Option<&str> {
        self.cncx.get(&offset?).map(String::as_str)
    }
}

/// Fields of an INDX header, by position after the "INDX" magic.
struct IndxHeader {
    start: usize,
    count: usize,
    code: u32,
    ncncx: usize,
    oentries: usize,
    ordt2: usize,
    tagx: usize,
}

fn indx_header(data: &[u8]) -> Result<IndxHeader, String> {
    if !data.starts_with(b"INDX") {
        return Err("an index record is missing".into());
    }
    let field = |i: usize| u32_at(data, 4 + 4 * i).ok_or_else(|| "an index record is cut short".to_owned());
    Ok(IndxHeader {
        start: field(4)? as usize,
        count: field(5)? as usize,
        code: field(6)?,
        ncncx: field(12)? as usize,
        oentries: field(41)? as usize,
        ordt2: field(43)? as usize,
        tagx: field(44)? as usize,
    })
}

/// Read the index whose header is record `idx`.
pub(super) fn read(records: &Records<'_>, idx: usize, utf8: bool) -> Result<Index, String> {
    let data = records.get(idx);
    let header = indx_header(data)?;
    let mut cncx = HashMap::new();
    for (n, i) in (idx + header.count + 1..idx + header.count + 1 + header.ncncx).enumerate() {
        read_cncx(records.get(i), n as u64 * 0x10000, utf8, &mut cncx);
    }

    // ORDT: some Amazon samples encode entry names through a table.
    let mut ordt: Vec<char> = Vec::new();
    if header.ordt2 > 0 && data.get(header.ordt2..header.ordt2 + 4) == Some(b"ORDT") {
        let raw = data.get(header.ordt2 + 4..header.ordt2 + 4 + 2 * header.oentries).unwrap_or(&[]);
        ordt = if header.code == 65002 {
            raw.chunks(2)
                .map(|p| p.get(1).filter(|b| (0x21..0x7F).contains(*b)).map_or('?', |b| char::from(*b)))
                .collect()
        } else {
            vec!['?'; header.oentries]
        };
    }

    let tagx_at = if data.get(header.tagx..header.tagx + 4) == Some(b"TAGX") {
        header.tagx
    } else {
        find(data, b"TAGX", 184).ok_or("an index has no tag table")?
    };
    let tagx = &data[tagx_at..];
    let first_entry = u32_at(tagx, 4).ok_or("an index's tag table is cut short")? as usize;
    let control_bytes = u32_at(tagx, 8).ok_or("an index's tag table is cut short")? as usize;
    let tags: Vec<[u8; 4]> = (12..first_entry).step_by(4).filter_map(|i| tagx.get(i..i + 4)?.try_into().ok()).collect();

    let mut entries = Vec::new();
    for i in idx + 1..=idx + header.count {
        let data = records.get(i);
        let h = indx_header(data)?;
        let mut positions: Vec<usize> = (0..h.count)
            .map(|j| u16_at(data, h.start + 4 + 2 * j).map(usize::from))
            .collect::<Option<_>>()
            .ok_or("an index record is cut short")?;
        positions.push(h.start);
        for w in positions.windows(2) {
            let Some(rec) = data.get(w[0]..w[1]) else { continue };
            let Some((name, used)) = entry_name(rec, utf8, &ordt) else { continue };
            entries.push((name, tag_map(control_bytes, &tags, &rec[used..])));
        }
    }
    Ok(Index { entries, cncx })
}

fn find(data: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    data.get(from..)?.windows(needle.len()).position(|w| w == needle).map(|p| p + from)
}

/// An entry's name: a length byte then the bytes.
fn entry_name(rec: &[u8], utf8: bool, ordt: &[char]) -> Option<(String, usize)> {
    let len = usize::from(*rec.first()?);
    let raw = rec.get(1..1 + len)?;
    let name = if !ordt.is_empty() {
        raw.iter().map(|b| ordt.get(usize::from(*b)).copied().unwrap_or('?')).collect()
    } else if raw.contains(&0) && raw.len() % 2 == 0 {
        // Some guide entries are UTF-16.
        let units: Vec<u16> = raw.chunks(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
        String::from_utf16_lossy(&units)
    } else {
        decode(raw, utf8)
    };
    Some((name, 1 + len))
}

/// A forward variable-width integer: 7 bits a byte, the last byte flagged.
pub(super) fn decint(raw: &[u8]) -> (u64, usize) {
    let mut val = 0u64;
    for (i, &b) in raw.iter().enumerate() {
        val = (val << 7) | u64::from(b & 0x7F);
        if b & 0x80 != 0 {
            return (val, i + 1);
        }
    }
    (val, raw.len())
}

fn read_cncx(raw: &[u8], record_offset: u64, utf8: bool, out: &mut HashMap<u64, String>) {
    let mut pos = 0;
    while pos < raw.len() {
        let (len, used) = decint(&raw[pos..]);
        let len = len as usize;
        if len > 0
            && let Some(s) = raw.get(pos + used..pos + used + len)
        {
            out.insert(pos as u64 + record_offset, decode(s, utf8));
        }
        pos += used.max(1) + len;
    }
}

/// An entry's tagged values, as the TAGX table and the control bytes say.
fn tag_map(control_byte_count: usize, tagx: &[[u8; 4]], data: &[u8]) -> TagMap {
    let mut control: &[u8] = data.get(..control_byte_count).unwrap_or(data);
    let mut data = data.get(control_byte_count..).unwrap_or(&[]);
    // (tag, value count, value bytes, values per entry)
    let mut wanted: Vec<(u8, Option<u64>, Option<u64>, u8)> = Vec::new();
    for &[tag, per_entry, mask, eof] in tagx {
        if eof == 1 {
            control = control.get(1..).unwrap_or(&[]);
            continue;
        }
        let Some(&byte) = control.first() else { break };
        let value = byte & mask;
        if value == 0 {
            continue;
        }
        if value == mask {
            if mask.count_ones() > 1 {
                // A number of bytes follows, holding the values.
                let (bytes, used) = decint(data);
                data = &data[used..];
                wanted.push((tag, None, Some(bytes), per_entry));
            } else {
                wanted.push((tag, Some(1), None, per_entry));
            }
        } else {
            wanted.push((tag, Some(u64::from(value >> mask.trailing_zeros())), None, per_entry));
        }
    }
    let mut out = TagMap::new();
    for (tag, count, bytes, per_entry) in wanted {
        let mut values = Vec::new();
        match (count, bytes) {
            (Some(count), _) => {
                for _ in 0..count * u64::from(per_entry) {
                    if data.is_empty() {
                        break;
                    }
                    let (v, used) = decint(data);
                    data = &data[used..];
                    values.push(v);
                }
            }
            (None, Some(bytes)) => {
                let mut consumed = 0u64;
                while consumed < bytes && !data.is_empty() {
                    let (v, used) = decint(data);
                    data = &data[used..];
                    consumed += used as u64;
                    values.push(v);
                }
            }
            (None, None) => {}
        }
        out.insert(tag, values);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_variable_width_integers() {
        assert_eq!(decint(&[0x80]), (0, 1));
        assert_eq!(decint(&[0xFF, 0x12]), (127, 1));
        assert_eq!(decint(&[0x01, 0x80]), (128, 2));
        assert_eq!(decint(&[0x02, 0xAC]), (300, 2));
        assert_eq!(decint(&[0x02]), (2, 1), "unterminated: what's there");
    }

    #[test]
    fn reads_tag_values() {
        // Tag 1: one value (mask 1); tag 6: two values per entry (mask 2).
        let tagx = [[1, 1, 1, 0], [6, 2, 2, 0], [0, 0, 0, 1]];
        let map = tag_map(1, &tagx, &[0b11, 0x85, 0x81, 0x82]);
        assert_eq!(map[&1], [5]);
        assert_eq!(map[&6], [1, 2]);
    }
}
