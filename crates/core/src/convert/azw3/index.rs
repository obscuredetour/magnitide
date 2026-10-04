//! KF8 index records (INDX): the skeleton, fragment ("chunk"), NCX (table of
//! contents) and guide indexes. Each index is a header record describing its
//! tags (TAGX), entry records holding the entries, and CNCX records for the
//! strings entries refer to. Layout follows calibre's writer8/index.py.

use std::collections::HashMap;

/// A tag in an index's TAGX table.
#[derive(Clone, Copy)]
pub(super) struct Tag {
    pub number: u8,
    pub values_per_entry: u8,
    pub mask: u8,
    /// Marks the end of a control byte's tags.
    pub end: bool,
}

const fn tag(number: u8, values_per_entry: u8, mask: u8) -> Tag {
    Tag { number, values_per_entry, mask, end: false }
}
const END: Tag = Tag { number: 0, values_per_entry: 0, mask: 0, end: true };

pub(super) const SKEL_TAGS: &[Tag] = &[tag(1, 1, 3), tag(6, 2, 12), END];
pub(super) const CHUNK_TAGS: &[Tag] = &[tag(2, 1, 1), tag(3, 1, 2), tag(4, 1, 4), tag(6, 2, 8), END];
pub(super) const GUIDE_TAGS: &[Tag] = &[tag(1, 1, 1), tag(6, 2, 2), END];
pub(super) const NCX_TAGS: &[Tag] = &[
    tag(1, 1, 1),
    tag(2, 1, 2),
    tag(3, 1, 4),
    tag(4, 1, 8),
    tag(21, 1, 16),
    tag(22, 1, 32),
    tag(23, 1, 64),
    tag(6, 2, 128),
    END,
];

/// An index entry: its name and, per tag number, its values.
pub(super) struct Entry {
    pub name: Vec<u8>,
    pub values: Vec<(u8, Vec<u64>)>,
}

/// A variable-width integer, forward-encoded: 7 bits per byte, the high bit
/// set on the last byte.
pub(super) fn encint(mut value: u64) -> Vec<u8> {
    let mut bytes = Vec::new();
    loop {
        bytes.push((value & 0x7F) as u8);
        value >>= 7;
        if value == 0 {
            break;
        }
    }
    bytes[0] |= 0x80;
    bytes.reverse();
    bytes
}

pub(super) fn align4(mut v: Vec<u8>) -> Vec<u8> {
    while !v.len().is_multiple_of(4) {
        v.push(0);
    }
    v
}

/// Strings for an index, in CNCX records: each string is its length as an
/// encint followed by UTF-8; entries refer to them by offset.
pub(super) struct Cncx {
    pub records: Vec<Vec<u8>>,
    offsets: HashMap<String, u64>,
}

impl Cncx {
    pub fn new<'a>(strings: impl IntoIterator<Item = &'a str>) -> Self {
        const LIMIT: usize = 0x10000 - 1024;
        let mut records = Vec::new();
        let mut offsets = HashMap::new();
        let mut buf: Vec<u8> = Vec::new();
        let mut offset = 0u64;
        for s in strings {
            if offsets.contains_key(s) {
                continue;
            }
            let text: String = s.chars().take(500).collect();
            let text = if text.trim().is_empty() { "Unknown".to_owned() } else { text.trim().to_owned() };
            let mut raw = encint(text.len() as u64);
            raw.extend_from_slice(text.as_bytes());
            if buf.len() + raw.len() > LIMIT {
                records.push(align4(std::mem::take(&mut buf)));
                offset = records.len() as u64 * 0x10000;
            }
            buf.extend_from_slice(&raw);
            offsets.insert(s.to_owned(), offset);
            offset += raw.len() as u64;
        }
        if !buf.is_empty() {
            records.push(align4(buf));
        }
        Self { records, offsets }
    }

    pub fn offset(&self, s: &str) -> u64 {
        self.offsets[s]
    }

    pub fn empty() -> Self {
        Self { records: Vec::new(), offsets: HashMap::new() }
    }
}

const HEADER_LENGTH: usize = 192;

/// An entry record being filled: entries, their IDXT offsets, count, last name.
type Block = (Vec<u8>, Vec<u8>, u16, Vec<u8>);

/// Build an index's records: header, entry records, then CNCX records.
pub(super) fn build(tags: &[Tag], entries: &[Entry], cncx: &Cncx) -> Vec<Vec<u8>> {
    let control_bytes = tags.iter().filter(|t| t.end).count();
    const RECORD_LIMIT: usize = 0x10000 - HEADER_LENGTH - 1048;

    // Entry records, rolling over when one fills up.
    let mut blocks: Vec<Block> = vec![(Vec::new(), Vec::new(), 0, Vec::new())];
    for e in entries {
        let mut raw = vec![e.name.len() as u8];
        raw.extend_from_slice(&e.name);
        // Control bytes: per tag, how many value groups the entry has.
        let mut cb = 0u8;
        for t in tags {
            if t.end {
                raw.push(cb);
                cb = 0;
                continue;
            }
            let n = e.values.iter().find(|(num, _)| *num == t.number).map_or(0, |(_, v)| v.len());
            let groups = (n / usize::from(t.values_per_entry)) as u8;
            cb |= t.mask & (groups << t.mask.trailing_zeros());
        }
        debug_assert_eq!(raw.len(), e.name.len() + 1 + control_bytes);
        for t in tags.iter().filter(|t| !t.end) {
            if let Some((_, values)) = e.values.iter().find(|(num, _)| *num == t.number) {
                for v in values {
                    raw.extend(encint(*v));
                }
            }
        }
        let block = blocks.last_mut().expect("never empty");
        if block.0.len() + block.1.len() + raw.len() + 2 > RECORD_LIMIT {
            blocks.push((Vec::new(), Vec::new(), 0, Vec::new()));
        }
        let block = blocks.last_mut().expect("never empty");
        block.1.extend_from_slice(&((HEADER_LENGTH + block.0.len()) as u16).to_be_bytes());
        block.0.extend_from_slice(&raw);
        block.2 += 1;
        block.3 = e.name.clone();
    }

    let mut records = Vec::new();
    for (index_block, idxt_block, count, _) in &blocks {
        let index_block = align4(index_block.clone());
        let mut idxt = b"IDXT".to_vec();
        idxt.extend_from_slice(idxt_block);
        let idxt = align4(idxt);
        let mut rec = b"INDX".to_vec();
        rec.extend_from_slice(&(HEADER_LENGTH as u32).to_be_bytes());
        rec.extend_from_slice(&[0; 4]);
        rec.extend_from_slice(&1u32.to_be_bytes()); // an entry record
        rec.extend_from_slice(&[0; 4]);
        rec.extend_from_slice(&((HEADER_LENGTH + index_block.len()) as u32).to_be_bytes());
        rec.extend_from_slice(&u32::from(*count).to_be_bytes());
        rec.extend_from_slice(&[0xFF; 8]);
        rec.extend_from_slice(&[0; 156]);
        debug_assert_eq!(rec.len(), HEADER_LENGTH);
        rec.extend_from_slice(&index_block);
        rec.extend_from_slice(&idxt);
        records.push(rec);
    }

    // The header record: TAGX, then each entry record's last name and count.
    let mut tagx = b"TAGX".to_vec();
    let tag_bytes: Vec<u8> =
        tags.iter().flat_map(|t| [t.number, t.values_per_entry, t.mask, u8::from(t.end)]).collect();
    tagx.extend_from_slice(&(12 + tag_bytes.len() as u32).to_be_bytes());
    tagx.extend_from_slice(&(control_bytes as u32).to_be_bytes());
    tagx.extend_from_slice(&tag_bytes);
    let tagx = align4(tagx);
    let mut geometry = Vec::new();
    let mut idxt = b"IDXT".to_vec();
    let mut pos = HEADER_LENGTH + tagx.len();
    for (_, _, count, last) in &blocks {
        let start = geometry.len();
        idxt.extend_from_slice(&(pos as u16).to_be_bytes());
        geometry.push(last.len() as u8);
        geometry.extend_from_slice(last);
        geometry.extend_from_slice(&count.to_be_bytes());
        pos += geometry.len() - start;
    }
    let geometry = align4(geometry);
    let idxt = align4(idxt);

    let mut header = b"INDX".to_vec();
    header.extend_from_slice(&(HEADER_LENGTH as u32).to_be_bytes());
    header.extend_from_slice(&[0; 8]);
    header.extend_from_slice(&2u32.to_be_bytes()); // type
    let idxt_offset_at = header.len();
    header.extend_from_slice(&[0; 4]); // IDXT offset, filled in below
    header.extend_from_slice(&(records.len() as u32).to_be_bytes());
    header.extend_from_slice(&65001u32.to_be_bytes());
    header.extend_from_slice(&0xFFFF_FFFFu32.to_be_bytes());
    header.extend_from_slice(&(entries.len() as u32).to_be_bytes());
    header.extend_from_slice(&[0; 12]); // ORDT, LIGT, their count
    header.extend_from_slice(&(cncx.records.len() as u32).to_be_bytes());
    header.extend_from_slice(&[0; 124]);
    header.extend_from_slice(&(HEADER_LENGTH as u32).to_be_bytes()); // TAGX offset
    header.extend_from_slice(&[0; 8]);
    debug_assert_eq!(header.len(), HEADER_LENGTH);
    header.extend_from_slice(&tagx);
    header.extend_from_slice(&geometry);
    let idxt_offset = header.len() as u32;
    header[idxt_offset_at..idxt_offset_at + 4].copy_from_slice(&idxt_offset.to_be_bytes());
    header.extend_from_slice(&idxt);

    let mut out = vec![align4(header)];
    out.extend(records);
    out.extend(cncx.records.iter().cloned());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_variable_width_integers() {
        assert_eq!(encint(0), [0x80]);
        assert_eq!(encint(0x7F), [0xFF]);
        assert_eq!(encint(0x11111), [0x04, 0x22, 0x91]);
    }

    #[test]
    fn builds_a_skeleton_index() {
        let entries =
            vec![Entry { name: b"SKEL0000000000".to_vec(), values: vec![(1, vec![2, 2]), (6, vec![0, 500, 0, 500])] }];
        let recs = build(SKEL_TAGS, &entries, &Cncx::empty());
        assert_eq!(recs.len(), 2, "header + one entry record");
        assert_eq!(&recs[0][..4], b"INDX");
        assert_eq!(&recs[0][192..196], b"TAGX");
        assert_eq!(u32::from_be_bytes(recs[0][36..40].try_into().unwrap()), 1, "one entry");
        // Entry: name length, name, control byte (2 chunk counts → 2 groups in mask 3; 2 geometry groups → mask 12).
        let e = &recs[1][192..];
        assert_eq!(e[0], 14);
        assert_eq!(&e[1..15], b"SKEL0000000000");
        assert_eq!(e[15], 0b1010);
        assert_eq!(&e[16..18], [0x82, 0x82]);
    }

    #[test]
    fn cncx_dedupes_and_offsets() {
        let c = Cncx::new(["a", "bb", "a"]);
        assert_eq!((c.offset("a"), c.offset("bb")), (0, 2));
        assert_eq!(c.records[0][..5], [0x81, b'a', 0x82, b'b', b'b']);
    }
}
