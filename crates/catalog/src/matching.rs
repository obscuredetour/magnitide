//! Deciding whether two descriptions are the same book: a metadata lookup
//! result and a library book, or a file on an e-reader and a library book.
//!
//! Plain string comparison after normalizing: no learned or AI ranking, so a
//! score is always explainable ("same ISBN", "title and author match").

/// A title reduced to what identifies it: ASCII, lower case, no subtitle,
/// punctuation or leading article. "The Dispossessed: An Ambiguous Utopia"
/// → "dispossessed".
pub fn normalize_title(title: &str) -> String {
    let main = title.split([':', '(', '[']).next().unwrap_or(title);
    let words = words(main);
    let skip = usize::from(words.len() > 1 && matches!(words[0].as_str(), "the" | "a" | "an"));
    words[skip..].join(" ")
}

/// An author's surname, normalized: "Ursula K. Le Guin" → "guin",
/// "Le Guin, Ursula K." → "guin".
pub fn surname(author: &str) -> String {
    let last = match author.split_once(',') {
        Some((before, _)) => before,
        None => author,
    };
    words(last).pop().unwrap_or_default()
}

/// ASCII, lower case, alphanumeric words.
fn words(s: &str) -> Vec<String> {
    deunicode::deunicode(s)
        .to_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect()
}

/// 0–1: how alike two titles are once normalized.
pub fn title_similarity(a: &str, b: &str) -> f64 {
    let (a, b) = (normalize_title(a), normalize_title(b));
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    strsim::jaro_winkler(&a, &b)
}

/// 0–1: the share of `wanted` authors (by surname) found in `found`.
/// 1 when nothing is wanted, so a title-only search isn't penalized.
pub fn author_similarity(wanted: &[String], found: &[String]) -> f64 {
    let wanted: Vec<String> = wanted.iter().map(|a| surname(a)).filter(|s| !s.is_empty()).collect();
    if wanted.is_empty() {
        return 1.0;
    }
    let found: Vec<String> = found.iter().map(|a| surname(a)).collect();
    let hits = wanted.iter().filter(|w| found.iter().any(|f| strsim::jaro_winkler(w, f) > 0.92)).count();
    hits as f64 / wanted.len() as f64
}

/// Digits (and a final X) of an ISBN, as ISBN-13. None if it isn't one.
pub fn isbn13(s: &str) -> Option<String> {
    let raw: String = s.chars().filter(|c| c.is_ascii_digit() || *c == 'X' || *c == 'x').collect();
    match raw.len() {
        13 if raw.bytes().all(|b| b.is_ascii_digit()) => Some(raw),
        10 if raw[..9].bytes().all(|b| b.is_ascii_digit()) => {
            let body = format!("978{}", &raw[..9]);
            let sum: u32 =
                body.bytes().enumerate().map(|(i, b)| u32::from(b - b'0') * if i % 2 == 0 { 1 } else { 3 }).sum();
            Some(format!("{body}{}", (10 - sum % 10) % 10))
        }
        _ => None,
    }
}

/// How well a candidate matches what was looked up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum MatchKind {
    /// Title or author differ noticeably.
    Possible,
    /// Title and authors match.
    Strong,
    /// Same ISBN.
    Isbn,
}

/// Score a candidate (0–1) against what was looked up.
pub fn score(
    want_title: &str,
    want_authors: &[String],
    want_isbn: Option<&str>,
    title: &str,
    authors: &[String],
    isbns: &[String],
) -> (f64, MatchKind) {
    if let Some(want) = want_isbn.and_then(isbn13)
        && isbns.iter().filter_map(|i| isbn13(i)).any(|i| i == want)
    {
        return (1.0, MatchKind::Isbn);
    }
    let t = if want_title.trim().is_empty() { 1.0 } else { title_similarity(want_title, title) };
    let s = 0.7 * t + 0.3 * author_similarity(want_authors, authors);
    (s, if t > 0.93 && s > 0.9 { MatchKind::Strong } else { MatchKind::Possible })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn normalizes_titles() {
        assert_eq!(normalize_title("The Dispossessed: An Ambiguous Utopia"), "dispossessed");
        assert_eq!(normalize_title("Ægypt (Ægypt #1)"), "aegypt");
        assert_eq!(normalize_title("A"), "a");
        assert_eq!(normalize_title("Ender's Game"), "ender s game");
    }

    #[test]
    fn surnames_either_order() {
        assert_eq!(surname("Ursula K. Le Guin"), "guin");
        assert_eq!(surname("Le Guin, Ursula K."), "guin");
        assert_eq!(surname("Gabriel García Márquez"), "marquez");
    }

    #[test]
    fn converts_isbns() {
        assert_eq!(isbn13("0-06-105488-7").as_deref(), Some("9780061054884"));
        assert_eq!(isbn13("978-0-06-105488-4").as_deref(), Some("9780061054884"));
        assert_eq!(isbn13("12345"), None);
    }

    #[test]
    fn scores() {
        let le_guin = v(&["Ursula K. Le Guin"]);
        let (s, k) = score("The Dispossessed", &le_guin, Some("0061054887"), "x", &[], &v(&["9780061054884"]));
        assert_eq!((s, k), (1.0, MatchKind::Isbn));

        let (s, k) = score("The Dispossessed", &le_guin, None, "Dispossessed", &v(&["Le Guin, Ursula"]), &[]);
        assert!(s > 0.99, "{s}");
        assert_eq!(k, MatchKind::Strong);

        let (wrong_author, k) =
            score("The Dispossessed", &le_guin, None, "The Dispossessed", &v(&["Someone Else"]), &[]);
        assert_eq!(k, MatchKind::Possible);
        let (wrong_title, _) = score("The Dispossessed", &le_guin, None, "The Lathe of Heaven", &le_guin, &[]);
        assert!(wrong_title < wrong_author && wrong_author < s);
    }
}
