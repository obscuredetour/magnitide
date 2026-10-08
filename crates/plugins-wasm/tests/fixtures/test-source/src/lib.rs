use magnitide_plugin_sdk::prelude::*;

struct TestSource;

impl MetadataSource for TestSource {
    fn info() -> SourceInfo {
        SourceInfo { name: "Magnitide Test Source".into(), description: "Answers from memory".into(), version: "1.2.3".into() }
    }

    fn identify(lookup: Lookup) -> Result<Vec<BookRecord>, String> {
        log::info(format!("identify {:?} by {:?}", lookup.title, lookup.authors));
        match lookup.title.as_str() {
            "Fail" => return Err("This source failed on purpose.".into()),
            "Panic" => panic!("This source panicked on purpose."),
            "Loop" => loop {
                std::hint::black_box(0);
            },
            "Fetch" => {
                // The network: refused without the capability; the test checks the message.
                let body = http::get("https://example.invalid/book")?;
                return Ok(vec![BookRecord { title: String::from_utf8_lossy(&body).into_owned(), ..empty("fetched") }]);
            }
            _ => {}
        }
        Ok(vec![BookRecord {
            source_key: "42".into(),
            link: Some("https://example.invalid/book/42".into()),
            title: "Project Hail Mary".into(),
            subtitle: None,
            authors: vec!["Andy Weir".into()],
            publisher: Some("Ballantine Books".into()),
            published: Some("2021-05-04".into()),
            languages: vec!["eng".into()],
            identifiers: vec![
                Identifier { key: "isbn".into(), value: "9780593135204".into() },
                Identifier { key: "test".into(), value: "42".into() },
            ],
            tags: vec!["Science Fiction".into(), "Space".into()],
            description_html: Some("A lone astronaut & a mission.".into()),
            cover_url: Some("https://example.invalid/cover/42.jpg".into()),
            pages: Some(496),
        }])
    }
}

fn empty(key: &str) -> BookRecord {
    BookRecord {
        source_key: key.into(),
        link: None,
        title: String::new(),
        subtitle: None,
        authors: vec![],
        publisher: None,
        published: None,
        languages: vec![],
        identifiers: vec![],
        tags: vec![],
        description_html: None,
        cover_url: None,
        pages: None,
    }
}

magnitide_plugin_sdk::export_metadata_source!(TestSource);
