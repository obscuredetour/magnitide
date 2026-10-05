//! KOReader's document fingerprint ("partial MD5", `util.partialMD5`), which
//! its progress sync uses to name a book: MD5 over 1 KiB samples at offsets
//! 0, 1 KiB, 4 KiB, 16 KiB, … (×4 each time, up to 1 GiB), stopping at the
//! end of the file. Any change to those bytes (writing metadata into an EPUB,
//! say) gives the file a new fingerprint, so a book is known by the copy the
//! reader opened, not the library's.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

use md5::{Digest, Md5};

const SAMPLE: u64 = 1024;

/// The fingerprint of the file at `path`, as KOReader computes it (lower-case hex).
pub fn partial_md5(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    let mut hash = Md5::new();
    let mut buf = [0u8; SAMPLE as usize];
    // KOReader loops i = -1..=10 over `lshift(1024, 2*i)`; LuaJIT masks the
    // negative shift to 30, which overflows 32 bits to 0: so offset 0 first.
    for i in 0..=11u32 {
        let offset = if i == 0 { 0 } else { SAMPLE << (2 * (i - 1)) };
        // Lua's read at or past the end returns nil, which ends the loop.
        if offset >= len {
            break;
        }
        file.seek(SeekFrom::Start(offset))?;
        let n = read_up_to(&mut file, &mut buf)?;
        hash.update(&buf[..n]);
    }
    Ok(hash.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Fill `buf` as far as the file allows (a short read is only the end of the file).
fn read_up_to(file: &mut File, buf: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match file.read(&mut buf[filled..])? {
            0 => break,
            n => filled += n,
        }
    }
    Ok(filled)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The same algorithm written out the slow way: whole samples from a buffer.
    fn reference(data: &[u8]) -> String {
        let mut hash = Md5::new();
        for i in -1i32..=10 {
            let offset = if i < 0 { 0 } else { 1024usize << (2 * i) };
            if offset >= data.len() {
                break;
            }
            hash.update(&data[offset..data.len().min(offset + 1024)]);
        }
        hash.finalize().iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn matches_koreader() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("book.epub");
        // Small (one partial sample), mid-sized (several), and past the 1 MiB sample.
        for len in [10usize, 1024, 5000, 70_000, 1_200_000] {
            let data: Vec<u8> = (0..len).map(|i| (i * 31 % 251) as u8).collect();
            std::fs::write(&path, &data).unwrap();
            assert_eq!(partial_md5(&path).unwrap(), reference(&data), "{len} bytes");
        }
        // An empty file is MD5 of nothing.
        std::fs::write(&path, b"").unwrap();
        assert_eq!(partial_md5(&path).unwrap(), "d41d8cd98f00b204e9800998ecf8427e");
    }

    #[test]
    fn known_value() {
        // Under 1 KiB, the fingerprint is the plain MD5 of the file.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        std::fs::write(&path, b"The quick brown fox jumps over the lazy dog").unwrap();
        assert_eq!(partial_md5(&path).unwrap(), "9e107d9d372bb6826bd81d3542a419d6");
    }
}
