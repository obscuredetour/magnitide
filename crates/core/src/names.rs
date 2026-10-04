//! Sort forms of titles and author names.

pub use calibre_compat::sqlfuncs::title_sort;

/// Honorifics dropped from the sort form (when a full name remains).
const PREFIXES: &[&str] = &["mr", "mrs", "ms", "dr", "prof", "sir", "dame"];
/// Generational and similar suffixes kept after the given names.
const SUFFIXES: &[&str] = &["jr", "sr", "ii", "iii", "iv", "phd", "md"];

/// "Ursula K. Le Guin" → "Guin, Ursula K. Le".
///
/// Follows calibre's default algorithm (last word is the surname), so
/// imported and newly added authors sort consistently. Like calibre, it can't
/// know that "Le Guin" is one surname; editable sort names will fix those.
/// Names already in "Surname, Given" form, and single words, are kept.
pub fn author_sort(name: &str) -> String {
    let name = name.trim();
    if name.contains(',') || !name.contains(' ') {
        return name.to_owned();
    }
    let norm = |w: &str| w.trim_end_matches('.').to_ascii_lowercase();
    let mut words: Vec<&str> = name.split_whitespace().collect();
    // Drop honorifics only while a given name and surname remain ("Dr. Seuss" stays).
    while words.len() > 2 && PREFIXES.contains(&norm(words[0]).as_str()) {
        words.remove(0);
    }
    let mut suffix = Vec::new();
    while words.len() > 1 && SUFFIXES.contains(&norm(words[words.len() - 1]).as_str()) {
        suffix.insert(0, words.pop().unwrap_or_default());
    }
    match words.split_last() {
        Some((last, rest)) if !rest.is_empty() => {
            let mut given = rest.join(" ");
            if !suffix.is_empty() {
                given = format!("{given} {}", suffix.join(" "));
            }
            format!("{last}, {given}")
        }
        _ => name.to_owned(),
    }
}

/// "King, Stephen" → ("Stephen King", sort "King, Stephen"); None for names
/// that aren't surname-first ("Stephen King", "Martin Luther King, Jr.",
/// "Smith, Jones, and Co.").
pub fn from_surname_first(name: &str) -> Option<(String, String)> {
    let (surname, given) = name.trim().split_once(',')?;
    let (surname, given) = (surname.trim(), given.trim());
    let norm = given.trim_end_matches('.').to_ascii_lowercase();
    let ok = !surname.is_empty()
        && !given.is_empty()
        && !given.contains(',')
        && !SUFFIXES.contains(&norm.as_str())
        && surname.split_whitespace().count() <= 3;
    ok.then(|| (format!("{given} {surname}"), format!("{surname}, {given}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surname_first() {
        assert_eq!(author_sort("Ursula K. Le Guin"), "Guin, Ursula K. Le");
        assert_eq!(author_sort("Octavia E. Butler"), "Butler, Octavia E.");
        assert_eq!(author_sort("  N. K.  Jemisin "), "Jemisin, N. K.");
    }

    #[test]
    fn handles_prefixes_suffixes_and_ready_forms() {
        assert_eq!(author_sort("Dr. Seuss"), "Seuss, Dr.");
        assert_eq!(author_sort("Mr. John Smith"), "Smith, John");
        assert_eq!(author_sort("Martin Luther King Jr."), "King, Martin Luther Jr.");
        assert_eq!(author_sort("Le Guin, Ursula K."), "Le Guin, Ursula K.");
        assert_eq!(author_sort("Plato"), "Plato");
    }

    #[test]
    fn turns_surname_first_names_around() {
        assert_eq!(from_surname_first("King, Stephen"), Some(("Stephen King".into(), "King, Stephen".into())));
        assert_eq!(
            from_surname_first("Le Guin, Ursula K."),
            Some(("Ursula K. Le Guin".into(), "Le Guin, Ursula K.".into()))
        );
        assert_eq!(from_surname_first("Stephen King"), None);
        assert_eq!(from_surname_first("Martin Luther King, Jr."), None);
        assert_eq!(from_surname_first("Smith, Jones, and Co."), None);
    }
}
