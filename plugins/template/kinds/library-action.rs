//! A library action: menu items in the book menu (under Plugins) and the
//! toolbar's Plugins button, run on the selected books. Reads the library
//! freely; editing needs capabilities = ["write-library"], book files
//! "read-files". manifest.toml: kind = "library-action".

use magnitide_plugin_sdk::prelude::*;

struct MyAction;

impl LibraryAction for MyAction {
    fn menu() -> Vec<MenuItem> {
        vec![MenuItem { id: "tag".into(), text: "Tag as Example".into(), needs_selection: true }]
    }

    /// Returns a line shown when done; an error is shown as one. Status
    /// lines and progress show in the job bar meanwhile.
    fn run(item: String, selection: Vec<u64>) -> Result<String, String> {
        match item.as_str() {
            "tag" => {
                let books = library::books(&selection);
                let total = books.len() as u32;
                for (i, book) in books.iter().enumerate() {
                    ui::status(&format!("Tagging {}…", book.title));
                    ui::progress(i as u32, total);
                    let mut tags = book.tags.clone();
                    if !tags.contains(&"Example".to_string()) {
                        tags.push("Example".into());
                    }
                    library::edit(book.id, &BookEdit { tags: Some(tags), ..unchanged() })?;
                }
                Ok(format!("Tagged {total} books"))
            }
            other => Err(format!("unknown menu item {other}")),
        }
    }
}

/// An edit that changes nothing; set the fields to change.
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

magnitide_plugin_sdk::export_library_action!(MyAction);
