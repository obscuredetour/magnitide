//! A library action for the tests: tags the selected books as read, counts
//! the words in their TXT files into a description, and misbehaves on
//! request.

use magnitide_plugin_sdk::prelude::*;

struct Actions;

impl LibraryAction for Actions {
    fn menu() -> Vec<MenuItem> {
        vec![
            MenuItem { id: "read".into(), text: "Mark as Read".into(), needs_selection: true },
            MenuItem { id: "count".into(), text: "Count Words".into(), needs_selection: true },
            MenuItem { id: "survey".into(), text: "Survey the Library".into(), needs_selection: false },
            MenuItem { id: "ask".into(), text: "Ask First".into(), needs_selection: false },
            MenuItem { id: "name".into(), text: "Ask a Name".into(), needs_selection: false },
            MenuItem { id: "fail".into(), text: "Fail".into(), needs_selection: false },
            MenuItem { id: "loop".into(), text: "Loop".into(), needs_selection: false },
        ]
    }

    fn run(item: String, selection: Vec<u64>) -> Result<String, String> {
        log::info(format!("run {item} on {selection:?}"));
        match item.as_str() {
            "read" => {
                let books = library::books(&selection);
                let total = books.len() as u32;
                for (i, b) in books.iter().enumerate() {
                    ui::status(&format!("Marking {}…", b.title));
                    ui::progress(i as u32, total);
                    let mut tags = b.tags.clone();
                    if !tags.iter().any(|t| t == "Read") {
                        tags.push("Read".into());
                    }
                    let fields = library::fields();
                    let custom = if fields.iter().any(|f| f.key == "read") {
                        vec![CustomEdit { key: "read".into(), value: Some(FieldValue::Boolean(true)) }]
                    } else {
                        vec![]
                    };
                    library::edit(
                        b.id,
                        &BookEdit {
                            title: None,
                            authors: None,
                            series: None,
                            publisher: None,
                            published: None,
                            rating: None,
                            tags: Some(tags),
                            languages: None,
                            identifiers: None,
                            description_html: None,
                            custom,
                        },
                    )?;
                }
                Ok(format!("Marked {} as read", plural(total)))
            }
            "count" => {
                let mut counted = 0;
                for b in library::books(&selection) {
                    if !b.formats.iter().any(|f| f == "TXT") {
                        continue;
                    }
                    let path = library::open_format(b.id, "TXT")?;
                    let text = std::fs::read_to_string(&path).map_err(|e| format!("{path}: {e}"))?;
                    let words = text.split_whitespace().count();
                    library::edit(b.id, &BookEdit { description_html: Some(format!("<p>{words} words</p>")), ..unchanged() })?;
                    // A fresh copy of the text, upper-cased, as a new format.
                    std::fs::write("/out/shout.txt", text.to_uppercase()).map_err(|e| e.to_string())?;
                    library::add_format(b.id, "SHOUT", "/out/shout.txt")?;
                    counted += 1;
                }
                Ok(format!("Counted words in {}", plural(counted)))
            }
            "survey" => {
                let all = library::all_books();
                let unread = library::search("tag:unread");
                Ok(format!("{} books, {} unread", all.len(), unread.len()))
            }
            "ask" => match ui::ask(&format!("Tag {} as read?", plural(selection.len() as u32)), &["Tag".into(), "Skip".into()]) {
                Some(0) => Ok("Tagged".into()),
                Some(_) => Ok("Skipped".into()),
                None => Ok("No answer".into()),
            },
            "name" => Ok(match ui::ask_text("What's your name?", "Your name") {
                Some(name) => format!("Hello, {name}"),
                None => "Hello, stranger".into(),
            }),
            "fail" => Err("This action failed on purpose.".into()),
            "loop" => loop {
                std::hint::black_box(0);
            },
            other => Err(format!("unknown item {other}")),
        }
    }
}

fn unchanged() -> BookEdit {
    BookEdit {
        title: None,
        authors: None,
        series: None,
        publisher: None,
        published: None,
        rating: None,
        tags: None,
        languages: None,
        identifiers: None,
        description_html: None,
        custom: vec![],
    }
}

fn plural(n: u32) -> String {
    if n == 1 { "1 book".into() } else { format!("{n} books") }
}

magnitide_plugin_sdk::export_library_action!(Actions);
