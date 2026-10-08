//! A file-metadata plugin for the tests: TXT files whose first lines are
//! `Title: …`, `Author: …`, `Tag: …`, `Series: …`, `Published: …`. Reads
//! them (overriding Magnitide's reader for TXT, a fallback for MGTEST) and
//! writes them back (TXT only).

use magnitide_plugin_sdk::prelude::*;

struct Headers;

fn split(text: &str) -> (Vec<(&str, &str)>, &str) {
    let mut headers = Vec::new();
    let mut rest = text;
    for line in text.lines() {
        match line.split_once(": ") {
            Some((k, v)) if !k.contains(' ') => headers.push((k, v.trim())),
            _ => break,
        }
        rest = &text[text.find(line).unwrap_or(0) + line.len()..];
    }
    (headers, rest.trim_start_matches('\n'))
}

impl FileMetadata for Headers {
    fn reads() -> Vec<ReadsFormat> {
        vec![
            ReadsFormat { format: "txt".into(), precedence: Precedence::Override },
            ReadsFormat { format: "MGTEST".into(), precedence: Precedence::Fallback },
        ]
    }

    fn read(file: String, format: String) -> Result<BookMetadata, String> {
        log::info(format!("read {file} ({format})"));
        let text = std::fs::read_to_string(&file).map_err(|e| format!("couldn't read {file}: {e}"))?;
        if text.trim() == "FAIL" {
            return Err("This reader failed on purpose.".into());
        }
        let (headers, body) = split(&text);
        let get = |k: &str| headers.iter().filter(|(key, _)| *key == k).map(|(_, v)| v.to_string()).collect::<Vec<_>>();
        Ok(BookMetadata {
            title: get("Title").into_iter().next(),
            authors: get("Author").into_iter().map(|name| Author { name, sort: None }).collect(),
            series: get("Series").into_iter().next(),
            series_index: get("Index").into_iter().next().and_then(|i| i.parse().ok()),
            publisher: None,
            published: get("Published").into_iter().next(),
            languages: vec!["eng".into()],
            identifiers: get("ISBN").into_iter().map(|value| Identifier { key: "isbn".into(), value }).collect(),
            description: (!body.trim().is_empty()).then(|| body.trim().to_owned()),
            tags: get("Tag"),
            cover: None,
            uuids: vec![],
        })
    }

    fn writes() -> Vec<String> {
        vec!["TXT".into()]
    }

    fn write(file: String, format: String, meta: BookMetadata) -> Result<(), String> {
        log::info(format!("write {file} ({format})"));
        let text = std::fs::read_to_string(&file).map_err(|e| format!("couldn't read {file}: {e}"))?;
        let (_, body) = split(&text);
        let mut out = String::new();
        if let Some(t) = &meta.title {
            out.push_str(&format!("Title: {t}\n"));
        }
        for a in &meta.authors {
            out.push_str(&format!("Author: {}\n", a.name));
        }
        if let Some(s) = &meta.series {
            out.push_str(&format!("Series: {s}\nIndex: {}\n", meta.series_index.unwrap_or(1.0)));
        }
        if let Some(p) = &meta.published {
            out.push_str(&format!("Published: {p}\n"));
        }
        for i in meta.identifiers.iter().filter(|i| i.key == "isbn") {
            out.push_str(&format!("ISBN: {}\n", i.value));
        }
        for t in &meta.tags {
            out.push_str(&format!("Tag: {t}\n"));
        }
        out.push('\n');
        out.push_str(body);
        std::fs::write(&file, out).map_err(|e| format!("couldn't write {file}: {e}"))
    }
}

magnitide_plugin_sdk::export_file_metadata!(Headers);
