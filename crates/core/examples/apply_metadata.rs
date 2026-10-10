//! Set books' metadata from a JSON file, matching them by title (as the
//! landing site's demo library takes its tags, series and shelves from the
//! site's mock):
//!
//!     cargo run --release -p magnitide-core --example apply_metadata -- <library> <books.json>
//!
//! The file is an array of `{ title, authors, series?, seriesIndex?,
//! published? (a year), tags?, stars? (0–5), shelves? }`, `authors` joined by
//! " & ". Titles match ignoring case, punctuation, a leading "The" and
//! anything after a semicolon ("Frankenstein; or, The Modern Prometheus" is
//! "Frankenstein"). Each field given replaces the book's; shelves are made as
//! needed. What's not given (description, languages, identifiers) is kept.

use std::collections::BTreeMap;

use magnitide_core::Library;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    title: String,
    authors: String,
    series: Option<String>,
    series_index: Option<f64>,
    published: Option<i32>,
    tags: Option<Vec<String>>,
    stars: Option<u8>,
    #[serde(default)]
    shelves: Vec<String>,
}

/// "The Adventures of Huckleberry Finn" and "Adventures of Huckleberry Finn" alike.
fn key(title: &str) -> String {
    let main = title.split(';').next().unwrap_or(title).to_lowercase().replace(['-', '—'], " ");
    let words: String = main.chars().filter(|c| c.is_alphanumeric() || c.is_whitespace()).collect();
    let words: Vec<&str> = words.split_whitespace().collect();
    words.strip_prefix(&["the"]).unwrap_or(&words).join(" ")
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [root, json] = args.as_slice() else {
        eprintln!("usage: apply_metadata <library> <books.json>");
        std::process::exit(2);
    };
    let entries: Vec<Entry> = serde_json::from_str(&std::fs::read_to_string(json).unwrap_or_else(|e| {
        eprintln!("error: {json}: {e}");
        std::process::exit(1);
    }))
    .unwrap_or_else(|e| {
        eprintln!("error: {json}: {e}");
        std::process::exit(1);
    });
    let run = || -> magnitide_core::Result<(usize, Vec<String>)> {
        let mut lib = Library::open(root)?;
        let by_key: BTreeMap<String, _> =
            lib.load_catalog()?.books.into_iter().map(|b| (key(&b.title), b.id)).collect();
        let mut shelves: BTreeMap<String, _> = lib.shelves()?.into_iter().map(|s| (s.name, s.id)).collect();
        let (mut done, mut missing) = (0, Vec::new());
        for e in &entries {
            let Some(&id) = by_key.get(&key(&e.title)) else {
                missing.push(e.title.clone());
                continue;
            };
            let mut edit = lib.book_edit(id)?;
            edit.title = e.title.clone();
            edit.title_sort = String::new();
            edit.authors = e.authors.split(" & ").map(str::to_owned).collect();
            if let Some(s) = &e.series {
                edit.series = Some(s.clone());
                edit.series_index = e.series_index.unwrap_or(1.0);
            }
            if let Some(year) = e.published {
                edit.published = Some(format!("{year:04}-01-01"));
            }
            if let Some(tags) = &e.tags {
                edit.tags = tags.clone();
            }
            if let Some(stars) = e.stars {
                edit.rating = (stars > 0).then_some(stars.min(5) * 2);
            }
            lib.update_book(id, &edit)?;
            for name in &e.shelves {
                let shelf = match shelves.get(name) {
                    Some(&s) => s,
                    None => {
                        let s = lib.create_shelf(name)?.id;
                        shelves.insert(name.clone(), s);
                        s
                    }
                };
                lib.add_to_shelf(shelf, &[id])?;
            }
            done += 1;
        }
        Ok((done, missing))
    };
    match run() {
        Ok((done, missing)) => {
            println!("updated {done} books");
            for m in missing {
                println!("  no book titled {m:?}");
            }
        }
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}
