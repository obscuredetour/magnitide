//! Goodreads as a Magnitide metadata source: a port of the calibre
//! Goodreads plugin's lookup. A title/author search on goodreads.com, the
//! likeliest results' pages, and the book data each page embeds for its
//! own scripts (`__NEXT_DATA__`). An ISBN goes straight to the book page.

// The component exports the source; a native build (`cargo test`) only checks the parser.
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

use magnitide_plugin_sdk::prelude::*;

pub mod parse;

/// Book pages fetched per lookup, best search results first. Few: Goodreads
/// starts answering with a browser check after a burst of requests.
const PAGES: usize = 2;
const BASE: &str = "https://www.goodreads.com";

struct Goodreads;

impl MetadataSource for Goodreads {
    fn info() -> SourceInfo {
        SourceInfo {
            name: "Goodreads".into(),
            description: "Looks books up on goodreads.com".into(),
            version: env!("CARGO_PKG_VERSION").into(),
        }
    }

    fn identify(lookup: Lookup) -> Result<Vec<BookRecord>, String> {
        let isbn = lookup.isbn.as_deref().map(parse::clean_isbn).filter(|i| !i.is_empty());
        let urls: Vec<String> = match isbn {
            Some(isbn) => {
                // Goodreads answers an ISBN with the book's page itself.
                let url = format!("{BASE}/search?search_type=books&search%5Bquery%5D={isbn}");
                log::info(format!("GET {url}"));
                let page = fetch(&url)?;
                if parse::is_book_page(&page) {
                    return Ok(parse::book(&page, &lookup.title).into_iter().collect());
                }
                best(&page, &lookup)
            }
            None => {
                let query = parse::query(&lookup.title, lookup.authors.first().map(String::as_str));
                if query.is_empty() {
                    return Ok(vec![]);
                }
                let url = format!("{BASE}/search?search_type=books&search%5Bquery%5D={query}");
                log::info(format!("GET {url}"));
                best(&fetch(&url)?, &lookup)
            }
        };
        let mut records = Vec::new();
        for url in urls {
            log::info(format!("GET {url}"));
            match fetch(&url) {
                Ok(page) => records.extend(parse::book(&page, &lookup.title)),
                Err(e) => log::warn(format!("{url}: {e}")),
            }
        }
        Ok(records)
    }
}

/// The likeliest results' pages, best first.
fn best(search_page: &str, lookup: &Lookup) -> Vec<String> {
    let rows = parse::search_rows(search_page);
    log::info(format!("{} results", rows.len()));
    parse::rank(rows, &lookup.title, &lookup.authors)
        .into_iter()
        .take(PAGES)
        .map(|row| format!("{BASE}/book/show/{}", row.id))
        .collect()
}

/// A page, or why Goodreads didn't give it: its bot protection answers a
/// burst of requests with a browser check (HTTP 202 and a stub page)
/// instead of the page, for a while.
fn fetch(url: &str) -> Result<String, String> {
    let req = http::Request { method: "GET".into(), url: url.into(), headers: vec![], body: None };
    let resp = http::fetch(&req)?;
    let body = String::from_utf8_lossy(&resp.body).into_owned();
    if resp.status == 202 || (resp.status == 200 && !body.contains("schema.org/Book") && !parse::is_book_page(&body)) {
        return Err(CHALLENGED.into());
    }
    if !(200..300).contains(&resp.status) {
        return Err(format!("HTTP {} from Goodreads", resp.status));
    }
    Ok(body)
}

const CHALLENGED: &str = "Goodreads is asking for a browser check instead of answering (it does that after many requests); try again in a few minutes.";

#[cfg(target_arch = "wasm32")]
magnitide_plugin_sdk::export_metadata_source!(Goodreads);
