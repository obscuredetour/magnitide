//! Converting books between formats.
//!
//! EPUB → KEPUB is done natively ([`kepub`]). Everything else goes through
//! calibre's `ebook-convert` command when calibre is installed (detected,
//! never bundled): AZW3 and MOBI for Kindles, EPUB from other formats, and
//! so on. Conversions read a book file and write a new one; the source is
//! never changed.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;

use crate::error::{Error, Result};
use crate::import::store::add_formats;
use crate::library::{BookId, Library};

pub mod kepub;

/// Formats calibre's `ebook-convert` reads (KEPUB as EPUB). KFX isn't one.
const CALIBRE_INPUTS: &[&str] = &[
    "EPUB", "KEPUB", "AZW3", "MOBI", "AZW", "PRC", "PDB", "DOCX", "ODT", "RTF", "FB2", "FBZ", "HTML", "HTM", "TXT",
    "LIT", "CBZ", "CBR", "CB7", "CHM", "DJVU", "PDF",
];
/// Formats it writes that are worth offering.
const CALIBRE_OUTPUTS: &[&str] = &["EPUB", "AZW3", "MOBI", "PDF", "DOCX", "FB2", "TXT"];
/// Which of a book's files to convert from, best first (PDF is a last resort:
/// its layout rarely survives).
const SOURCE_PREFERENCE: &[&str] = &[
    "EPUB", "KEPUB", "AZW3", "MOBI", "AZW", "DOCX", "ODT", "FB2", "FBZ", "RTF", "HTML", "HTM", "LIT", "PRC", "PDB",
    "TXT", "CBZ", "CBR", "CB7", "CHM", "DJVU", "PDF",
];

/// What can convert: always the native converters, plus calibre if found.
#[derive(Debug, Clone, Default)]
pub struct Converter {
    calibre: Option<PathBuf>,
}

impl Converter {
    /// Native conversions, plus calibre's `ebook-convert` if it's installed.
    pub fn detect() -> Self {
        Self { calibre: find_ebook_convert() }
    }

    /// A specific `ebook-convert` (or none).
    pub fn with_calibre(path: Option<PathBuf>) -> Self {
        Self { calibre: path }
    }

    pub fn calibre(&self) -> Option<&Path> {
        self.calibre.as_deref()
    }

    /// The format to convert from, of `have`, to get `to`; None if it can't be done.
    pub fn source_for<'a>(&self, have: &'a [String], to: &str) -> Option<&'a str> {
        if have.iter().any(|h| h == to) {
            return None;
        }
        let ok = |from: &str| match (from, to) {
            ("EPUB", "KEPUB") => true,
            // calibre to EPUB, then native.
            (_, "KEPUB") => self.calibre.is_some() && CALIBRE_INPUTS.contains(&from),
            _ => self.calibre.is_some() && CALIBRE_INPUTS.contains(&from) && CALIBRE_OUTPUTS.contains(&to),
        };
        SOURCE_PREFERENCE.iter().find_map(|p| have.iter().find(|h| h == p).filter(|h| ok(h))).map(String::as_str)
    }

    /// Every format a book with `have` can be converted to.
    pub fn targets(&self, have: &[String]) -> Vec<String> {
        let mut all: Vec<&str> = vec!["KEPUB"];
        all.extend(CALIBRE_OUTPUTS);
        all.into_iter().filter(|t| self.source_for(have, t).is_some()).map(str::to_owned).collect()
    }

    /// Convert `src` (in format `from`) to `to`, writing `dest` (must not exist).
    pub fn convert(&self, src: &Path, from: &str, to: &str, dest: &Path) -> std::result::Result<(), String> {
        match (from, to) {
            ("EPUB", "KEPUB") => kepub::convert(src, dest).map(|_| ()).map_err(|e| e.to_string()),
            (_, "KEPUB") => {
                let work = TempDir::new()?;
                let epub = work.0.join("book.epub");
                self.calibre_convert(src, from, &epub)?;
                kepub::convert(&epub, dest).map(|_| ()).map_err(|e| e.to_string())
            }
            _ => self.calibre_convert(src, from, dest),
        }
    }

    /// Run `ebook-convert`; the output format follows `dest`'s extension.
    fn calibre_convert(&self, src: &Path, from: &str, dest: &Path) -> std::result::Result<(), String> {
        let tool = self.calibre.as_deref().ok_or("converting to this format needs calibre installed")?;
        let work = TempDir::new()?;
        // It decides the input format by extension: give it one it knows.
        let ext = match from {
            "KEPUB" => "epub".to_owned(),
            f => f.to_ascii_lowercase(),
        };
        let input = work.0.join(format!("input.{ext}"));
        std::fs::copy(src, &input).map_err(|e| e.to_string())?;
        let output = work
            .0
            .join(format!("output.{}", dest.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default()));
        let out = Command::new(tool).arg(&input).arg(&output).output().map_err(|e| e.to_string())?;
        if !out.status.success() || !output.is_file() {
            let log = String::from_utf8_lossy(&out.stderr);
            let last = log.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("no output").trim().to_owned();
            return Err(format!("calibre couldn't convert it: {last}"));
        }
        if dest.exists() {
            return Err(format!("{} already exists", dest.display()));
        }
        std::fs::copy(&output, dest).map(|_| ()).map_err(|e| e.to_string())
    }
}

/// Where calibre's converter usually lives.
fn find_ebook_convert() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if cfg!(target_os = "macos") {
        candidates.push("/Applications/calibre.app/Contents/MacOS/ebook-convert".into());
        if let Some(home) = std::env::var_os("HOME") {
            candidates.push(Path::new(&home).join("Applications/calibre.app/Contents/MacOS/ebook-convert"));
        }
    } else if cfg!(windows) {
        for var in ["ProgramFiles", "ProgramFiles(x86)"] {
            if let Some(dir) = std::env::var_os(var) {
                candidates.push(Path::new(&dir).join("Calibre2").join("ebook-convert.exe"));
            }
        }
    } else {
        candidates.push("/opt/calibre/ebook-convert".into());
        candidates.push("/usr/bin/ebook-convert".into());
    }
    let exe = if cfg!(windows) { "ebook-convert.exe" } else { "ebook-convert" };
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&path).map(|d| d.join(exe)));
    }
    candidates.into_iter().find(|p| p.is_file())
}

/// A scratch folder, removed when dropped.
pub(crate) struct TempDir(pub(crate) PathBuf);

impl TempDir {
    pub(crate) fn new() -> std::result::Result<Self, String> {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "magnitide-convert-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        Ok(Self(dir))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Debug, Default, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ConvertReport {
    /// Books that got the new format.
    pub converted: usize,
    /// Books that already had it.
    pub already: usize,
    /// Books that couldn't be converted, with the reason.
    pub failed: Vec<String>,
}

impl Library {
    /// Convert each book to `to` and add the result as a new format.
    pub fn convert_books(
        &mut self,
        ids: &[BookId],
        to: &str,
        converter: &Converter,
        mut progress: impl FnMut(usize, usize, &str),
    ) -> Result<ConvertReport> {
        let mut report = ConvertReport::default();
        for (done, id) in ids.iter().enumerate() {
            let book = self.book(*id)?;
            progress(done, ids.len(), &book.title);
            match self.convert_book(*id, to, converter) {
                Ok(true) => report.converted += 1,
                Ok(false) => report.already += 1,
                Err(e) => report.failed.push(format!("“{}”: {e}", book.title)),
            }
        }
        progress(ids.len(), ids.len(), "");
        Ok(report)
    }

    /// Add format `to` to a book by converting one of its files. False if it
    /// already has that format.
    pub fn convert_book(&mut self, id: BookId, to: &str, converter: &Converter) -> Result<bool> {
        let book = self.book(id)?;
        let files: Vec<(String, String)> = {
            let mut stmt = self.conn.prepare("SELECT format, filename FROM files WHERE book_id = ?")?;
            let rows = stmt.query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        let have: Vec<String> = files.iter().map(|(f, _)| f.clone()).collect();
        if have.iter().any(|f| f == to) {
            return Ok(false);
        }
        let from = converter
            .source_for(&have, to)
            .ok_or_else(|| Error::InvalidBook(format!("can't be converted to {to} from {}", have.join(", "))))?
            .to_owned();
        let filename = &files.iter().find(|(f, _)| *f == from).expect("from is one of the book's formats").1;
        let src = crate::edit::dir_for(&self.root, &book.path).join(filename);

        let work = TempDir::new().map_err(Error::Import)?;
        let out = work.0.join(format!("converted.{}", to.to_ascii_lowercase()));
        converter.convert(&src, &from, to, &out).map_err(Error::Import)?;
        add_formats(self, id, &[(to.to_owned(), out)])?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| (*x).to_owned()).collect()
    }

    #[test]
    fn routes_conversions() {
        let native = Converter::with_calibre(None);
        assert_eq!(native.source_for(&s(&["PDF", "EPUB"]), "KEPUB"), Some("EPUB"));
        assert_eq!(native.source_for(&s(&["PDF"]), "KEPUB"), None, "needs calibre");
        assert_eq!(native.source_for(&s(&["EPUB"]), "AZW3"), None, "needs calibre");
        assert_eq!(native.targets(&s(&["EPUB"])), ["KEPUB"]);

        let calibre = Converter::with_calibre(Some("/x/ebook-convert".into()));
        assert_eq!(calibre.source_for(&s(&["PDF", "MOBI"]), "EPUB"), Some("MOBI"), "PDF is a last resort");
        assert_eq!(calibre.source_for(&s(&["PDF"]), "KEPUB"), Some("PDF"));
        assert_eq!(calibre.source_for(&s(&["KFX"]), "EPUB"), None);
        assert_eq!(calibre.source_for(&s(&["EPUB"]), "EPUB"), None);
        assert_eq!(calibre.targets(&s(&["EPUB"])), ["KEPUB", "AZW3", "MOBI", "PDF", "DOCX", "FB2", "TXT"]);
    }
}
