//! Google Books. Anonymous requests get no quota any more, so this only
//! runs with the user's own API key (Google Cloud console, Books API).

use std::collections::BTreeMap;

use serde_json::Value;

use magnitide_catalog::languages::language_code;
use magnitide_catalog::matching::isbn13;

use crate::text::{iso_date, tags};
use crate::{Error, Lookup, Result, SourceId, SourceRecord, http};

const VOLUMES: &str = "https://www.googleapis.com/books/v1/volumes";

pub(crate) fn search(lookup: &Lookup, key: &str) -> Result<Vec<SourceRecord>> {
    if key.trim().is_empty() {
        return Err(Error::new("needs an API key (Settings → Metadata Sources)"));
    }
    let ask = |q: &str| -> Result<Vec<SourceRecord>> {
        let params = [("q", q), ("maxResults", "10"), ("printType", "books"), ("key", key.trim())];
        Ok(parse_volumes(&http::get_json(VOLUMES, &params)?))
    };
    if let Some(isbn) = lookup.isbn.as_deref().and_then(isbn13) {
        let found = ask(&format!("isbn:{isbn}"))?;
        if !found.is_empty() || lookup.title.trim().is_empty() {
            return Ok(found);
        }
    }
    if lookup.title.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut q = format!("intitle:{}", lookup.title.trim());
    if let Some(author) = lookup.authors.first().map(|a| a.trim()).filter(|a| !a.is_empty()) {
        q.push_str(&format!(" inauthor:{author}"));
    }
    ask(&q)
}

fn parse_volumes(v: &Value) -> Vec<SourceRecord> {
    let strings = |v: &Value| -> Vec<String> {
        v.as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect()
    };
    let items = v["items"].as_array().map(Vec::as_slice).unwrap_or_default();
    items
        .iter()
        .filter_map(|item| {
            let id = item["id"].as_str()?;
            let info = &item["volumeInfo"];
            let title = info["title"].as_str()?.trim().to_owned();
            let mut identifiers = BTreeMap::new();
            let isbn = info["industryIdentifiers"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|i| i["type"].as_str().is_some_and(|t| t.starts_with("ISBN")))
                .filter_map(|i| isbn13(i["identifier"].as_str()?))
                .next();
            if let Some(isbn) = isbn {
                identifiers.insert("isbn".to_owned(), isbn);
            }
            identifiers.insert("google".to_owned(), id.to_owned());
            // "Fiction / Science Fiction / General" → Fiction, Science Fiction.
            let categories = strings(&info["categories"])
                .into_iter()
                .flat_map(|c| c.split(" / ").map(str::to_owned).collect::<Vec<_>>())
                .filter(|c| c != "General");
            Some(SourceRecord {
                source: Some(SourceId::Google),
                source_name: SourceId::Google.name().to_owned(),
                source_key: id.to_owned(),
                link: info["canonicalVolumeLink"].as_str().map(str::to_owned),
                title,
                subtitle: info["subtitle"].as_str().map(str::to_owned),
                authors: strings(&info["authors"]),
                publisher: info["publisher"].as_str().map(str::to_owned),
                published: info["publishedDate"].as_str().and_then(iso_date),
                languages: info["language"].as_str().map(language_code).into_iter().collect(),
                identifiers,
                tags: tags(categories, 8),
                description_html: info["description"].as_str().map(str::to_owned),
                cover_url: info["imageLinks"]["thumbnail"].as_str().and_then(cover_url),
                pages: info["pageCount"].as_u64().and_then(|p| u32::try_from(p).ok()).filter(|p| *p > 0),
                series: None,
                series_index: None,
                has_details: false,
                comic: None,
            })
        })
        .collect()
}

/// The thumbnail link, made HTTPS, without the page-curl effect, and asking
/// for a larger image.
fn cover_url(thumbnail: &str) -> Option<String> {
    let rest = thumbnail.strip_prefix("http://").or_else(|| thumbnail.strip_prefix("https://"))?;
    let (host, query) = rest.split_once("/books/content?")?;
    if host != "books.google.com" {
        return None;
    }
    let params: Vec<&str> = query.split('&').filter(|p| !p.starts_with("edge=") && !p.starts_with("zoom=")).collect();
    Some(format!("https://books.google.com/books/content?{}&zoom=1&fife=w800-h1200", params.join("&")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Value {
        let path = format!("{}/fixtures/google-volumes.json", env!("CARGO_MANIFEST_DIR"));
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn parses_volumes() {
        let records = parse_volumes(&fixture());
        assert_eq!(records.len(), 2);
        let r = &records[0];
        assert_eq!(r.title, "The Dispossessed");
        assert_eq!(r.authors, ["Ursula K. Le Guin"]);
        assert_eq!(r.published.as_deref(), Some("2009-10-13"));
        assert_eq!(r.languages, ["eng"]);
        assert_eq!(r.identifiers.get("isbn").map(String::as_str), Some("9780061054884"));
        assert_eq!(r.identifiers.get("google").map(String::as_str), Some("bQ3dDwAAQBAJ"));
        assert_eq!(r.tags, ["Fiction", "Science Fiction"]);
        assert_eq!(r.pages, Some(387));
        assert!(r.description_html.as_deref().unwrap().contains("<b>Winner"));
        let cover = r.cover_url.as_deref().unwrap();
        assert!(crate::is_cover_url(cover), "{cover}");
        assert!(!cover.contains("edge=curl") && cover.contains("fife=w800-h1200"), "{cover}");
        assert_eq!(records[1].published.as_deref(), Some("2015-01-01"));
    }

    #[test]
    fn refuses_to_run_without_a_key() {
        let err = search(&Lookup { title: "x".into(), ..Default::default() }, " ").unwrap_err();
        assert!(err.to_string().contains("API key"));
    }
}
