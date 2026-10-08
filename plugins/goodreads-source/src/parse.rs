//! Parsing Goodreads pages: the search results table, and the data a
//! book page embeds for its own scripts. No bindings in here, so it's
//! tested natively on saved pages (tests/fixtures).

use magnitide_plugin_sdk::prelude::{BookRecord, Identifier};
use serde_json::Value;

/// One row of the search results.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// Goodreads' numeric book id.
    pub id: String,
    pub title: String,
    pub authors: Vec<String>,
    pub ratings: u64,
}

/// The search query for a title and first author, as Goodreads' search
/// takes it: the title's words (a subtitle after ':' dropped) and the
/// author's, '+'-joined and percent-encoded.
pub fn query(title: &str, author: Option<&str>) -> String {
    let title = title.split(':').next().unwrap_or("");
    let mut tokens: Vec<String> = words(title);
    tokens.extend(author.map(words).unwrap_or_default());
    tokens.iter().map(|t| encode(t)).collect::<Vec<_>>().join("+")
}

/// The digits (and a final X) of an ISBN; empty when it isn't one.
pub fn clean_isbn(s: &str) -> String {
    let clean: String = s.chars().filter(|c| c.is_ascii_digit() || *c == 'X' || *c == 'x').map(|c| c.to_ascii_uppercase()).collect();
    if clean.len() == 10 || clean.len() == 13 { clean } else { String::new() }
}

/// Whether a page is a book's page (rather than search results).
pub fn is_book_page(html: &str) -> bool {
    next_data(html).is_some()
}

/// The result rows of a search page, in Goodreads' order.
pub fn search_rows(html: &str) -> Vec<Row> {
    html.split("<tr itemscope itemtype=\"http://schema.org/Book\">")
        .skip(1)
        .filter_map(|row| {
            let row = row.split("</tr>").next()?;
            let id: String = after(row, "/book/show/")?.chars().take_while(char::is_ascii_digit).collect();
            if id.is_empty() {
                return None;
            }
            let title = decode(text(after(after(row, "itemprop='name'")?, ">")?)?);
            let authors = row
                .split("class=\"authorName\"")
                .skip(1)
                .filter_map(|a| after(a, "itemprop=\"name\">"))
                .filter_map(text)
                .map(decode)
                .collect();
            let ratings = after(row, "avg rating &mdash; ")
                .map(|s| s.chars().take_while(|c| c.is_ascii_digit() || *c == ',').filter(char::is_ascii_digit).collect::<String>())
                .and_then(|n| n.parse().ok())
                .unwrap_or(0);
            Some(Row { id, title, authors, ratings })
        })
        .collect()
}

/// Rows that match the lookup, likeliest first: every title word found,
/// an author found, few extra words (summaries and study guides have long
/// titles), and popularity to break ties.
pub fn rank(rows: Vec<Row>, title: &str, authors: &[String]) -> Vec<Row> {
    let want: Vec<String> = words(title.split(':').next().unwrap_or("")).iter().map(|w| w.to_lowercase()).collect();
    let want_authors: Vec<String> = authors.iter().flat_map(|a| words(a)).map(|w| w.to_lowercase()).filter(|w| w.len() > 1).collect();
    let mut scored: Vec<(f64, usize, Row)> = rows
        .into_iter()
        .enumerate()
        .filter_map(|(i, row)| {
            let have: Vec<String> = words(&row.title).iter().map(|w| w.to_lowercase()).collect();
            let hits = want.iter().filter(|w| have.contains(w)).count();
            if !want.is_empty() && (hits as f64) < (want.len() as f64) / 2.0 {
                return None;
            }
            let coverage = if want.is_empty() { 1.0 } else { hits as f64 / want.len() as f64 };
            let extra = have.iter().filter(|w| !want.contains(w)).count().min(10) as f64;
            let author_text = row.authors.join(" ").to_lowercase();
            let author = if want_authors.is_empty() {
                0.5
            } else if want_authors.iter().any(|a| author_text.contains(a.as_str())) {
                1.0
            } else {
                0.0
            };
            let popularity = ((row.ratings + 1) as f64).log10() / 10.0;
            Some((2.0 * coverage + author + popularity - 0.03 * extra, i, row))
        })
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal).then(a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, _, row)| row).collect()
}

/// The book a page is about, as a record; None when the page holds none.
pub fn book(html: &str, _looked_up: &str) -> Option<BookRecord> {
    let data: Value = serde_json::from_str(next_data(html)?).ok()?;
    let state = &data["props"]["pageProps"]["apolloState"];
    let state = state.as_object()?;
    // The page's own book: the root query names it; else the first book with a title.
    let own = state
        .get("ROOT_QUERY")
        .and_then(Value::as_object)
        .and_then(|root| root.iter().find(|(k, _)| k.starts_with("getBookByLegacyId")))
        .and_then(|(_, v)| v["__ref"].as_str())
        .and_then(|r| state.get(r));
    let book = own.or_else(|| state.iter().find(|(k, v)| k.starts_with("Book:") && v.get("title").is_some()).map(|(_, v)| v))?;
    let title = book["title"].as_str()?.trim().to_owned();
    if title.is_empty() {
        return None;
    }
    let id = book["legacyId"].as_u64().map(|n| n.to_string()).unwrap_or_default();
    let details = &book["details"];

    // Authors: contributors in the Author role; the primary one whatever its role, failing that.
    let name_of = |edge: &Value| edge["node"]["__ref"].as_str().and_then(|r| state.get(r)).and_then(|c| c["name"].as_str()).map(|n| n.trim().to_owned());
    let primary = &book["primaryContributorEdge"];
    let mut authors: Vec<String> = Vec::new();
    if matches!(primary["role"].as_str(), Some("Author" | "Pseudonym")) {
        authors.extend(name_of(primary));
    }
    for edge in book["secondaryContributorEdges"].as_array().into_iter().flatten() {
        if edge["role"].as_str() == Some("Author") {
            authors.extend(name_of(edge));
        }
    }
    if authors.is_empty() {
        authors.extend(name_of(primary));
    }
    authors.dedup();

    let mut identifiers = Vec::new();
    if !id.is_empty() {
        identifiers.push(Identifier { key: "goodreads".into(), value: id.clone() });
    }
    if let Some(isbn) = details["isbn13"].as_str().filter(|s| s.len() == 13) {
        identifiers.push(Identifier { key: "isbn".into(), value: isbn.to_owned() });
    } else if let Some(isbn) = details["isbn"].as_str().filter(|s| s.len() == 10) {
        identifiers.push(Identifier { key: "isbn".into(), value: isbn.to_owned() });
    }
    if let Some(asin) = details["asin"].as_str().filter(|a| !a.is_empty() && a.chars().any(|c| c.is_ascii_alphabetic())) {
        identifiers.push(Identifier { key: "amazon".into(), value: asin.to_owned() });
    }

    // The edition's date, else the work's first publication.
    let work = book["work"]["__ref"].as_str().and_then(|r| state.get(r));
    let published = details["publicationTime"]
        .as_i64()
        .or_else(|| work.and_then(|w| w["details"]["publicationTime"].as_i64()))
        .map(|ms| date_of(ms / 1000));
    let tags: Vec<String> = book["bookGenres"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|g| g["genre"]["name"].as_str())
        .map(str::to_owned)
        .take(8)
        .collect();
    let cover_url = book["imageUrl"].as_str().filter(|u| !u.contains("nophoto")).map(str::to_owned);
    let description_html = book["description"].as_str().map(str::trim).filter(|d| !d.is_empty()).map(str::to_owned);
    let languages = details["language"]["name"].as_str().and_then(language_code).map(|c| vec![c.to_owned()]).unwrap_or_default();
    let link = book["webUrl"].as_str().map(str::to_owned).or_else(|| (!id.is_empty()).then(|| format!("https://www.goodreads.com/book/show/{id}")));
    let series = series_of(state, book);
    Some(BookRecord {
        source_key: if id.is_empty() { title.clone() } else { id },
        link,
        title,
        subtitle: None,
        authors,
        publisher: details["publisher"].as_str().map(str::trim).filter(|p| !p.is_empty()).map(str::to_owned),
        published,
        languages,
        identifiers,
        tags,
        description_html,
        cover_url,
        pages: details["numPages"].as_u64().and_then(|n| u32::try_from(n).ok()).filter(|n| *n > 0),
        series: series.as_ref().map(|(name, _)| name.clone()),
        series_index: series.and_then(|(_, index)| index),
    })
}

/// The first series a book is in, and its position there ("1", "2.5";
/// None when Goodreads has none or it isn't a number).
fn series_of(state: &serde_json::Map<String, Value>, book: &Value) -> Option<(String, Option<f64>)> {
    let first = book["bookSeries"].as_array()?.first()?;
    let name = first["series"]["__ref"].as_str().and_then(|r| state.get(r)).and_then(|s| s["title"].as_str())?;
    let index = first["userPosition"].as_str().and_then(|p| p.trim().parse().ok());
    Some((name.trim().to_owned(), index))
}

fn next_data(html: &str) -> Option<&str> {
    let start = after(html, "<script id=\"__NEXT_DATA__\" type=\"application/json\">")?;
    start.split("</script>").next()
}

fn after<'a>(s: &'a str, marker: &str) -> Option<&'a str> {
    s.find(marker).map(|i| &s[i + marker.len()..])
}

/// The text up to the next tag, trimmed.
fn text(s: &str) -> Option<&str> {
    s.split('<').next().map(str::trim).filter(|t| !t.is_empty())
}

fn words(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric() && c != '\'')
        .map(|w| w.trim_matches('\''))
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect()
}

fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// The HTML entities Goodreads' search page uses.
fn decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(end) = rest.find(';').filter(|e| *e < 12) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..end];
        let replacement = match entity {
            "amp" => Some('&'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "mdash" => Some('—'),
            "ndash" => Some('–'),
            "nbsp" => Some(' '),
            _ => entity
                .strip_prefix('#')
                .and_then(|n| n.strip_prefix('x').map(|h| u32::from_str_radix(h, 16).ok()).unwrap_or_else(|| n.parse().ok()))
                .and_then(char::from_u32),
        };
        match replacement {
            Some(c) => out.push(c),
            None => out.push_str(&rest[..=end]),
        }
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out
}

/// "YYYY-MM-DD" of a Unix time (UTC), by the civil-from-days algorithm.
fn date_of(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

/// ISO 639-2 codes for the language names Goodreads uses.
fn language_code(name: &str) -> Option<&'static str> {
    Some(match name.trim() {
        "English" => "eng",
        "French" => "fre",
        "German" => "ger",
        "Spanish" | "Spanish; Castilian" => "spa",
        "Italian" => "ita",
        "Portuguese" => "por",
        "Dutch" | "Dutch; Flemish" => "dut",
        "Russian" => "rus",
        "Japanese" => "jpn",
        "Chinese" => "chi",
        "Polish" => "pol",
        "Swedish" => "swe",
        "Danish" => "dan",
        "Norwegian" => "nor",
        "Finnish" => "fin",
        "Czech" => "cze",
        "Hungarian" => "hun",
        "Greek" | "Greek, Modern" => "gre",
        "Turkish" => "tur",
        "Arabic" => "ara",
        "Hebrew" => "heb",
        "Korean" => "kor",
        "Indonesian" => "ind",
        "Romanian" => "rum",
        "Ukrainian" => "ukr",
        "Catalan" => "cat",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEARCH: &str = include_str!("../tests/fixtures/search-project-hail-mary.html");
    const PHM: &str = include_str!("../tests/fixtures/book-project-hail-mary.html");
    const DUNE: &str = include_str!("../tests/fixtures/book-dune.html");

    #[test]
    fn queries() {
        assert_eq!(query("Project Hail Mary: A Novel", Some("Andy Weir")), "Project+Hail+Mary+Andy+Weir");
        assert_eq!(query("L'Étranger", None), "L%27%C3%89tranger");
        assert_eq!(clean_isbn("978-0-593-13520-4"), "9780593135204");
        assert_eq!(clean_isbn("059313520x"), "059313520X");
        assert_eq!(clean_isbn("12345"), "");
    }

    #[test]
    fn search_rows_are_read_and_ranked() {
        let rows = search_rows(SEARCH);
        assert_eq!(rows.len(), 17);
        let real = rows.iter().find(|r| r.id == "54493401").unwrap();
        assert_eq!((real.title.as_str(), real.authors.as_slice(), real.ratings), ("Project Hail Mary", &["Andy Weir".to_owned()][..], 1_898_143));
        assert!(rows[0].title.starts_with("SUMMARY AND ANALYSIS"));
        let ranked = rank(rows.clone(), "Project Hail Mary", &["Andy Weir".into()]);
        assert_eq!(ranked[0].id, "54493401", "the book itself beats the summaries of it");
        // Without an author the real book still wins on title length and popularity.
        assert_eq!(rank(rows.clone(), "Project Hail Mary", &[])[0].id, "54493401");
        // A title that shares too few words is out.
        assert!(rank(rows, "The Martian", &["Andy Weir".into()]).is_empty());
        assert!(!is_book_page(SEARCH));
    }

    #[test]
    fn a_book_page_becomes_a_record() {
        assert!(is_book_page(PHM));
        let r = book(PHM, "Project Hail Mary").unwrap();
        assert_eq!((r.title.as_str(), r.authors.as_slice()), ("Project Hail Mary", &["Andy Weir".to_owned()][..]));
        assert_eq!(r.source_key, "54493401");
        assert_eq!(r.link.as_deref(), Some("https://www.goodreads.com/book/show/54493401-project-hail-mary"));
        assert_eq!(r.publisher.as_deref(), Some("Ballantine Books"));
        assert_eq!(r.published.as_deref(), Some("2021-05-04"));
        assert_eq!(r.pages, Some(476));
        assert_eq!(r.languages, ["eng"]);
        let ids: Vec<(&str, &str)> = r.identifiers.iter().map(|i| (i.key.as_str(), i.value.as_str())).collect();
        assert_eq!(ids, [("goodreads", "54493401"), ("isbn", "9780593135204")]);
        assert_eq!(r.tags[..2], ["Science Fiction", "Fiction"]);
        assert!(r.description_html.as_deref().unwrap().starts_with("Ryland Grace is the sole survivor"));
        assert!(r.cover_url.as_deref().unwrap().ends_with("/54493401.jpg"));
        assert_eq!((r.series, r.series_index), (None, None));
    }

    #[test]
    fn dune_has_its_series_and_asin() {
        let r = book(DUNE, "Dune").unwrap();
        assert_eq!((r.title.as_str(), r.authors.as_slice()), ("Dune", &["Frank Herbert".to_owned()][..]));
        assert!(r.identifiers.iter().any(|i| i.key == "amazon" && i.value == "059309932X"));
        assert_eq!((r.series.as_deref(), r.series_index), (Some("Dune"), Some(1.0)));
    }

    #[test]
    fn helpers() {
        assert_eq!(decode("Tom &amp; Jerry &#39;s &#x27;day&#x27; &mdash; ok &unknown; &"), "Tom & Jerry 's 'day' — ok &unknown; &");
        assert_eq!(date_of(1_620_111_600), "2021-05-04");
        assert_eq!(date_of(0), "1970-01-01");
        assert_eq!(date_of(-86_400), "1969-12-31");
        assert_eq!(language_code("English"), Some("eng"));
    }
}
