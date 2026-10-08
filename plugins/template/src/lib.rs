//! A metadata source: asked in Find Metadata beside Magnitide's own.
//! manifest.toml: kind = "metadata-source", capabilities = ["network"].

use magnitide_plugin_sdk::prelude::*;

struct MySource;

impl MetadataSource for MySource {
    fn info() -> SourceInfo {
        SourceInfo {
            name: "My Plugin".into(),
            description: "Looks books up somewhere".into(),
            version: env!("CARGO_PKG_VERSION").into(),
        }
    }

    /// Records for the lookup, best first; an empty list when nothing
    /// matched; an error only when the source couldn't be asked.
    fn identify(lookup: Lookup) -> Result<Vec<BookRecord>, String> {
        log::info(format!("looking up {:?} by {:?}", lookup.title, lookup.authors));
        // let body = http::get_with(&url, &[("Accept", "application/json")])?;
        // let parsed: MyReply = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
        Ok(vec![BookRecord {
            source_key: "example-1".into(),
            link: None,
            title: lookup.title.clone(),
            subtitle: None,
            authors: lookup.authors.clone(),
            publisher: None,
            published: None,
            languages: vec![],
            identifiers: lookup.isbn.iter().map(|i| Identifier { key: "isbn".into(), value: i.clone() }).collect(),
            tags: vec![],
            description_html: None,
            cover_url: None,
            pages: None,
        }])
    }
}

magnitide_plugin_sdk::export_metadata_source!(MySource);
