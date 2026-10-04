//! Human-readable, portable file and folder names for the library on disk.
//!
//! Layout: `Author/Title (id)/Title - Author.ext`. Names must be valid on
//! macOS, Windows and Linux at once, because libraries move between machines
//! (and onto NAS shares that may be accessed from Windows).

/// Longest folder or file-stem name, in characters. Keeps full paths well
/// inside Windows' traditional 260-character limit.
const MAX_COMPONENT: usize = 80;

/// One path component made safe for every OS we support.
pub fn sanitize(name: &str, max_chars: usize) -> String {
    // Characters reserved on Windows (and '/' everywhere); control characters.
    let replaced: String =
        name.chars().map(|c| if c.is_control() || r#"/\:*?"<>|"#.contains(c) { '_' } else { c }).collect();
    let collapsed = replaced.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out: String = collapsed.chars().take(max_chars).collect();
    // Windows drops trailing dots and spaces; a leading dot hides files on Unix.
    out = out.trim_end_matches(['.', ' ']).trim_start_matches('.').trim().to_owned();
    if out.is_empty() {
        return "Unknown".to_owned();
    }
    // Device names Windows reserves, with or without an extension.
    let stem = out.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit());
    if reserved { format!("_{out}") } else { out }
}

/// A book's folder relative to the library root, always '/'-separated:
/// `"Ursula K. Le Guin/The Dispossessed (42)"`. The id keeps it unique.
pub fn book_dir(first_author: &str, title: &str, id: i64) -> String {
    format!("{}/{} ({id})", sanitize(first_author, MAX_COMPONENT), sanitize(title, MAX_COMPONENT))
}

/// A book file's name: `"The Dispossessed - Ursula K. Le Guin.epub"`.
pub fn book_file(title: &str, first_author: &str, format: &str) -> String {
    let stem = sanitize(&format!("{title} - {first_author}"), MAX_COMPONENT * 3 / 2);
    format!("{stem}.{}", format.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_characters_invalid_anywhere() {
        assert_eq!(sanitize(r#"What? A "Book": Part 1/2"#, 80), "What_ A _Book__ Part 1_2");
        assert_eq!(sanitize("tab\there", 80), "tab_here");
    }

    #[test]
    fn trims_dots_spaces_and_hidden_prefixes() {
        assert_eq!(sanitize("  Ends with dots... ", 80), "Ends with dots");
        assert_eq!(sanitize(".hidden", 80), "hidden");
        assert_eq!(sanitize("   ", 80), "Unknown");
        assert_eq!(sanitize("...", 80), "Unknown");
    }

    #[test]
    fn avoids_windows_device_names() {
        assert_eq!(sanitize("CON", 80), "_CON");
        assert_eq!(sanitize("com1.txt", 80), "_com1.txt");
        assert_eq!(sanitize("Console", 80), "Console");
    }

    #[test]
    fn truncates_by_characters_not_bytes() {
        let long = "ü".repeat(100);
        assert_eq!(sanitize(&long, 80).chars().count(), 80);
    }

    #[test]
    fn builds_layout() {
        assert_eq!(book_dir("Ursula K. Le Guin", "The Dispossessed", 42), "Ursula K. Le Guin/The Dispossessed (42)");
        assert_eq!(
            book_file("The Dispossessed", "Ursula K. Le Guin", "EPUB"),
            "The Dispossessed - Ursula K. Le Guin.epub"
        );
        assert_eq!(book_dir("AC/DC", "Live: 1977", 7), "AC_DC/Live_ 1977 (7)");
    }
}
