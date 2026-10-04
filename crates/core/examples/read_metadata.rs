//! Print the metadata Magnitide reads from book files (for checking real-world files):
//!
//!     cargo run --release -p magnitide-core --example read_metadata -- <file or folder>...

use std::path::{Path, PathBuf};

fn main() {
    let mut files = Vec::new();
    for arg in std::env::args().skip(1) {
        collect(Path::new(&arg), &mut files);
    }
    let (mut with_cover, mut with_author, mut with_series) = (0, 0, 0);
    for f in &files {
        let format = f.extension().map(|e| e.to_string_lossy().to_uppercase()).unwrap_or_default();
        let m = magnitide_core::metadata::read(f, &format);
        with_cover += m.cover.is_some() as usize;
        with_author += !m.authors.is_empty() as usize;
        with_series += m.series.is_some() as usize;
        println!(
            "{:<45} | {:<28} | {:<22} | cover {} | {}",
            m.title.unwrap_or_default().chars().take(45).collect::<String>(),
            m.authors
                .iter()
                .map(|a| a.name.as_str())
                .collect::<Vec<_>>()
                .join(" & ")
                .chars()
                .take(28)
                .collect::<String>(),
            m.series.map(|(s, i)| format!("{s} #{i}")).unwrap_or_default().chars().take(22).collect::<String>(),
            m.cover.map(|c| format!("{:>4} KB", c.len() / 1024)).unwrap_or_else(|| "   none".into()),
            m.identifiers.keys().cloned().collect::<Vec<_>>().join(","),
        );
    }
    eprintln!("{} files: {with_author} with authors, {with_cover} with covers, {with_series} with series", files.len());
}

fn collect(path: &Path, out: &mut Vec<PathBuf>) {
    if path.is_dir() {
        for entry in std::fs::read_dir(path).into_iter().flatten().flatten() {
            collect(&entry.path(), out);
        }
    } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("epub")) {
        out.push(path.to_path_buf());
    }
}
