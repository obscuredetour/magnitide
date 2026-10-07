//! Open Library (openlibrary.org): free, no account. The search API returns
//! the work plus its best-matching edition (the one with the ISBN, when
//! searching by ISBN); the description lives on the work, fetched on demand.

use serde_json::Value;

use magnitide_catalog::languages::language_code;
use magnitide_catalog::matching::isbn13;

use crate::text::{iso_date, tags, text_to_html};
use crate::{Error, Lookup, Result, SourceId, SourceRecord, http};

const SEARCH: &str = "https://openlibrary.org/search.json";
const FIELDS: &str = "key,title,subtitle,author_name,cover_i,first_publish_year,language,subject,\
editions,editions.key,editions.title,editions.subtitle,editions.publisher,editions.publish_date,\
editions.isbn,editions.cover_i,editions.language";

/// Open Library housekeeping subjects, not topics.
const NOT_TAGS: &[&str] = &[
    "accessible book",
    "protected daisy",
    "in library",
    "lending library",
    "large type books",
    "overdrive",
    "internet archive wishlist",
    "open library staff picks",
    "new york times bestseller",
    "new york times reviewed",
];

pub(crate) fn search(lookup: &Lookup) -> Result<Vec<SourceRecord>> {
    if let Some(isbn) = lookup.isbn.as_deref().and_then(isbn13) {
        let found = parse_search(&http::get_json(SEARCH, &[("isbn", &isbn), ("fields", FIELDS), ("limit", "5")])?);
        if !found.is_empty() || lookup.title.trim().is_empty() {
            return Ok(found);
        }
    }
    if lookup.title.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut query = vec![("title", lookup.title.trim()), ("fields", FIELDS), ("limit", "10")];
    if let Some(author) = lookup.authors.first().map(|a| a.trim()).filter(|a| !a.is_empty()) {
        query.push(("author", author));
    }
    Ok(parse_search(&http::get_json(SEARCH, &query)?))
}

fn parse_search(v: &Value) -> Vec<SourceRecord> {
    let strings = |v: &Value| -> Vec<String> {
        v.as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect()
    };
    let docs = v["docs"].as_array().map(Vec::as_slice).unwrap_or_default();
    docs.iter()
        .filter_map(|doc| {
            let work = doc["key"].as_str()?;
            let title = doc["title"].as_str()?.trim().to_owned();
            let edition = &doc["editions"]["docs"][0];
            let isbn = strings(&edition["isbn"]).iter().filter_map(|i| isbn13(i)).next();
            let cover = edition["cover_i"].as_i64().or_else(|| doc["cover_i"].as_i64()).filter(|id| *id > 0);
            // The work's language list spans translations; only trust it alone.
            let languages = match strings(&edition["language"]) {
                l if !l.is_empty() => l,
                _ => Some(strings(&doc["language"])).filter(|l| l.len() == 1).unwrap_or_default(),
            };
            let mut identifiers = std::collections::BTreeMap::new();
            if let Some(isbn) = isbn {
                identifiers.insert("isbn".to_owned(), isbn);
            }
            if let Some(key) = edition["key"].as_str().and_then(|k| k.strip_prefix("/books/")) {
                identifiers.insert("openlibrary".to_owned(), key.to_owned());
            }
            Some(SourceRecord {
                source: Some(SourceId::OpenLibrary),
                source_name: SourceId::OpenLibrary.name().to_owned(),
                source_key: work.to_owned(),
                link: Some(format!("https://openlibrary.org{}", edition["key"].as_str().unwrap_or(work))),
                subtitle: doc["subtitle"].as_str().map(str::to_owned),
                title,
                authors: strings(&doc["author_name"]),
                publisher: strings(&edition["publisher"]).into_iter().next(),
                published: strings(&edition["publish_date"])
                    .first()
                    .and_then(|d| iso_date(d))
                    .or_else(|| doc["first_publish_year"].as_i64().and_then(|y| iso_date(&y.to_string()))),
                languages: languages.iter().map(|l| language_code(l)).collect(),
                identifiers,
                tags: tags(
                    strings(&doc["subject"]).into_iter().filter(|s| !NOT_TAGS.contains(&s.to_lowercase().as_str())),
                    8,
                ),
                description_html: None,
                cover_url: cover.map(|id| format!("https://covers.openlibrary.org/b/id/{id}-L.jpg?default=false")),
                pages: None,
                has_details: true,
            })
        })
        .collect()
}

/// The work's description.
pub(crate) fn details(record: &SourceRecord) -> Result<SourceRecord> {
    // The key came back from the UI: only ever a work path.
    let key = &record.source_key;
    let id = key.strip_prefix("/works/OL").and_then(|k| k.strip_suffix('W'));
    if !id.is_some_and(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit())) {
        return Err(Error::new("not an Open Library work"));
    }
    let work = http::get_json(&format!("https://openlibrary.org{key}.json"), &[])?;
    Ok(SourceRecord { description_html: parse_description(&work), has_details: false, ..record.clone() })
}

/// A string, or `{"type": "/type/text", "value": "…"}`.
fn parse_description(work: &Value) -> Option<String> {
    let d = &work["description"];
    text_to_html(d.as_str().or_else(|| d["value"].as_str())?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Value {
        let path = format!("{}/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn parses_an_isbn_search_with_its_edition() {
        let records = parse_search(&fixture("openlibrary-isbn.json"));
        assert_eq!(records.len(), 1);
        let r = &records[0];
        assert_eq!(r.title, "The Dispossessed");
        assert_eq!(r.subtitle.as_deref(), Some("An Ambiguous Utopia"));
        assert_eq!(r.authors, ["Ursula K. Le Guin"]);
        // From the edition with that ISBN, not the work's many editions.
        assert_eq!(r.publisher.as_deref(), Some("HarperPrism"));
        assert_eq!(r.published.as_deref(), Some("1994-01-01"));
        assert_eq!(r.languages, ["eng"]);
        assert_eq!(r.identifiers.get("isbn").map(String::as_str), Some("9780061054884"));
        assert_eq!(r.identifiers.get("openlibrary").map(String::as_str), Some("OL18092578M"));
        assert_eq!(r.cover_url.as_deref(), Some("https://covers.openlibrary.org/b/id/14614926-L.jpg?default=false"));
        assert_eq!(r.source_key, "/works/OL59863W");
        assert!(r.tags.contains(&"Anarchism".to_owned()), "{:?}", r.tags);
        assert!(r.has_details);
    }

    #[test]
    fn parses_a_title_search() {
        let records = parse_search(&fixture("openlibrary-search.json"));
        assert_eq!(records.len(), 3);
        assert_eq!(records[0].title, "The Subtle Knife");
        assert_eq!(records[0].authors, ["Philip Pullman"]);
        assert!(records.iter().all(|r| r.tags.iter().all(|t| t != "Accessible book")));
    }

    #[test]
    fn parses_work_descriptions() {
        let html = parse_description(&fixture("openlibrary-work.json")).unwrap();
        assert!(html.starts_with("<p>Shevek, a brilliant physicist"), "{html}");
        let typed = serde_json::json!({"description": {"type": "/type/text", "value": "One.\n\nTwo."}});
        assert_eq!(parse_description(&typed).as_deref(), Some("<p>One.</p><p>Two.</p>"));
    }

    #[test]
    fn details_only_fetch_work_keys() {
        let r = SourceRecord { source_key: "/../evil".into(), has_details: true, ..Default::default() };
        assert!(details(&r).is_err());
    }

    #[test]
    #[ignore = "uses the network"]
    fn live_search() {
        let lookup = Lookup { title: "The Dispossessed".into(), authors: vec!["Le Guin".into()], isbn: None };
        let records = search(&lookup).unwrap();
        assert!(records.iter().any(|r| r.title == "The Dispossessed"), "{records:#?}");
        let full = details(&records[0]).unwrap();
        assert!(full.description_html.is_some());
        let cover = crate::download_cover(records[0].cover_url.as_deref().unwrap()).unwrap();
        assert!(cover.starts_with(b"\xFF\xD8"), "a JPEG");
    }
}
