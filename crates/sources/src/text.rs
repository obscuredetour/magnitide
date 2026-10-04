//! Cleaning up what sources return.

use chrono::NaiveDate;

/// Free-form publication dates ("May 1, 1974", "July 1997", "1994",
/// "2009-10-13") as ISO dates; a year alone becomes 1 January.
pub(crate) fn iso_date(s: &str) -> Option<String> {
    let s = s.trim().trim_end_matches('.');
    // "%B %d %Y" reads "July 1997" as 19 July 97: reject implausible years.
    let plausible = |d: NaiveDate| (chrono::Datelike::year(&d) > 1000).then_some(d);
    let full = ["%Y-%m-%d", "%B %d, %Y", "%b %d, %Y", "%d %B %Y", "%B %d %Y", "%b. %d, %Y"];
    let date = full
        .iter()
        .find_map(|f| NaiveDate::parse_from_str(s, f).ok().and_then(plausible))
        .or_else(|| {
            ["%Y-%m", "%B %Y", "%b %Y"]
                .iter()
                .find_map(|f| NaiveDate::parse_from_str(&format!("{s} 1"), &format!("{f} %d")).ok().and_then(plausible))
        })
        .or_else(|| {
            // Any four-digit year in the text: "c1994", "1994 printing".
            let year = s
                .as_bytes()
                .windows(4)
                .enumerate()
                .find(|(i, w)| {
                    w.iter().all(u8::is_ascii_digit)
                        && !s.as_bytes().get(i + 4).is_some_and(u8::is_ascii_digit)
                        && (*i == 0 || !s.as_bytes()[i - 1].is_ascii_digit())
                })
                .and_then(|(i, _)| s[i..i + 4].parse().ok())?;
            NaiveDate::from_ymd_opt(year, 1, 1).and_then(plausible)
        })?;
    Some(date.format("%Y-%m-%d").to_string())
}

/// Plain text (Open Library's descriptions) as HTML paragraphs. Their
/// trailing source notes ("----------\nAlso contained in: …") and Markdown
/// reference links are dropped.
pub(crate) fn text_to_html(s: &str) -> Option<String> {
    let s = s.replace("\r\n", "\n");
    let body = s.split("\n----------").next().unwrap_or(&s);
    let paragraphs: Vec<String> = body
        .split("\n\n")
        .map(|p| {
            p.lines()
                .filter(|l| !l.trim_start().starts_with('[') || !l.contains("]: "))
                .map(escape)
                .collect::<Vec<_>>()
                .join("<br>")
        })
        .filter(|p| !p.trim().is_empty())
        .map(|p| format!("<p>{}</p>", p.trim()))
        .collect();
    (!paragraphs.is_empty()).then(|| paragraphs.join(""))
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Subjects worth offering as tags: short, no catalogue syntax, no
/// duplicates (case-insensitive), at most `max`.
pub(crate) fn tags(subjects: impl IntoIterator<Item = String>, max: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for s in subjects {
        let s = s.trim().to_owned();
        let usable = !s.is_empty() && s.chars().count() <= 32 && !s.contains([':', '=', '(', '"', '/']);
        if usable && !out.iter().any(|o| o.eq_ignore_ascii_case(&s)) {
            out.push(s);
            if out.len() == max {
                break;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_dates() {
        assert_eq!(iso_date("May 1, 1974").as_deref(), Some("1974-05-01"));
        assert_eq!(iso_date("July 1997").as_deref(), Some("1997-07-01"));
        assert_eq!(iso_date("1994").as_deref(), Some("1994-01-01"));
        assert_eq!(iso_date("2009-10-13").as_deref(), Some("2009-10-13"));
        assert_eq!(iso_date("2009-10").as_deref(), Some("2009-10-01"));
        assert_eq!(iso_date("c1994.").as_deref(), Some("1994-01-01"));
        assert_eq!(iso_date("someday"), None);
    }

    #[test]
    fn plain_text_becomes_paragraphs() {
        let s = "Shevek <b> leaves.\nHe returns.\n\nSecond.\n\n----------\nAlso contained in:\n[1]: https://x";
        assert_eq!(text_to_html(s).as_deref(), Some("<p>Shevek &lt;b&gt; leaves.<br>He returns.</p><p>Second.</p>"));
        assert_eq!(text_to_html("  \n\n "), None);
    }

    #[test]
    fn picks_usable_tags() {
        let subjects = [
            "Fiction",
            "fiction",
            "Science fiction",
            "Fiction, general",
            "Accessible book",
            "place:Anarres",
            "A very very long subject heading indeed, yes",
        ];
        assert_eq!(tags(subjects.map(String::from), 3), ["Fiction", "Science fiction", "Fiction, general"]);
    }
}
