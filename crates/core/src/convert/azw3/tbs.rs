//! Trailing byte sequences (TBS): appended to each text record, they say
//! which table of contents entries start, end, span or sit inside that
//! record, so a Kindle can jump chapter to chapter. Ported from calibre's
//! writer8/tbs.py, which matches kindlegen's output.

use super::index::encint;

/// A table of contents entry, as the NCX index holds it.
#[derive(Debug, Clone)]
pub(super) struct TocEntry {
    pub index: usize,
    pub start: u64,
    pub length: u64,
    pub depth: u64,
    pub parent: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Action {
    Starts,
    Ends,
    Spans,
    Completes,
}

#[derive(Debug, Clone)]
struct Local {
    index: usize,
    depth: u64,
    parent: Option<usize>,
    action: Action,
}

/// A strand's entries at one depth.
type Layer = (u64, Vec<Local>);
/// A sequence: its value and extra (flag, value) data.
type Sequence = (u64, Vec<(u8, u64)>);

/// Flags in a sequence's extra data.
const SPANS: u8 = 0b1;
const TYPE: u8 = 0b10;
const COUNT: u8 = 0b100;
const STRAND: u8 = 0b1000;

/// The trailing bytes for each text record (in order), given its
/// uncompressed length.
pub(super) fn all(entries: &[TocEntry], record_lengths: &[usize]) -> Vec<Vec<u8>> {
    let data = collect(entries, record_lengths);
    let tbs = |kind: u64| data.iter().map(|strands| encode(strands, kind)).collect::<Option<Vec<_>>>();
    let sequences = tbs(8).or_else(|| tbs(5)).unwrap_or_else(|| vec![Vec::new(); record_lengths.len()]);
    sequences.into_iter().map(|s| trailing_entry(&sequences_to_bytes(&s))).collect()
}

/// For each record, its entries arranged into strands: a strand is a run
/// from a top entry down its first children, then same-level siblings.
fn collect(entries: &[TocEntry], record_lengths: &[usize]) -> Vec<Vec<Vec<Layer>>> {
    let mut sorted: Vec<&TocEntry> = entries.iter().collect();
    sorted.sort_by_key(|e| e.start);
    let mut out = Vec::new();
    let mut record_start = 0u64;
    for &len in record_lengths {
        let next = record_start + len as u64;
        let mut local = Vec::new();
        for e in &sorted {
            if e.start >= next {
                break;
            }
            if e.start + e.length <= record_start {
                continue;
            }
            let start_offset = e.start as i64 - record_start as i64;
            let end = start_offset + e.length as i64;
            let action = match (start_offset < 0, end > len as i64) {
                (true, true) => Action::Spans,
                (true, false) => Action::Ends,
                (false, true) => Action::Starts,
                (false, false) => Action::Completes,
            };
            local.push(Local { index: e.index, depth: e.depth, parent: e.parent, action });
        }
        out.push(separate_strands(local));
        record_start = next;
    }
    out
}

fn separate_strands(mut entries: Vec<Local>) -> Vec<Vec<Layer>> {
    let mut strands = Vec::new();
    while !entries.is_empty() {
        let top = entries.remove(0);
        let strand = populate(top, &mut entries);
        let mut layers: Vec<Layer> = Vec::new();
        for e in strand {
            match layers.iter_mut().find(|(d, _)| *d == e.depth) {
                Some((_, v)) => v.push(e),
                None => layers.push((e.depth, vec![e])),
            }
        }
        strands.push(layers);
    }
    strands
}

fn populate(parent: Local, entries: &mut Vec<Local>) -> Vec<Local> {
    let mut out = vec![parent.clone()];
    if let Some(pos) = entries.iter().position(|c| c.parent == Some(parent.index)) {
        let child = entries.remove(pos);
        out.extend(populate(child, entries));
        return out;
    }
    let mut current = parent.index;
    let mut siblings = Vec::new();
    let mut i = 0;
    while i < entries.len() {
        let e = &entries[i];
        if e.depth == parent.depth && e.parent == parent.parent && e.index == current + 1 {
            current += 1;
            let e = entries.remove(i);
            if entries.iter().any(|c| c.parent == Some(e.index)) {
                siblings.extend(populate(e, entries));
                break;
            }
            siblings.push(e);
            continue;
        }
        i += 1;
    }
    out.extend(siblings);
    out
}

/// A record's strands as (value, extra flags) sequences. None if a later
/// strand would need a negative index under this type (the caller retries
/// with type 5).
fn encode(strands: &[Vec<Layer>], kind: u64) -> Option<Vec<Sequence>> {
    let first = strands.first().and_then(|s| s.first()).and_then(|(_, v)| v.first()).map(|e| e.index);
    let mut out: Vec<Sequence> = Vec::new();
    let mut last_index = 0usize;
    for strand in strands {
        let mut seqs: Vec<Sequence> = Vec::new();
        for (_, entries) in strand {
            let mut extra: Vec<(u8, u64)> = Vec::new();
            let (head, tail) = (&entries[0], entries.last().expect("non-empty layer"));
            if tail.action == Action::Spans {
                extra.push((SPANS, 0));
            }
            if Some(head.index) == first && out.is_empty() && seqs.is_empty() {
                extra.push((TYPE, kind));
            }
            if entries.len() > 1 {
                extra.push((COUNT, entries.len() as u64));
            }
            let mut index = head.index as i64 - head.parent.unwrap_or(0) as i64;
            if !out.is_empty() && seqs.is_empty() {
                index = last_index as i64 - head.index as i64;
                if index < 0 {
                    if kind == 5 {
                        index = -index;
                    } else {
                        return None;
                    }
                } else {
                    extra.push((STRAND, 1));
                }
            }
            last_index = tail.index;
            seqs.push((index.max(0) as u64, extra));
        }
        // Of consecutive spanning layers only the last keeps the flag.
        for i in 0..seqs.len().saturating_sub(1) {
            let next_spans = seqs[i + 1].1.iter().any(|(f, _)| *f == SPANS);
            if next_spans {
                seqs[i].1.retain(|(f, _)| *f != SPANS);
            }
        }
        out.extend(seqs);
    }
    Some(out)
}

fn sequences_to_bytes(seqs: &[Sequence]) -> Vec<u8> {
    let mut out = Vec::new();
    for (i, (value, extra)) in seqs.iter().enumerate() {
        // Only the first sequence uses 3 flag bits; later ones may need 0b1000.
        let flag_size = if i == 0 { 3 } else { 4 };
        let flags = extra.iter().fold(0u8, |f, (k, _)| f | k);
        let mut v = value << flag_size;
        for bit in 0..flag_size {
            v |= u64::from(flags & (1 << bit));
        }
        out.extend(encint(v));
        let get = |k: u8| extra.iter().find(|(f, _)| *f == k).map(|(_, v)| *v);
        if let Some(t) = get(TYPE) {
            out.extend(encint(t));
        }
        if let Some(c) = get(COUNT) {
            out.push(c as u8);
        }
        if let Some(s) = get(SPANS) {
            out.extend(encint(s));
        }
    }
    out
}

/// `data` followed by its total length as a backward-encoded integer.
pub(super) fn trailing_entry(data: &[u8]) -> Vec<u8> {
    let mut size_len = 1;
    loop {
        let encoded = backward_encint((data.len() + size_len) as u64);
        if encoded.len() == size_len {
            let mut out = data.to_vec();
            out.extend(encoded);
            return out;
        }
        size_len += 1;
    }
}

fn backward_encint(mut value: u64) -> Vec<u8> {
    let mut bytes = Vec::new();
    loop {
        bytes.push((value & 0x7F) as u8);
        value >>= 7;
        if value == 0 {
            break;
        }
    }
    let last = bytes.len() - 1;
    bytes[last] |= 0x80;
    bytes.reverse();
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trailing_entries_end_with_their_size() {
        assert_eq!(trailing_entry(&[]), [0x81]);
        assert_eq!(trailing_entry(&[1, 2, 3]), [1, 2, 3, 0x84]);
    }

    #[test]
    fn flat_toc_across_records() {
        // Three chapters over two 4096-byte records; chapter 1 crosses the boundary.
        let entries = vec![
            TocEntry { index: 0, start: 0, length: 3000, depth: 0, parent: None },
            TocEntry { index: 1, start: 3000, length: 3000, depth: 0, parent: None },
            TocEntry { index: 2, start: 6000, length: 2192, depth: 0, parent: None },
        ];
        let tbs = all(&entries, &[4096, 4096]);
        assert_eq!(tbs.len(), 2);
        // Record 1: entries 0 and 1 (a run of two), first entry flagged with type 8.
        assert_eq!(tbs[0], trailing_entry(&[0x86, 0x88, 2]));
        // Record 2: entry 1 ends here, entry 2 completes; again one run of two.
        assert_eq!(tbs[1], trailing_entry(&[0x8E, 0x88, 2]));
    }
}
