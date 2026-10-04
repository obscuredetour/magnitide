use std::collections::HashMap;

use serde::Serialize;

use crate::model::{Book, BookId, CustomColumn};

/// Every book in a library, loaded into memory. Search, sort and every view
/// work on this, whatever kind of library it came from.
#[derive(Debug, Serialize)]
pub struct Catalog {
    /// The source library's own id, when it has one.
    pub library_uuid: Option<String>,
    pub custom_columns: Vec<CustomColumn>,
    pub books: Vec<Book>,
    /// Non-fatal problems hit while loading (e.g. a missing custom column table).
    pub warnings: Vec<String>,
    #[serde(skip)]
    index: HashMap<BookId, usize>,
}

impl Catalog {
    pub fn new(
        library_uuid: Option<String>,
        custom_columns: Vec<CustomColumn>,
        books: Vec<Book>,
        warnings: Vec<String>,
    ) -> Self {
        let index = books.iter().enumerate().map(|(i, b)| (b.id, i)).collect();
        Self { library_uuid, custom_columns, books, warnings, index }
    }

    pub fn get(&self, id: BookId) -> Option<&Book> {
        self.index.get(&id).map(|&i| &self.books[i])
    }
}
