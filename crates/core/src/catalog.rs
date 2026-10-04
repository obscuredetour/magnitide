//! Loading a native library into the shared in-memory [`Catalog`], so the
//! same search, sort and views work for Magnitide and calibre libraries.

use std::collections::{BTreeMap, HashMap};

use chrono::{DateTime, NaiveDate, Utc};
use magnitide_catalog::{Book, BookId, Catalog, CustomColumn, CustomValue, Datatype, Format};
use rusqlite::{Connection, Row};

use crate::error::Result;
use crate::library::{LOCAL_USER, Library};

impl Library {
    /// Every book with its metadata and the local user's rating. One query
    /// per table, not per book, so 50k books load in a fraction of a second.
    pub fn load_catalog(&self) -> Result<Catalog> {
        let conn = &self.conn;
        let mut warnings = Vec::new();

        let mut books = Vec::new();
        each_row(
            conn,
            "SELECT b.id, b.uuid, b.title, b.title_sort, s.name, b.series_index, p.name, b.published,
                    b.description_html, b.path, b.cover_file, b.added_at, b.modified_at, ub.rating
             FROM books b
             LEFT JOIN series s ON s.id = b.series_id
             LEFT JOIN publishers p ON p.id = b.publisher_id
             LEFT JOIN user_books ub ON ub.book_id = b.id AND ub.user_id = ?1
             ORDER BY b.id",
            [LOCAL_USER],
            |r| {
                books.push(Book {
                    id: r.get(0)?,
                    uuid: r.get(1)?,
                    title: r.get(2)?,
                    sort: r.get(3)?,
                    series: r.get(4)?,
                    series_index: r.get::<_, Option<f64>>(5)?.unwrap_or(1.0),
                    publisher: r.get(6)?,
                    pubdate: r.get::<_, Option<String>>(7)?.as_deref().and_then(date),
                    comments: r.get(8)?,
                    path: r.get(9)?,
                    has_cover: r.get::<_, Option<String>>(10)?.is_some(),
                    timestamp: r.get::<_, String>(11)?.parse().ok(),
                    last_modified: r.get::<_, String>(12)?.parse().ok(),
                    rating: r.get(13)?,
                    authors: Vec::new(),
                    author_sort: None,
                    tags: Vec::new(),
                    languages: Vec::new(),
                    identifiers: BTreeMap::new(),
                    formats: Vec::new(),
                    custom: BTreeMap::new(),
                });
                Ok(())
            },
        )?;
        let index: HashMap<BookId, usize> = books.iter().enumerate().map(|(i, b)| (b.id, i)).collect();
        let with = |id: BookId| index.get(&id).copied();

        let mut author_sorts: HashMap<usize, Vec<String>> = HashMap::new();
        each_row(
            conn,
            "SELECT ba.book_id, a.name, a.sort_name FROM book_authors ba JOIN authors a ON a.id = ba.author_id
             ORDER BY ba.book_id, ba.position",
            [],
            |r| {
                if let Some(i) = with(r.get(0)?) {
                    books[i].authors.push(r.get(1)?);
                    author_sorts.entry(i).or_default().push(r.get(2)?);
                }
                Ok(())
            },
        )?;
        // calibre-style combined sort: "Gaiman, Neil & Pratchett, Terry".
        for (i, sorts) in author_sorts {
            books[i].author_sort = Some(sorts.join(" & "));
        }
        each_row(
            conn,
            "SELECT bt.book_id, t.name FROM book_tags bt JOIN tags t ON t.id = bt.tag_id ORDER BY t.name",
            [],
            |r| {
                if let Some(i) = with(r.get(0)?) {
                    books[i].tags.push(r.get(1)?);
                }
                Ok(())
            },
        )?;
        each_row(conn, "SELECT book_id, lang FROM book_languages ORDER BY book_id, position", [], |r| {
            if let Some(i) = with(r.get(0)?) {
                books[i].languages.push(r.get(1)?);
            }
            Ok(())
        })?;
        each_row(conn, "SELECT book_id, scheme, value FROM identifiers", [], |r| {
            if let Some(i) = with(r.get(0)?) {
                books[i].identifiers.insert(r.get(1)?, r.get(2)?);
            }
            Ok(())
        })?;
        each_row(conn, "SELECT book_id, format, filename, size FROM files ORDER BY id", [], |r| {
            if let Some(i) = with(r.get(0)?) {
                let format: String = r.get(1)?;
                let filename: String = r.get(2)?;
                // The catalog locates files as "<name>.<format, lower case>", which is
                // how this library names them (see paths::book_file).
                let suffix = format!(".{}", format.to_lowercase());
                match filename.strip_suffix(&suffix) {
                    Some(stem) => books[i].formats.push(Format {
                        name: stem.to_owned(),
                        size: r.get::<_, i64>(3)?.max(0) as u64,
                        format,
                    }),
                    None => warnings.push(format!("book {}: file {filename:?} doesn't end in {suffix}", books[i].id)),
                }
            }
            Ok(())
        })?;

        let custom_columns = custom_columns(conn)?;
        let kinds: HashMap<i64, (String, Datatype, bool)> =
            custom_columns.iter().map(|c| (c.id, (c.lookup_name(), c.datatype.clone(), c.is_multiple))).collect();
        each_row(
            conn,
            "SELECT book_id, field_id, text_value, num_value FROM custom_values ORDER BY book_id, field_id, position",
            [],
            |r| {
                let (Some(i), Some((key, datatype, multiple))) = (with(r.get(0)?), kinds.get(&r.get(1)?)) else {
                    return Ok(());
                };
                let text: Option<String> = r.get(2)?;
                let num: Option<f64> = r.get(3)?;
                let value = match datatype {
                    Datatype::Text if *multiple => {
                        if let (Some(t), CustomValue::TextList(list)) =
                            (text, books[i].custom.entry(key.clone()).or_insert(CustomValue::TextList(Vec::new())))
                        {
                            list.push(t);
                        }
                        return Ok(());
                    }
                    Datatype::Series => text.map(|name| CustomValue::Series { name, index: num.unwrap_or(1.0) }),
                    Datatype::Int => num.map(|n| CustomValue::Int(n as i64)),
                    Datatype::Float => num.map(CustomValue::Float),
                    Datatype::Bool => num.map(|n| CustomValue::Bool(n != 0.0)),
                    Datatype::Rating => num.map(|n| CustomValue::Rating(n as u8)),
                    Datatype::Datetime => text.and_then(|t| t.parse().ok()).map(CustomValue::Datetime),
                    _ => text.map(CustomValue::Text),
                };
                if let Some(v) = value {
                    books[i].custom.insert(key.clone(), v);
                }
                Ok(())
            },
        )?;

        let uuid = conn.query_row("SELECT value FROM library_info WHERE key = 'uuid'", [], |r| r.get(0)).ok();
        Ok(Catalog::new(uuid, custom_columns, books, warnings))
    }
}

fn custom_columns(conn: &Connection) -> Result<Vec<CustomColumn>> {
    let mut cols = Vec::new();
    each_row(conn, "SELECT id, key, name, kind, multiple, config FROM custom_fields ORDER BY id", [], |r| {
        let kind: String = r.get(3)?;
        let config: String = r.get(5)?;
        let config: serde_json::Value = serde_json::from_str(&config).unwrap_or(serde_json::Value::Null);
        cols.push(CustomColumn {
            id: r.get(0)?,
            label: r.get(1)?,
            name: r.get(2)?,
            datatype: match kind.as_str() {
                "text" => Datatype::Text,
                "long_text" => Datatype::Comments,
                "series" => Datatype::Series,
                "choice" => Datatype::Enumeration,
                "rating" => Datatype::Rating,
                "integer" => Datatype::Int,
                "number" => Datatype::Float,
                "bool" => Datatype::Bool,
                "date" => Datatype::Datetime,
                other => Datatype::Unknown(other.to_owned()),
            },
            is_multiple: r.get(4)?,
            normalized: false,
            editable: true,
            // Imported fields keep calibre's display settings under "calibre".
            display: config.get("calibre").cloned().unwrap_or(config),
        });
        Ok(())
    })?;
    Ok(cols)
}

/// Publication dates are stored as ISO dates; the catalog wants a timestamp.
fn date(s: &str) -> Option<DateTime<Utc>> {
    s.parse::<NaiveDate>().ok().and_then(|d| d.and_hms_opt(0, 0, 0)).map(|d| d.and_utc())
}

fn each_row<P: rusqlite::Params>(
    conn: &Connection,
    sql: &str,
    params: P,
    mut f: impl FnMut(&Row<'_>) -> rusqlite::Result<()>,
) -> Result<()> {
    let mut stmt = conn.prepare(sql)?;
    let mut rows = stmt.query(params)?;
    while let Some(row) = rows.next()? {
        f(row)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::*;
    use crate::library::NewBook;

    #[test]
    fn native_books_load_into_the_catalog() {
        let dir = tempfile::tempdir().unwrap();
        let mut lib = Library::create(dir.path(), "Test").unwrap();
        let id = lib
            .add_book(&NewBook {
                title: "Good Omens".into(),
                authors: vec!["Neil Gaiman".into(), "Terry Pratchett".into()],
                series: Some(("Standalone".into(), 1.0)),
                published: NaiveDate::from_ymd_opt(1990, 5, 1),
                tags: vec!["Humour".into(), "Fantasy".into()],
                rating: Some(8),
                ..Default::default()
            })
            .unwrap();

        let cat = lib.load_catalog().unwrap();
        let b = cat.get(id).unwrap();
        assert_eq!(b.authors, ["Neil Gaiman", "Terry Pratchett"]);
        assert_eq!(b.author_sort.as_deref(), Some("Gaiman, Neil & Pratchett, Terry"));
        assert_eq!(b.sort.as_deref(), Some("Good Omens"));
        assert_eq!(b.tags, ["Fantasy", "Humour"]);
        assert_eq!(b.rating, Some(8));
        assert_eq!(b.pubdate.map(|d| d.date_naive()), NaiveDate::from_ymd_opt(1990, 5, 1));
        assert!(b.timestamp.is_some());
        assert_eq!(cat.library_uuid, Some(lib.info().unwrap().uuid));
    }
}
