//! Open Library as a Magnitide metadata source. The same lookup Magnitide's
//! built-in source makes, written against the plugin API: one HTTP request
//! through the host, a JSON reply, records back.

use magnitide_plugin_sdk::prelude::*;
use serde::Deserialize;

struct OpenLibrary;

#[derive(Deserialize)]
struct Search {
    #[serde(default)]
    docs: Vec<Doc>,
}

#[derive(Deserialize, Default)]
struct Doc {
    key: String,
    title: Option<String>,
    subtitle: Option<String>,
    #[serde(default)]
    author_name: Vec<String>,
    #[serde(default)]
    publisher: Vec<String>,
    first_publish_year: Option<u32>,
    #[serde(default)]
    language: Vec<String>,
    #[serde(default)]
    isbn: Vec<String>,
    #[serde(default)]
    subject: Vec<String>,
    cover_i: Option<i64>,
    number_of_pages_median: Option<u32>,
}

impl MetadataSource for OpenLibrary {
    fn info() -> SourceInfo {
        SourceInfo {
            name: "Open Library (plugin)".into(),
            description: "Looks books up on openlibrary.org".into(),
            version: env!("CARGO_PKG_VERSION").into(),
        }
    }

    fn identify(lookup: Lookup) -> Result<Vec<BookRecord>, String> {
        let mut query = Vec::new();
        if let Some(isbn) = lookup.isbn.as_deref().filter(|s| !s.is_empty()) {
            query.push(format!("isbn={}", urlencode(isbn)));
        } else {
            if !lookup.title.is_empty() {
                query.push(format!("title={}", urlencode(&lookup.title)));
            }
            if let Some(author) = lookup.authors.first() {
                query.push(format!("author={}", urlencode(author)));
            }
        }
        if query.is_empty() {
            return Ok(vec![]);
        }
        let url = format!(
            "https://openlibrary.org/search.json?{}&limit=10&fields=key,title,subtitle,author_name,publisher,first_publish_year,language,isbn,subject,cover_i,number_of_pages_median",
            query.join("&")
        );
        log::info(format!("GET {url}"));
        let body = http::get_with(&url, &[("Accept", "application/json")])?;
        let search: Search = serde_json::from_slice(&body).map_err(|e| format!("Open Library answered with something unexpected: {e}"))?;
        Ok(search.docs.into_iter().filter_map(record).collect())
    }
}

fn record(doc: Doc) -> Option<BookRecord> {
    let title = doc.title?;
    let key = doc.key.trim_start_matches("/works/").to_owned();
    let mut identifiers = vec![Identifier { key: "openlibrary".into(), value: key.clone() }];
    if let Some(isbn13) = doc.isbn.iter().find(|i| i.len() == 13) {
        identifiers.push(Identifier { key: "isbn".into(), value: isbn13.clone() });
    }
    Some(BookRecord {
        source_key: key.clone(),
        link: Some(format!("https://openlibrary.org/works/{key}")),
        title,
        subtitle: doc.subtitle,
        authors: doc.author_name,
        publisher: doc.publisher.into_iter().next(),
        published: doc.first_publish_year.map(|y| y.to_string()),
        languages: doc.language.into_iter().take(3).collect(),
        identifiers,
        tags: doc.subject.into_iter().take(8).collect(),
        description_html: None,
        cover_url: doc.cover_i.filter(|i| *i > 0).map(|i| format!("https://covers.openlibrary.org/b/id/{i}-L.jpg")),
        pages: doc.number_of_pages_median,
    })
}

fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

magnitide_plugin_sdk::export_metadata_source!(OpenLibrary);
