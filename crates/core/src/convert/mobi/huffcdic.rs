// Derived from calibre's src/calibre/ebooks/mobi/huffcdic.py (calibre 9.18).
// Copyright: 2011, Kovid Goyal <kovid@kovidgoyal.net>, after darkninja and
// igorsk. License: GPLv3.
// Ported to Rust for Magnitide, 2026.

//! HUFF/CDIC decompression, which some Amazon-made books use instead of
//! PalmDOC: a Huffman code whose symbols are phrases from a dictionary, and
//! phrases may themselves be compressed.

use super::book::u32_at;

/// Phrases nest; real books need a few levels. Anything deeper is broken.
const MAX_DEPTH: usize = 32;

enum Phrase {
    Packed(Vec<u8>),
    Unpacked(Vec<u8>),
    /// Being unpacked: met again, the dictionary loops.
    Busy,
}

pub(super) struct Huff {
    /// By the code's top byte: code length, terminal, max code.
    dict1: Vec<(u32, bool, u64)>,
    mincode: [u64; 33],
    maxcode: [u64; 33],
    dictionary: Vec<Phrase>,
}

impl Huff {
    /// From the HUFF record followed by the CDIC records.
    pub fn new(tables: &[&[u8]]) -> Result<Self, String> {
        let bad = || "the book's HUFF/CDIC tables are broken".to_owned();
        let (huff, cdics) = tables.split_first().ok_or_else(bad)?;
        if !huff.starts_with(b"HUFF\x00\x00\x00\x18") {
            return Err(bad());
        }
        let (off1, off2) = (u32_at(huff, 8).ok_or_else(bad)? as usize, u32_at(huff, 12).ok_or_else(bad)? as usize);
        let mut dict1 = Vec::with_capacity(256);
        for i in 0..256 {
            let v = u32_at(huff, off1 + 4 * i).ok_or_else(bad)?;
            let (codelen, term, max) = (v & 0x1F, v & 0x80 != 0, u64::from(v >> 8));
            if codelen == 0 || (codelen <= 8 && !term) {
                return Err(bad());
            }
            dict1.push((codelen, term, ((max + 1) << (32 - codelen)) - 1));
        }
        let (mut mincode, mut maxcode) = ([0u64; 33], [0u64; 33]);
        maxcode[0] = (1 << 32) - 1;
        for codelen in 1..=32 {
            let min = u64::from(u32_at(huff, off2 + 8 * (codelen - 1)).ok_or_else(bad)?);
            let max = u64::from(u32_at(huff, off2 + 8 * (codelen - 1) + 4).ok_or_else(bad)?);
            mincode[codelen] = min << (32 - codelen);
            maxcode[codelen] = ((max + 1) << (32 - codelen)).wrapping_sub(1);
        }
        let mut dictionary = Vec::new();
        for cdic in cdics {
            if !cdic.starts_with(b"CDIC\x00\x00\x00\x10") {
                return Err(bad());
            }
            let (phrases, bits) = (u32_at(cdic, 8).ok_or_else(bad)? as usize, u32_at(cdic, 12).ok_or_else(bad)?);
            let n = (1usize << bits.min(31)).min(phrases.saturating_sub(dictionary.len()));
            for i in 0..n {
                let off = usize::from(super::book::u16_at(cdic, 16 + 2 * i).ok_or_else(bad)?);
                let blen = usize::from(super::book::u16_at(cdic, 16 + off).ok_or_else(bad)?);
                let start = 18 + off;
                let slice = cdic.get(start..start + (blen & 0x7FFF)).ok_or_else(bad)?.to_vec();
                dictionary.push(if blen & 0x8000 != 0 { Phrase::Unpacked(slice) } else { Phrase::Packed(slice) });
            }
        }
        Ok(Self { dict1, mincode, maxcode, dictionary })
    }

    /// Decompress one text record.
    pub fn unpack(&mut self, data: &[u8]) -> Result<Vec<u8>, String> {
        let mut out = Vec::with_capacity(data.len() * 3);
        self.unpack_into(data, &mut out, 0)?;
        Ok(out)
    }

    fn unpack_into(&mut self, data: &[u8], out: &mut Vec<u8>, depth: usize) -> Result<(), String> {
        let bad = || "the book's compressed text is broken".to_owned();
        if depth > MAX_DEPTH {
            return Err(bad());
        }
        let mut padded = data.to_vec();
        padded.extend_from_slice(&[0; 8]);
        let word = |pos: usize| u64::from_be_bytes(padded[pos..pos + 8].try_into().expect("8 bytes"));
        let mut bitsleft = data.len() as i64 * 8;
        let mut pos = 0;
        let mut x = word(pos);
        let mut n: i64 = 32;
        loop {
            if n <= 0 {
                pos += 4;
                if pos + 8 > padded.len() {
                    break;
                }
                x = word(pos);
                n += 32;
            }
            let code = (x >> n) & 0xFFFF_FFFF;
            let (mut codelen, term, mut maxcode) = self.dict1[(code >> 24) as usize];
            if !term {
                while code < self.mincode[codelen as usize] {
                    codelen += 1;
                    if codelen > 32 {
                        return Err(bad());
                    }
                }
                maxcode = self.maxcode[codelen as usize];
            }
            n -= i64::from(codelen);
            bitsleft -= i64::from(codelen);
            if bitsleft < 0 {
                break;
            }
            let r = (maxcode.wrapping_sub(code) >> (32 - codelen)) as usize;
            let phrase = std::mem::replace(self.dictionary.get_mut(r).ok_or_else(bad)?, Phrase::Busy);
            let unpacked = match phrase {
                Phrase::Unpacked(p) => p,
                Phrase::Packed(p) => {
                    let mut sub = Vec::new();
                    self.unpack_into(&p, &mut sub, depth + 1)?;
                    sub
                }
                Phrase::Busy => return Err(bad()),
            };
            out.extend_from_slice(&unpacked);
            self.dictionary[r] = Phrase::Unpacked(unpacked);
        }
        Ok(())
    }
}
