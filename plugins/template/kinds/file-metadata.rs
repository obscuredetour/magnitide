//! A file-metadata plugin: reads and writes metadata in book formats.
//! Reading sees the file at `/in/<name>`; writing gets a copy at
//! `/out/<name>` that Magnitide moves over the original when the plugin
//! succeeds. manifest.toml: kind = "file-metadata".

use magnitide_plugin_sdk::prelude::*;

struct MyFormat;

impl FileMetadata for MyFormat {
    /// Upper-case formats, each with whether the plugin's reader is used
    /// before Magnitide's own (`Override`) or only for formats Magnitide
    /// can't read (`Fallback`).
    fn reads() -> Vec<ReadsFormat> {
        vec![ReadsFormat { format: "CBZ".into(), precedence: Precedence::Fallback }]
    }

    fn read(file: String, format: String) -> Result<BookMetadata, String> {
        log::info(format!("reading {file} ({format})"));
        let _bytes = std::fs::read(&file).map_err(|e| format!("couldn't read {file}: {e}"))?;
        Ok(BookMetadata {
            title: None,
            authors: vec![],
            series: None,
            series_index: None,
            publisher: None,
            published: None,
            languages: vec![],
            identifiers: vec![],
            description: None,
            tags: vec![],
            cover: None,
            uuids: vec![],
        })
    }

    /// Upper-case formats the plugin writes into. Empty for a reader only.
    fn writes() -> Vec<String> {
        vec![]
    }

    fn write(file: String, format: String, _meta: BookMetadata) -> Result<(), String> {
        Err(format!("writing {format} isn't supported ({file})"))
    }
}

magnitide_plugin_sdk::export_file_metadata!(MyFormat);
