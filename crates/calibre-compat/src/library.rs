//! Read-only access to a calibre library.
//!
//! The whole library is loaded with one query per table (no per-book
//! queries), the same strategy calibre's own in-memory cache uses.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags, Row};

use crate::dates;
use magnitide_catalog::{Book, BookId, Catalog, CustomColumn, CustomValue, Datatype, Format};

/// `PRAGMA application_id` of calibre databases: "cali" as a big-endian int.
pub const CALIBRE_APPLICATION_ID: i32 = 0x6361_6c69;
/// The `PRAGMA user_version` this code was written and tested against.
pub const TESTED_SCHEMA_VERSION: i32 = 28;

pub struct Library {
    root: PathBuf,
    db_path: PathBuf,
    conn: Connection,
    pub schema_version: i32,
    pub application_id: i32,
}

/// A calibre library loaded into memory (the shared catalog type).
pub type Snapshot = Catalog;

impl Library {
    /// `path` may be the library folder or the `metadata.db` file itself.
    ///
    /// The database is opened read-only and `query_only`, so this can never
    /// modify a library, even one calibre currently has open.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let (root, db_path) = if path.is_dir() {
            (path.to_path_buf(), path.join("metadata.db"))
        } else {
            let root = path.parent().unwrap_or(Path::new(".")).to_path_buf();
            (root, path.to_path_buf())
        };
        if !db_path.is_file() {
            bail!("no metadata.db at {}", db_path.display());
        }

        let conn =
            Connection::open_with_flags(&db_path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)
                .with_context(|| format!("opening {}", db_path.display()))?;
        conn.pragma_update(None, "query_only", true)?;
        conn.busy_timeout(Duration::from_secs(5))?;

        let application_id: i32 = conn.query_row("PRAGMA application_id", [], |r| r.get(0))?;
        let schema_version: i32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if application_id != CALIBRE_APPLICATION_ID {
            // Libraries created by old calibre versions predate the application id.
            let core_tables: i64 = conn.query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'table' \
                 AND name IN ('books', 'custom_columns', 'data', 'books_authors_link')",
                [],
                |r| r.get(0),
            )?;
            if core_tables != 4 {
                bail!("{} does not look like a calibre library", db_path.display());
            }
        }

        Ok(Self { root, db_path, conn, schema_version, application_id })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    /// A human-readable note when the schema differs from the tested version.
    pub fn schema_note(&self) -> Option<String> {
        use std::cmp::Ordering::*;
        match self.schema_version.cmp(&TESTED_SCHEMA_VERSION) {
            Equal => None,
            Greater => Some(format!(
                "schema v{} is newer than tested v{TESTED_SCHEMA_VERSION}; reading should still work, writing must not be attempted",
                self.schema_version
            )),
            Less => Some(format!(
                "schema v{} is older than v{TESTED_SCHEMA_VERSION}; calibre would upgrade it on open",
                self.schema_version
            )),
        }
    }

    /// Read a value from the `preferences` table (JSON-encoded by calibre).
    pub fn preference(&self, key: &str) -> Result<Option<serde_json::Value>> {
        let raw: Option<String> =
            self.conn.query_row("SELECT val FROM preferences WHERE key = ?", [key], |r| r.get(0)).map(Some).or_else(
                |e| match e {
                    rusqlite::Error::QueryReturnedNoRows => Ok(None),
                    e => Err(e),
                },
            )?;
        raw.map(|s| serde_json::from_str(&s).with_context(|| format!("preference {key:?} is not JSON"))).transpose()
    }

    pub fn load(&self) -> Result<Snapshot> {
        let conn = &self.conn;
        let mut warnings = Vec::new();

        let mut books = Vec::new();
        each_row(
            conn,
            "SELECT id, title, sort, author_sort, timestamp, pubdate, last_modified, \
                    series_index, path, uuid, has_cover FROM books ORDER BY id",
            |r| {
                books.push(Book {
                    id: r.get(0)?,
                    title: text(r.get_ref(1)?).unwrap_or_default(),
                    sort: text(r.get_ref(2)?),
                    author_sort: text(r.get_ref(3)?),
                    timestamp: date(r.get_ref(4)?),
                    pubdate: date(r.get_ref(5)?),
                    last_modified: date(r.get_ref(6)?),
                    series_index: float(r.get_ref(7)?).unwrap_or(1.0),
                    path: text(r.get_ref(8)?).unwrap_or_default(),
                    uuid: text(r.get_ref(9)?),
                    has_cover: boolean(r.get_ref(10)?).unwrap_or(false),
                    authors: Vec::new(),
                    series: None,
                    tags: Vec::new(),
                    publisher: None,
                    rating: None,
                    languages: Vec::new(),
                    identifiers: BTreeMap::new(),
                    comments: None,
                    formats: Vec::new(),
                    custom: BTreeMap::new(),
                });
                Ok(())
            },
        )?;
        let index: HashMap<BookId, usize> = books.iter().enumerate().map(|(i, b)| (b.id, i)).collect();

        // Link rows whose book no longer exists are ignored, as calibre does.
        macro_rules! link {
            ($sql:expr, |$b:ident, $r:ident| $body:expr) => {
                each_row(conn, $sql, |$r| {
                    if let Some($b) = book_mut(&mut books, &index, $r.get(0)?) {
                        $body;
                    }
                    Ok(())
                })?
            };
        }

        link!(
            "SELECT l.book, a.name FROM books_authors_link l \
             JOIN authors a ON a.id = l.author ORDER BY l.id",
            |b, r| b.authors.extend(text(r.get_ref(1)?))
        );
        link!("SELECT l.book, s.name FROM books_series_link l JOIN series s ON s.id = l.series", |b, r| b.series =
            text(r.get_ref(1)?));
        link!("SELECT l.book, t.name FROM books_tags_link l JOIN tags t ON t.id = l.tag ORDER BY l.id", |b, r| b
            .tags
            .extend(text(r.get_ref(1)?)));
        link!(
            "SELECT l.book, p.name FROM books_publishers_link l \
             JOIN publishers p ON p.id = l.publisher",
            |b, r| b.publisher = text(r.get_ref(1)?)
        );
        link!("SELECT l.book, x.rating FROM books_ratings_link l JOIN ratings x ON x.id = l.rating", |b, r| b.rating =
            rating(r.get_ref(1)?));
        link!(
            "SELECT l.book, x.lang_code FROM books_languages_link l \
             JOIN languages x ON x.id = l.lang_code ORDER BY l.book, l.item_order",
            |b, r| b.languages.extend(text(r.get_ref(1)?))
        );
        link!("SELECT book, type, val FROM identifiers", |b, r| {
            if let (Some(k), Some(v)) = (text(r.get_ref(1)?), text(r.get_ref(2)?)) {
                b.identifiers.insert(k, v);
            }
        });
        link!("SELECT book, text FROM comments", |b, r| b.comments = text(r.get_ref(1)?));
        link!("SELECT book, format, uncompressed_size, name FROM data ORDER BY id", |b, r| {
            b.formats.push(Format {
                format: text(r.get_ref(1)?).unwrap_or_default().to_uppercase(),
                size: int(r.get_ref(2)?).unwrap_or(0).max(0) as u64,
                name: text(r.get_ref(3)?).unwrap_or_default(),
            })
        });

        let custom_columns = self.custom_columns()?;
        for col in &custom_columns {
            if let Err(e) = load_custom_values(conn, col, &mut books, &index) {
                warnings.push(format!("custom column #{}: {e:#}", col.label));
            }
        }

        let library_uuid = conn.query_row("SELECT uuid FROM library_id LIMIT 1", [], |r| r.get(0)).ok();

        Ok(Catalog::new(library_uuid, custom_columns, books, warnings))
    }

    pub fn custom_columns(&self) -> Result<Vec<CustomColumn>> {
        let mut cols = Vec::new();
        each_row(
            &self.conn,
            "SELECT id, label, name, datatype, is_multiple, normalized, editable, display \
             FROM custom_columns WHERE mark_for_delete = 0 ORDER BY id",
            |r| {
                cols.push(CustomColumn {
                    id: r.get(0)?,
                    label: r.get(1)?,
                    name: r.get(2)?,
                    datatype: Datatype::parse(&r.get::<_, String>(3)?),
                    is_multiple: boolean(r.get_ref(4)?).unwrap_or(false),
                    normalized: boolean(r.get_ref(5)?).unwrap_or(false),
                    editable: boolean(r.get_ref(6)?).unwrap_or(true),
                    display: text(r.get_ref(7)?)
                        .and_then(|s| serde_json::from_str(&s).ok())
                        .unwrap_or(serde_json::Value::Null),
                });
                Ok(())
            },
        )?;
        Ok(cols)
    }
}

fn load_custom_values(
    conn: &Connection,
    col: &CustomColumn,
    books: &mut [Book],
    index: &HashMap<BookId, usize>,
) -> Result<()> {
    let key = col.lookup_name();
    match &col.datatype {
        // Composite columns are templates evaluated at display time; nothing is stored.
        Datatype::Composite => return Ok(()),
        Datatype::Unknown(t) => bail!("unknown datatype {t:?}"),
        _ => {}
    }

    if col.normalized {
        // Values live in custom_column_N; books_custom_column_N_link maps
        // books to them. Series columns keep the series index in `extra`.
        let is_series = col.datatype == Datatype::Series;
        let sql = format!(
            "SELECT l.book, v.value{} FROM {} l JOIN {} v ON v.id = l.value ORDER BY l.id",
            if is_series { ", l.extra" } else { "" },
            col.link_table(),
            col.value_table(),
        );
        each_row(conn, &sql, |r| {
            let Some(b) = book_mut(books, index, r.get(0)?) else { return Ok(()) };
            let raw = r.get_ref(1)?;
            let value = match col.datatype {
                Datatype::Series => text(raw).map(|name| CustomValue::Series {
                    name,
                    index: float(r.get_ref(2).unwrap_or(ValueRef::Null)).unwrap_or(1.0),
                }),
                Datatype::Rating => rating(raw).map(CustomValue::Rating),
                Datatype::Text if col.is_multiple => {
                    if let Some(t) = text(raw)
                        && let CustomValue::TextList(v) =
                            b.custom.entry(key.clone()).or_insert_with(|| CustomValue::TextList(Vec::new()))
                    {
                        v.push(t);
                    }
                    return Ok(());
                }
                _ => text(raw).map(CustomValue::Text),
            };
            if let Some(v) = value {
                b.custom.insert(key.clone(), v);
            }
            Ok(())
        })
    } else {
        // One (book, value) row per book in custom_column_N.
        let sql = format!("SELECT book, value FROM {}", col.value_table());
        each_row(conn, &sql, |r| {
            let Some(b) = book_mut(books, index, r.get(0)?) else { return Ok(()) };
            let raw = r.get_ref(1)?;
            let value = match col.datatype {
                Datatype::Int => int(raw).map(CustomValue::Int),
                Datatype::Float => float(raw).map(CustomValue::Float),
                Datatype::Bool => boolean(raw).map(CustomValue::Bool),
                Datatype::Datetime => date(raw).map(CustomValue::Datetime),
                _ => text(raw).map(CustomValue::Text),
            };
            if let Some(v) = value {
                b.custom.insert(key.clone(), v);
            }
            Ok(())
        })
    }
}

fn each_row(conn: &Connection, sql: &str, mut f: impl FnMut(&Row<'_>) -> rusqlite::Result<()>) -> Result<()> {
    let mut stmt = conn.prepare(sql).with_context(|| format!("preparing: {sql}"))?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        f(row)?;
    }
    Ok(())
}

fn book_mut<'a>(books: &'a mut [Book], index: &HashMap<BookId, usize>, id: BookId) -> Option<&'a mut Book> {
    index.get(&id).map(|&i| &mut books[i])
}

// SQLite is dynamically typed and calibre libraries are decades old, so a
// column may hold any storage class. These coerce leniently.

fn text(v: ValueRef<'_>) -> Option<String> {
    match v {
        ValueRef::Null => None,
        ValueRef::Integer(i) => Some(i.to_string()),
        ValueRef::Real(f) => Some(f.to_string()),
        ValueRef::Text(t) | ValueRef::Blob(t) => Some(String::from_utf8_lossy(t).into_owned()),
    }
}

fn float(v: ValueRef<'_>) -> Option<f64> {
    match v {
        ValueRef::Integer(i) => Some(i as f64),
        ValueRef::Real(f) => Some(f),
        ValueRef::Text(t) => std::str::from_utf8(t).ok()?.trim().parse().ok(),
        _ => None,
    }
}

fn int(v: ValueRef<'_>) -> Option<i64> {
    match v {
        ValueRef::Integer(i) => Some(i),
        ValueRef::Real(f) => Some(f.round() as i64),
        ValueRef::Text(t) => std::str::from_utf8(t).ok()?.trim().parse().ok(),
        _ => None,
    }
}

fn boolean(v: ValueRef<'_>) -> Option<bool> {
    match v {
        ValueRef::Text(t) => match std::str::from_utf8(t).ok()?.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" => Some(true),
            "0" | "false" | "no" => Some(false),
            _ => None,
        },
        other => int(other).map(|i| i != 0),
    }
}

/// calibre ratings are 0..=10; 0 means "not rated".
fn rating(v: ValueRef<'_>) -> Option<u8> {
    int(v).filter(|&r| r > 0).map(|r| r.min(10) as u8)
}

fn date(v: ValueRef<'_>) -> Option<chrono::DateTime<chrono::Utc>> {
    dates::parse(&text(v)?)
}
