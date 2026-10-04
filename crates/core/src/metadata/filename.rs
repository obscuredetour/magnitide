//! Metadata guessed from a file name, for formats we can't read yet and as
//! a fallback for incomplete files.

use std::path::Path;

use super::{Author, BookMetadata};

/// "The Dispossessed - Ursula K. Le Guin.epub" → title and author. Splits on
/// the last " - " (calibre's default pattern). Underscores read as spaces.
pub fn parse(path: &Path) -> BookMetadata {
    let stem = book_stem(path);
    let stem = stem.replace('_', " ");
    let stem = stem.split_whitespace().collect::<Vec<_>>().join(" ");
    let (title, author) = match stem.rsplit_once(" - ") {
        Some((t, a)) if !t.trim().is_empty() && !a.trim().is_empty() => {
            (t.trim().to_owned(), Some(a.trim().to_owned()))
        }
        _ => (stem.clone(), None),
    };
    BookMetadata {
        title: (!title.is_empty()).then_some(title),
        authors: author.into_iter().map(|name| Author { name, sort: None }).collect(),
        ..Default::default()
    }
}

/// File name without its book extension(s): "Dune.kepub.epub" → "Dune".
pub fn book_stem(path: &Path) -> String {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let lower = name.to_lowercase();
    if lower.ends_with(".kepub.epub") {
        return name[..name.len() - ".kepub.epub".len()].to_owned();
    }
    match name.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem.to_owned(),
        _ => name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_and_author_from_name() {
        let m = parse(Path::new("/books/The Dispossessed - Ursula K. Le Guin.epub"));
        assert_eq!(m.title.as_deref(), Some("The Dispossessed"));
        assert_eq!(m.authors[0].name, "Ursula K. Le Guin");
    }

    #[test]
    fn last_separator_wins_and_underscores_are_spaces() {
        let m = parse(Path::new("Star Wars - A New Hope - George_Lucas.pdf"));
        assert_eq!(m.title.as_deref(), Some("Star Wars - A New Hope"));
        assert_eq!(m.authors[0].name, "George Lucas");
    }

    #[test]
    fn title_only() {
        let m = parse(Path::new("Untitled_Notes.txt"));
        assert_eq!((m.title.as_deref(), m.authors.len()), (Some("Untitled Notes"), 0));
    }

    #[test]
    fn stems() {
        assert_eq!(book_stem(Path::new("Dune.kepub.epub")), "Dune");
        assert_eq!(book_stem(Path::new("Dune.EPUB")), "Dune");
        assert_eq!(book_stem(Path::new("v1.2 Notes.pdf")), "v1.2 Notes");
    }
}
