//! PalmDOC compression, used for MOBI/AZW3 text records: an LZ77 variant
//! over records of at most 4096 bytes.
//!
//! Encoded bytes:
//! * `0x00`, `0x09`–`0x7F`: that byte;
//! * `0x01`–`0x08`: the next 1–8 bytes, verbatim;
//! * `0x80`–`0xBF` + one byte: copy 3–10 bytes from 1–2047 bytes back;
//! * `0xC0`–`0xFF`: a space, then the byte `b ^ 0x80`.

/// Compress one record (at most 4096 bytes).
pub fn compress(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    // Earlier positions of each 3-byte sequence, newest last.
    let mut seen: std::collections::HashMap<[u8; 3], Vec<usize>> = std::collections::HashMap::new();
    let remember = |seen: &mut std::collections::HashMap<[u8; 3], Vec<usize>>, from: usize, to: usize| {
        for p in from..to.min(input.len().saturating_sub(2)) {
            seen.entry([input[p], input[p + 1], input[p + 2]]).or_default().push(p);
        }
    };
    let mut i = 0;
    while i < input.len() {
        // A back-reference of 3–10 bytes within the last 2047.
        if i + 3 <= input.len()
            && let Some(starts) = seen.get(&[input[i], input[i + 1], input[i + 2]])
        {
            let max_len = (input.len() - i).min(10);
            let mut best = (0, 0); // (distance, length)
            for &start in starts.iter().rev().take(64) {
                if i - start > 2047 {
                    break;
                }
                let mut len = 3;
                while len < max_len && input[start + len] == input[i + len] {
                    len += 1;
                }
                if len > best.1 {
                    best = (i - start, len);
                    if len == max_len {
                        break;
                    }
                }
            }
            if best.1 >= 3 {
                let code = 0x8000u16 | ((best.0 as u16) << 3) | (best.1 as u16 - 3);
                out.extend_from_slice(&code.to_be_bytes());
                remember(&mut seen, i, i + best.1);
                i += best.1;
                continue;
            }
        }
        let b = input[i];
        // A space followed by a printable character packs into one byte.
        if b == b' ' && i + 1 < input.len() && (0x40..0x80).contains(&input[i + 1]) {
            out.push(input[i + 1] ^ 0x80);
            remember(&mut seen, i, i + 2);
            i += 2;
            continue;
        }
        if b == 0 || (0x09..0x80).contains(&b) {
            out.push(b);
            remember(&mut seen, i, i + 1);
            i += 1;
            continue;
        }
        // A run of bytes that can't stand alone: count, then the bytes.
        let mut n = 1;
        while n < 8 && i + n < input.len() && !(input[i + n] == 0 || (0x09..0x80).contains(&input[i + n])) {
            n += 1;
        }
        out.push(n as u8);
        out.extend_from_slice(&input[i..i + n]);
        remember(&mut seen, i, i + n);
        i += n;
    }
    out
}

/// Decompress one record.
pub fn decompress(input: &[u8]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::with_capacity(input.len() * 2);
    let mut i = 0;
    while i < input.len() {
        let b = input[i];
        i += 1;
        match b {
            0x01..=0x08 => {
                let end = (i + usize::from(b)).min(input.len());
                out.extend_from_slice(&input[i..end]);
                i = end;
            }
            0x00 | 0x09..=0x7F => out.push(b),
            0x80..=0xBF => {
                let Some(&next) = input.get(i) else { break };
                i += 1;
                let code = (u16::from(b) << 8) | u16::from(next);
                let distance = usize::from((code >> 3) & 0x07FF);
                let len = usize::from(code & 7) + 3;
                if distance == 0 || distance > out.len() {
                    break; // corrupt
                }
                for _ in 0..len {
                    out.push(out[out.len() - distance]);
                }
            }
            _ => {
                out.push(b' ');
                out.push(b ^ 0x80);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(data: &[u8]) {
        let packed = compress(data);
        assert_eq!(decompress(&packed), data, "round trip of {} bytes", data.len());
    }

    #[test]
    fn round_trips() {
        round_trip(b"");
        round_trip(b"a");
        round_trip(b"Hello Hello Hello, the cat sat on the mat. The cat sat on the mat!");
        round_trip("caf\u{e9} \u{201c}quoted\u{201d} — ünïcödé".as_bytes());
        round_trip(&[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 0x80, 0xFF, 0xC0, 0x20, 0x41, 0x20, 0x20]);
        let html =
            "<p class=\"x\">It was a bright cold day in April, and the clocks were striking thirteen.</p>\n".repeat(60);
        round_trip(&html.as_bytes()[..4096]);
        let all: Vec<u8> = (0..4096).map(|i| (i * 7 % 256) as u8).collect();
        round_trip(&all);
    }

    #[test]
    fn compresses_repetitive_text() {
        let html = "<p>The quick brown fox jumps over the lazy dog.</p>\n".repeat(80);
        let data = &html.as_bytes()[..4096];
        assert!(compress(data).len() < data.len() / 3, "{}", compress(data).len());
    }
}
