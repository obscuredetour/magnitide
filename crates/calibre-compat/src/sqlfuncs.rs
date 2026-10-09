// Derived from calibre's src/calibre/ebooks/metadata/__init__.py (title_sort) (calibre 9.15).
// Copyright: 2008, Kovid Goyal <kovid@kovidgoyal.net>. License: GPLv3.
// Ported to Rust for Magnitide, 2026.

//! SQL functions calibre registers on every connection.
//!
//! calibre's schema has triggers that call application-defined functions, so
//! a plain SQLite client cannot insert or rename books:
//!
//! ```text
//! sqlite> INSERT INTO books(title) VALUES ('x');
//! Error: in prepare, no such function: title_sort
//! ```
//!
//! Any writer must register at least the functions the triggers use.
//! calibre also registers `books_list_filter`, `concat`/`sortconcat`
//! (aggregates, used by views), `author_to_author_sort` and others; those are
//! only needed for the views and are not implemented here.

use rusqlite::Connection;
use rusqlite::functions::FunctionFlags;

/// Register the functions needed by calibre's triggers (`title_sort`, `uuid4`).
pub fn register(conn: &Connection) -> rusqlite::Result<()> {
    conn.create_scalar_function(
        "title_sort",
        1,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        |ctx| Ok(ctx.get::<Option<String>>(0)?.map(|t| title_sort(&t))),
    )?;
    conn.create_scalar_function("uuid4", 0, FunctionFlags::SQLITE_UTF8, |_| Ok(uuid::Uuid::new_v4().to_string()))?;
    Ok(())
}

/// Port of `calibre.ebooks.metadata.title_sort` with calibre's default
/// English articles. calibre also supports per-language article lists
/// (tweak `per_language_title_sort_articles`); not ported yet.
pub fn title_sort(title: &str) -> String {
    // calibre's `_ignore_starts`: ASCII quotes, U+2018..=U+201D, U+2032, U+2033.
    fn ignorable(c: char) -> bool {
        matches!(c, '\'' | '"' | '\u{2018}'..='\u{201d}' | '\u{2032}' | '\u{2033}')
    }
    fn strip_one(s: &str) -> &str {
        match s.chars().next() {
            Some(c) if ignorable(c) => &s[c.len_utf8()..],
            _ => s,
        }
    }

    let title = strip_one(title.trim());
    for article in ["A", "The", "An"] {
        let n = article.len();
        if title.len() > n
            && title.is_char_boundary(n)
            && title[..n].eq_ignore_ascii_case(article)
            && title[n..].starts_with(char::is_whitespace)
        {
            return format!("{}, {}", &title[n..], &title[..n]).trim().to_owned();
        }
    }
    title.trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_leading_articles() {
        assert_eq!(title_sort("The Hobbit"), "Hobbit, The");
        assert_eq!(title_sort("a Wizard of Earthsea"), "Wizard of Earthsea, a");
        assert_eq!(title_sort("An Unkindness of Ghosts"), "Unkindness of Ghosts, An");
        assert_eq!(title_sort("Theory of Everything"), "Theory of Everything");
        assert_eq!(title_sort("“The Quoted”"), "Quoted”, The");
        assert_eq!(title_sort("The “Inner”"), "“Inner”, The");
        assert_eq!(title_sort("  Dune "), "Dune");
    }

    #[test]
    fn triggers_work_once_registered() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("../schema/metadata_sqlite.sql")).unwrap();
        assert!(conn.execute("INSERT INTO books(title) VALUES ('The Dispossessed')", []).is_err());

        register(&conn).unwrap();
        conn.execute("INSERT INTO books(title) VALUES ('The Dispossessed')", []).unwrap();
        let (sort, uuid): (String, String) =
            conn.query_row("SELECT sort, uuid FROM books", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!(sort, "Dispossessed, The");
        assert_eq!(uuid.len(), 36);
    }
}
