//! Write a [`Catalog`] as a calibre library's `metadata.db`.
//!
//! The mirror of [`crate::library`]: what this writes, the reader reads back
//! unchanged (see the tests). Book ids, uuids, folders and custom-column ids
//! are kept, so a library written from Magnitide's catalog lines up with it
//! book for book. Only the database is written; the book folders are the
//! caller's (calibre plugins get links to the real files, for example).
//!
//! calibre itself opens these libraries: they use its real schema and the
//! DDL it uses for custom columns. It adds the preferences and views it
//! wants on first open.

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::Path;

use anyhow::{Result, bail};
use chrono::Utc;
use rusqlite::{Connection, Transaction, params};

use crate::synth::SCHEMA;
use crate::{dates, sqlfuncs};
use magnitide_catalog::{Book, Catalog, CustomColumn, CustomValue, Datatype};

/// calibre's "no date" (`calibre.utils.date.UNDEFINED_DATE`).
const UNDEFINED_DATE: &str = "0101-01-01 00:00:00+00:00";

#[derive(Debug, Default)]
pub struct WriteOptions {
    /// Author sort names ("Le Guin, Ursula K.") by author name. Authors
    /// missing from it sort by their name.
    pub author_sorts: HashMap<String, String>,
}

/// Write `catalog` as `dir/metadata.db`. Refuses to overwrite an existing one.
pub fn write_library(dir: &Path, catalog: &Catalog, opts: &WriteOptions) -> Result<()> {
    let db_path = dir.join("metadata.db");
    if db_path.exists() {
        bail!("{} already exists; refusing to overwrite", db_path.display());
    }
    fs::create_dir_all(dir)?;
    let mut conn = Connection::open(&db_path)?;
    sqlfuncs::register(&conn)?;
    conn.execute_batch(SCHEMA)?;
    let tx = conn.transaction()?;
    let library_uuid = catalog.library_uuid.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    tx.execute("INSERT INTO library_id(uuid) VALUES (?)", [library_uuid])?;

    let columns: Vec<(&CustomColumn, &'static str)> =
        catalog.custom_columns.iter().filter_map(|c| datatype_name(&c.datatype).map(|d| (c, d))).collect();
    for (col, datatype) in &columns {
        create_custom_column(&tx, Some(col.id), &col.label, &col.name, datatype, col.is_multiple, &display(col))?;
    }

    let mut names = Names::default();
    for book in &catalog.books {
        insert_book(&tx, book, opts, &mut names)?;
        for (col, _) in &columns {
            if let Some(value) = book.custom.get(&col.lookup_name()) {
                insert_custom_value(&tx, col, book.id, value, &mut names)?;
            }
        }
    }
    tx.commit()?;
    Ok(())
}

/// Ids of the rows already in each name table (authors, tags, …), by table and name.
#[derive(Default)]
struct Names(HashMap<(String, String), i64>);

impl Names {
    /// The id of `name` in `table`, inserting it with `insert` (bound to the name) if new.
    fn id(&mut self, tx: &Transaction<'_>, table: &str, name: &str, insert: &str) -> Result<i64> {
        // calibre's name columns are COLLATE NOCASE: "fantasy" is "Fantasy".
        let key = (table.to_owned(), name.to_lowercase());
        if let Some(&id) = self.0.get(&key) {
            return Ok(id);
        }
        tx.prepare_cached(insert)?.execute([name])?;
        let id = tx.last_insert_rowid();
        self.0.insert(key, id);
        Ok(id)
    }
}

fn insert_book(tx: &Transaction<'_>, b: &Book, opts: &WriteOptions, names: &mut Names) -> Result<()> {
    let sort_of = |a: &str| opts.author_sorts.get(a).cloned().unwrap_or_else(|| a.to_owned());
    let author_sort =
        b.author_sort.clone().unwrap_or_else(|| b.authors.iter().map(|a| sort_of(a)).collect::<Vec<_>>().join(" & "));
    let now = Utc::now();
    tx.prepare_cached(
        "INSERT INTO books (id, title, timestamp, pubdate, series_index, author_sort, path, has_cover, last_modified)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )?
    .execute(params![
        b.id,
        b.title,
        dates::format(&b.timestamp.unwrap_or(now)),
        b.pubdate.as_ref().map(dates::format).unwrap_or_else(|| UNDEFINED_DATE.to_owned()),
        b.series_index,
        author_sort,
        b.path,
        b.has_cover,
        dates::format(&b.last_modified.unwrap_or(now)),
    ])?;
    // The insert trigger sets a fresh sort and uuid; keep the book's own.
    tx.prepare_cached("UPDATE books SET sort = ?, uuid = ? WHERE id = ?")?.execute(params![
        b.sort.clone().unwrap_or_else(|| sqlfuncs::title_sort(&b.title)),
        b.uuid.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
        b.id
    ])?;

    let link = |sql: &str, value: i64| -> Result<()> {
        tx.prepare_cached(sql)?.execute([b.id, value])?;
        Ok(())
    };
    for author in &b.authors {
        let id = names.id(tx, "authors", author, "INSERT INTO authors (name) VALUES (?)")?;
        tx.prepare_cached("UPDATE authors SET sort = ? WHERE id = ? AND sort IS NULL")?
            .execute(params![sort_of(author), id])?;
        link("INSERT OR IGNORE INTO books_authors_link (book, author) VALUES (?, ?)", id)?;
    }
    for tag in &b.tags {
        let id = names.id(tx, "tags", tag, "INSERT INTO tags (name) VALUES (?)")?;
        link("INSERT OR IGNORE INTO books_tags_link (book, tag) VALUES (?, ?)", id)?;
    }
    if let Some(series) = &b.series {
        let id = names.id(tx, "series", series, "INSERT INTO series (name, sort) VALUES (?1, title_sort(?1))")?;
        link("INSERT INTO books_series_link (book, series) VALUES (?, ?)", id)?;
    }
    if let Some(publisher) = &b.publisher {
        let id = names.id(tx, "publishers", publisher, "INSERT INTO publishers (name) VALUES (?)")?;
        link("INSERT INTO books_publishers_link (book, publisher) VALUES (?, ?)", id)?;
    }
    if let Some(rating) = b.rating.filter(|&r| r > 0) {
        let id =
            names.id(tx, "ratings", &rating.to_string(), "INSERT INTO ratings (rating) VALUES (CAST(? AS INTEGER))")?;
        link("INSERT INTO books_ratings_link (book, rating) VALUES (?, ?)", id)?;
    }
    for (order, lang) in (0_i64..).zip(&b.languages) {
        let id = names.id(tx, "languages", lang, "INSERT INTO languages (lang_code) VALUES (?)")?;
        tx.prepare_cached("INSERT OR IGNORE INTO books_languages_link (book, lang_code, item_order) VALUES (?, ?, ?)")?
            .execute([b.id, id, order])?;
    }
    for (scheme, value) in &b.identifiers {
        tx.prepare_cached("INSERT OR REPLACE INTO identifiers (book, type, val) VALUES (?, ?, ?)")?
            .execute(params![b.id, scheme, value])?;
    }
    if let Some(comments) = &b.comments {
        tx.prepare_cached("INSERT INTO comments (book, text) VALUES (?, ?)")?.execute(params![b.id, comments])?;
    }
    for f in &b.formats {
        tx.prepare_cached("INSERT OR REPLACE INTO data (book, format, uncompressed_size, name) VALUES (?, ?, ?, ?)")?
            .execute(params![b.id, f.format.to_uppercase(), f.size as i64, f.name])?;
    }
    Ok(())
}

fn insert_custom_value(
    tx: &Transaction<'_>,
    col: &CustomColumn,
    book: i64,
    value: &CustomValue,
    names: &mut Names,
) -> Result<()> {
    let (table, link) = (col.value_table(), col.link_table());
    let insert_name = format!("INSERT INTO {table} (value) VALUES (?)");
    let mut link_to = |name: &str, extra: Option<f64>| -> Result<()> {
        let id = names.id(tx, &table, name, &insert_name)?;
        match extra {
            Some(index) => tx
                .prepare_cached(&format!("INSERT OR IGNORE INTO {link} (book, value, extra) VALUES (?, ?, ?)"))?
                .execute(params![book, id, index])?,
            None => tx
                .prepare_cached(&format!("INSERT OR IGNORE INTO {link} (book, value) VALUES (?, ?)"))?
                .execute([book, id])?,
        };
        Ok(())
    };
    match (&col.datatype, value) {
        (Datatype::Text, CustomValue::TextList(items)) => {
            for item in items.iter().collect::<BTreeSet<_>>() {
                link_to(item, None)?;
            }
        }
        (Datatype::Text | Datatype::Enumeration, CustomValue::Text(t)) => link_to(t, None)?,
        (Datatype::Series, CustomValue::Series { name, index }) => link_to(name, Some(*index))?,
        (Datatype::Rating, CustomValue::Rating(r)) if *r > 0 => {
            let id = names.id(
                tx,
                &table,
                &r.to_string(),
                &format!("INSERT INTO {table} (value) VALUES (CAST(? AS INTEGER))"),
            )?;
            tx.prepare_cached(&format!("INSERT OR IGNORE INTO {link} (book, value) VALUES (?, ?)"))?
                .execute([book, id])?;
        }
        (Datatype::Comments, CustomValue::Text(t)) => one(tx, &table, book, t)?,
        (Datatype::Int, CustomValue::Int(i)) => one(tx, &table, book, i)?,
        (Datatype::Float, CustomValue::Float(x)) => one(tx, &table, book, x)?,
        (Datatype::Bool, CustomValue::Bool(v)) => one(tx, &table, book, v)?,
        (Datatype::Datetime, CustomValue::Datetime(d)) => one(tx, &table, book, dates::format(d))?,
        // A value that doesn't fit its column's type isn't written.
        _ => {}
    }
    Ok(())
}

/// A non-normalized column: one (book, value) row.
fn one(tx: &Transaction<'_>, table: &str, book: i64, value: impl rusqlite::ToSql) -> Result<()> {
    tx.prepare_cached(&format!("INSERT INTO {table} (book, value) VALUES (?, ?)"))?.execute(params![book, value])?;
    Ok(())
}

/// A column's display settings as calibre keeps them. A choice column lists
/// its values in `enum_values` (Magnitide's own fields keep them in `choices`).
fn display(col: &CustomColumn) -> String {
    let mut display = if col.display.is_object() { col.display.clone() } else { serde_json::json!({}) };
    if col.datatype == Datatype::Enumeration {
        if display.get("enum_values").is_none() {
            display["enum_values"] = display.get("choices").cloned().unwrap_or_else(|| serde_json::json!([]));
        }
        if display.get("enum_colors").is_none() {
            display["enum_colors"] = serde_json::json!([]);
        }
    }
    display.to_string()
}

/// calibre's name for a column type; None for ones that can't be written.
fn datatype_name(d: &Datatype) -> Option<&'static str> {
    Some(match d {
        Datatype::Text => "text",
        Datatype::Comments => "comments",
        Datatype::Series => "series",
        Datatype::Enumeration => "enumeration",
        Datatype::Rating => "rating",
        Datatype::Int => "int",
        Datatype::Float => "float",
        Datatype::Bool => "bool",
        Datatype::Datetime => "datetime",
        Datatype::Composite | Datatype::Unknown(_) => return None,
    })
}

/// Port of the table/index/trigger DDL from calibre's
/// `DB.create_custom_column` (src/calibre/db/backend.py), returning the
/// column's id (`id`, when given). The tag-browser views are omitted because
/// they need calibre's `books_list_filter` function; calibre creates them
/// when it opens the library.
pub(crate) fn create_custom_column(
    conn: &Connection,
    id: Option<i64>,
    label: &str,
    name: &str,
    datatype: &str,
    is_multiple: bool,
    display: &str,
) -> Result<i64> {
    let normalized = !matches!(datatype, "datetime" | "comments" | "int" | "bool" | "float" | "composite");
    let is_multiple = is_multiple && matches!(datatype, "text" | "composite");
    conn.execute(
        "INSERT INTO custom_columns(id, label, name, datatype, is_multiple, editable, display, normalized) \
         VALUES (?, ?, ?, ?, ?, 1, ?, ?)",
        params![id, label, name, datatype, is_multiple, display, normalized],
    )?;
    let num = conn.last_insert_rowid();
    let dt = match datatype {
        "rating" | "int" => "INT",
        "float" => "REAL",
        "datetime" => "timestamp",
        "bool" => "BOOL",
        _ => "TEXT",
    };
    let collate = if dt == "TEXT" { "COLLATE NOCASE" } else { "" };
    let table = format!("custom_column_{num}");
    let lt = format!("books_custom_column_{num}_link");
    let fk_book = |trigger: &str, when: &str, tbl: &str| {
        format!(
            "CREATE TRIGGER {trigger} {when} ON {tbl} BEGIN SELECT CASE \
             WHEN (SELECT id FROM books WHERE id=NEW.book) IS NULL \
             THEN RAISE(ABORT, 'Foreign key violation: book not in books') END; END;"
        )
    };
    let ddl = if normalized {
        let extra = if datatype == "series" { "extra REAL," } else { "" };
        format!(
            "CREATE TABLE {table}(id INTEGER PRIMARY KEY AUTOINCREMENT, value {dt} NOT NULL {collate}, \
                 link TEXT NOT NULL DEFAULT '', UNIQUE(value));
             CREATE INDEX {table}_idx ON {table} (value {collate});
             CREATE TABLE {lt}(id INTEGER PRIMARY KEY AUTOINCREMENT, book INTEGER NOT NULL, \
                 value INTEGER NOT NULL, {extra} UNIQUE(book, value));
             CREATE INDEX {lt}_aidx ON {lt} (value);
             CREATE INDEX {lt}_bidx ON {lt} (book);
             {}
             CREATE TRIGGER fkc_delete_{lt} AFTER DELETE ON {table} BEGIN DELETE FROM {lt} WHERE value=OLD.id; END;",
            fk_book(&format!("fkc_update_{lt}_a"), "BEFORE UPDATE OF book", &lt),
        )
    } else {
        format!(
            "CREATE TABLE {table}(id INTEGER PRIMARY KEY AUTOINCREMENT, book INTEGER, \
                 value {dt} NOT NULL {collate}, UNIQUE(book));
             CREATE INDEX {table}_idx ON {table} (book);
             {}
             {}",
            fk_book(&format!("fkc_insert_{table}"), "BEFORE INSERT", &table),
            fk_book(&format!("fkc_update_{table}"), "BEFORE UPDATE OF book", &table),
        )
    };
    // calibre creates a (never used) value table even for composite columns.
    conn.execute_batch(&ddl)?;
    Ok(num)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use chrono::TimeZone;
    use magnitide_catalog::Format;

    use super::*;
    use crate::Library;

    fn column(id: i64, label: &str, datatype: Datatype, is_multiple: bool) -> CustomColumn {
        CustomColumn {
            id,
            label: label.into(),
            name: label.to_uppercase(),
            datatype,
            is_multiple,
            normalized: false,
            editable: true,
            display: serde_json::Value::Null,
        }
    }

    fn book(id: i64, title: &str) -> Book {
        Book {
            id,
            title: title.into(),
            sort: None,
            authors: vec!["Ursula K. Le Guin".into()],
            author_sort: None,
            series: None,
            series_index: 1.0,
            series_end: None,
            more_series: vec![],
            collects: vec![],
            collected_in: vec![],
            tags: vec![],
            shelves: vec![],
            publisher: None,
            rating: None,
            languages: vec![],
            identifiers: BTreeMap::new(),
            comments: None,
            timestamp: Some(Utc.with_ymd_and_hms(2020, 1, 2, 3, 4, 5).unwrap()),
            pubdate: None,
            last_modified: Some(Utc.with_ymd_and_hms(2021, 1, 2, 3, 4, 5).unwrap()),
            path: format!("Ursula K. Le Guin/{title} ({id})"),
            uuid: Some(format!("00000000-0000-4000-8000-{id:012}")),
            has_cover: false,
            formats: vec![],
            custom: BTreeMap::new(),
        }
    }

    #[test]
    fn what_is_written_reads_back() {
        let mut a = book(7, "The Dispossessed");
        a.sort = Some("Dispossessed, The".into());
        a.authors.push("Second Author".into());
        a.series = Some("Hainish Cycle".into());
        a.series_index = 5.0;
        a.tags = vec!["Science Fiction".into(), "Classics".into()];
        a.publisher = Some("Harper & Row".into());
        a.rating = Some(8);
        a.languages = vec!["eng".into(), "fra".into()];
        a.identifiers = BTreeMap::from([("isbn".into(), "9780061054884".into())]);
        a.comments = Some("<p>An ambiguous utopia.</p>".into());
        a.pubdate = Some(Utc.with_ymd_and_hms(1974, 5, 1, 0, 0, 0).unwrap());
        a.has_cover = true;
        a.formats =
            vec![Format { format: "EPUB".into(), size: 1234, name: "The Dispossessed - Ursula K. Le Guin".into() }];
        a.custom = BTreeMap::from([
            ("#genre".into(), CustomValue::TextList(vec!["Anarchism".into(), "Utopia".into()])),
            ("#shelf".into(), CustomValue::Text("Favourites".into())),
            ("#notes".into(), CustomValue::Text("<b>Reread</b>".into())),
            ("#order".into(), CustomValue::Series { name: "Hainish".into(), index: 2.5 }),
            ("#pages".into(), CustomValue::Int(387)),
            ("#score".into(), CustomValue::Float(4.5)),
            ("#read".into(), CustomValue::Bool(true)),
            ("#mine".into(), CustomValue::Rating(6)),
            ("#finished".into(), CustomValue::Datetime(Utc.with_ymd_and_hms(2024, 6, 1, 12, 0, 0).unwrap())),
        ]);
        // Shares the series, a tag and the author; tag case differs ("classics").
        let mut b = book(9, "A Wizard of Earthsea");
        b.tags = vec!["classics".into()];
        b.custom = BTreeMap::from([("#genre".into(), CustomValue::TextList(vec!["Utopia".into()]))]);

        let columns = vec![
            column(3, "genre", Datatype::Text, true),
            column(4, "shelf", Datatype::Enumeration, false),
            column(5, "notes", Datatype::Comments, false),
            column(6, "order", Datatype::Series, false),
            column(7, "pages", Datatype::Int, false),
            column(8, "score", Datatype::Float, false),
            column(9, "read", Datatype::Bool, false),
            column(10, "mine", Datatype::Rating, false),
            column(11, "finished", Datatype::Datetime, false),
        ];
        let catalog = Catalog::new(Some("lib-uuid".into()), columns, vec![a.clone(), b.clone()], vec![]);
        let dir = tempfile::tempdir().unwrap();
        let opts =
            WriteOptions { author_sorts: HashMap::from([("Ursula K. Le Guin".into(), "Le Guin, Ursula K.".into())]) };
        write_library(dir.path(), &catalog, &opts).unwrap();

        let back = Library::open(dir.path()).unwrap().load().unwrap();
        assert!(back.warnings.is_empty(), "{:?}", back.warnings);
        assert_eq!(back.library_uuid.as_deref(), Some("lib-uuid"));
        let ids: Vec<i64> = back.custom_columns.iter().map(|c| c.id).collect();
        assert_eq!(ids, [3, 4, 5, 6, 7, 8, 9, 10, 11]);

        let got = back.get(7).unwrap();
        assert_eq!(got.title, a.title);
        assert_eq!(got.sort, a.sort);
        assert_eq!(got.authors, a.authors);
        assert_eq!(got.author_sort.as_deref(), Some("Le Guin, Ursula K. & Second Author"));
        assert_eq!((got.series.as_deref(), got.series_index), (Some("Hainish Cycle"), 5.0));
        assert_eq!(got.tags, a.tags);
        assert_eq!(got.publisher, a.publisher);
        assert_eq!(got.rating, Some(8));
        assert_eq!(got.languages, a.languages);
        assert_eq!(got.identifiers, a.identifiers);
        assert_eq!(got.comments, a.comments);
        assert_eq!((got.timestamp, got.pubdate, got.last_modified), (a.timestamp, a.pubdate, a.last_modified));
        assert_eq!((got.path.as_str(), got.uuid.as_deref(), got.has_cover), (a.path.as_str(), a.uuid.as_deref(), true));
        assert_eq!((got.formats[0].format.as_str(), got.formats[0].size), ("EPUB", 1234));
        assert_eq!(got.formats[0].name, a.formats[0].name);
        assert_eq!(got.custom, a.custom);

        let other = back.get(9).unwrap();
        assert_eq!(other.tags, ["Classics"], "name tables are case-insensitive, first spelling wins");
        assert_eq!(other.pubdate, None, "undefined date reads back as none");
        assert_eq!(other.custom, b.custom);
    }

    #[test]
    fn choice_columns_list_their_values_for_calibre() {
        let mut shelf = column(2, "shelf", Datatype::Enumeration, false);
        shelf.display = serde_json::json!({ "choices": ["To Read", "Read"] });
        let catalog = Catalog::new(None, vec![shelf], vec![], vec![]);
        let dir = tempfile::tempdir().unwrap();
        write_library(dir.path(), &catalog, &WriteOptions::default()).unwrap();
        let back = Library::open(dir.path()).unwrap().custom_columns().unwrap();
        assert_eq!(back[0].display["enum_values"], serde_json::json!(["To Read", "Read"]));
        assert_eq!(back[0].display["enum_colors"], serde_json::json!([]));
    }

    #[test]
    fn refuses_to_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let catalog = Catalog::new(None, vec![], vec![], vec![]);
        write_library(dir.path(), &catalog, &WriteOptions::default()).unwrap();
        assert!(write_library(dir.path(), &catalog, &WriteOptions::default()).is_err());
    }
}
