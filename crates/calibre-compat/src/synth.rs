//! Generate a synthetic calibre-format library for benchmarks and tests.
//!
//! Uses calibre's real schema (`schema/metadata_sqlite.sql`, v28) and the same
//! DDL calibre uses for custom columns, so the reader is exercised against
//! realistic data. It does not write `metadata.opf` backups or the
//! tag-browser views calibre creates for custom columns, so it has not been
//! verified that calibre itself will open these libraries.

use std::fs;
use std::path::Path;

use anyhow::{Result, bail};
use chrono::{DateTime, Duration, TimeZone, Utc};
use rusqlite::{Connection, params};

use crate::{dates, sqlfuncs};

pub const SCHEMA: &str = include_str!("../schema/metadata_sqlite.sql");

const FIRST: &[&str] = &[
    "Ursula",
    "Octavia",
    "Iain",
    "Ann",
    "Ted",
    "N. K.",
    "Ken",
    "Martha",
    "Becky",
    "Arkady",
    "Adrian",
    "Jo",
    "Mary",
    "Kim",
    "Susanna",
    "Terry",
    "Neal",
    "China",
    "Ada",
    "Nnedi",
    "Yoon Ha",
    "Ruth",
    "Kazuo",
    "Haruki",
    "Toni",
    "Chimamanda",
    "Italo",
    "Jorge Luis",
    "Olga",
    "Hilary",
];
const LAST: &[&str] = &[
    "Le Guin",
    "Butler",
    "Banks",
    "Leckie",
    "Chiang",
    "Jemisin",
    "Liu",
    "Wells",
    "Chambers",
    "Martine",
    "Tchaikovsky",
    "Walton",
    "Robinson",
    "Clarke",
    "Pratchett",
    "Stephenson",
    "Miéville",
    "Palmer",
    "Okorafor",
    "Lee",
    "Ozeki",
    "Ishiguro",
    "Murakami",
    "Morrison",
    "Adichie",
    "Calvino",
    "Borges",
    "Tokarczuk",
    "Mantel",
    "Atwood",
    "Gibson",
    "Herbert",
    "Asimov",
    "Dick",
    "Lem",
    "Strugatsky",
];
const WORDS: &[&str] = &[
    "Left",
    "Hand",
    "Darkness",
    "Parable",
    "Sower",
    "Player",
    "Games",
    "Ancillary",
    "Justice",
    "Story",
    "Life",
    "Fifth",
    "Season",
    "Problem",
    "Body",
    "Murderbot",
    "Long",
    "Way",
    "Small",
    "Angry",
    "Planet",
    "Memory",
    "Empire",
    "Children",
    "Time",
    "Tooth",
    "Claw",
    "Ministry",
    "Future",
    "Light",
    "Fall",
    "City",
    "Glass",
    "Snow",
    "Crash",
    "Diamond",
    "Age",
    "Embassytown",
    "Too",
    "Sea",
    "Clock",
    "Remains",
    "Day",
    "Beloved",
    "Invisible",
    "Cities",
    "Ficciones",
    "Flights",
    "Wolf",
    "Hall",
    "Handmaid",
    "Tale",
    "Neuromancer",
    "Dune",
    "Foundation",
    "Ubik",
    "Solaris",
    "Roadside",
    "Picnic",
    "Kindred",
    "Dawn",
];
const TAGS: &[&str] = &[
    "Science Fiction",
    "Fantasy",
    "Literary Fiction",
    "Mystery",
    "Thriller",
    "Horror",
    "Romance",
    "History",
    "Biography",
    "Philosophy",
    "Poetry",
    "Essays",
    "Short Stories",
    "Classics",
    "Space Opera",
    "Cyberpunk",
    "Solarpunk",
    "Translated",
    "Award Winner",
    "Hugo",
    "Nebula",
    "Booker",
    "Favourites",
    "To Read",
    "Reread",
    "Non-fiction",
    "Science",
    "Politics",
    "Economics",
    "Travel",
    "Cooking",
    "Art",
];
const PUBLISHERS: &[&str] = &[
    "Ace",
    "Tor",
    "Orbit",
    "Gollancz",
    "Penguin",
    "Vintage",
    "Del Rey",
    "Faber & Faber",
    "Picador",
    "Harper Voyager",
    "Saga Press",
    "Tordotcom",
    "Small Beer Press",
    "New Directions",
    "Knopf",
];
const LANG_CODES: &[&str] = &["eng", "deu", "fra", "spa", "jpn"];
/// Indexes into LANG_CODES; English-heavy like a typical library.
const LANG_WEIGHTS: &[usize] = &[0, 0, 0, 0, 1, 2, 3, 4];
const SHELVES: &[&str] = &["Living room", "Bedroom", "Office", "Storage", "Lent out"];
const LOREM: &[&str] = &[
    "a",
    "quiet",
    "planet",
    "where",
    "the",
    "crew",
    "finds",
    "something",
    "unexpected",
    "about",
    "memory",
    "and",
    "empire",
    "while",
    "an",
    "old",
    "promise",
    "slowly",
    "comes",
    "due",
    "in",
    "city",
    "of",
    "glass",
];

/// SplitMix64: tiny, deterministic, good enough for fake data.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn chance(&mut self, pct: u64) -> bool {
        self.next() % 100 < pct
    }
    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
    fn date_between(&mut self, from: DateTime<Utc>, to: DateTime<Utc>) -> DateTime<Utc> {
        let span = (to - from).num_seconds().max(1) as u64;
        from + Duration::seconds((self.next() % span) as i64)
    }
}

pub struct Options {
    pub books: usize,
    /// Also create book folders with placeholder format files and covers.
    pub with_files: bool,
    pub seed: u64,
}

pub fn create(dir: &Path, opts: &Options) -> Result<()> {
    let db_path = dir.join("metadata.db");
    if db_path.exists() {
        bail!("{} already exists; refusing to overwrite", db_path.display());
    }
    fs::create_dir_all(dir)?;
    let mut conn = Connection::open(&db_path)?;
    sqlfuncs::register(&conn)?;
    conn.execute_batch(SCHEMA)?;
    conn.execute("INSERT INTO library_id(uuid) VALUES (uuid4())", [])?;

    let mut rng = Rng(opts.seed);
    let tx = conn.transaction()?;

    // Custom columns, one of each common kind.
    let read = create_custom_column(&tx, "read", "Read", "bool", false, "{}")?;
    let genre = create_custom_column(&tx, "genre", "Genre", "text", true, r#"{"is_names": false}"#)?;
    let shelf_values = serde_json::to_string(SHELVES)?;
    let shelf = create_custom_column(
        &tx,
        "shelf",
        "Shelf",
        "enumeration",
        false,
        &format!(r#"{{"enum_values": {shelf_values}, "enum_colors": []}}"#),
    )?;
    let my_rating = create_custom_column(&tx, "my_rating", "My Rating", "rating", false, "{}")?;
    let date_read = create_custom_column(&tx, "date_read", "Date Read", "datetime", false, "{}")?;
    let reading_order = create_custom_column(&tx, "reading_order", "Reading Order", "series", false, "{}")?;
    create_custom_column(
        &tx,
        "isbn",
        "ISBN",
        "composite",
        false,
        r#"{"composite_template": "{identifiers:select(isbn)}"}"#,
    )?;

    // Value pools.
    let n_authors = (opts.books / 3).clamp(10, FIRST.len() * LAST.len());
    let mut authors = Vec::with_capacity(n_authors);
    for i in 0..n_authors {
        let (f, l) = (FIRST[i % FIRST.len()], LAST[(i / FIRST.len()) % LAST.len()]);
        let name = format!("{f} {l}");
        tx.execute("INSERT INTO authors(name, sort) VALUES (?, ?)", params![name, format!("{l}, {f}")])?;
        authors.push((tx.last_insert_rowid(), name, format!("{l}, {f}")));
    }
    let tags = insert_names(&tx, "INSERT INTO tags(name) VALUES (?)", TAGS)?;
    let publishers = insert_names(&tx, "INSERT INTO publishers(name) VALUES (?)", PUBLISHERS)?;
    let langs = insert_names(&tx, "INSERT INTO languages(lang_code) VALUES (?)", LANG_CODES)?;
    let ratings: Vec<i64> = (1..=10)
        .map(|r| {
            tx.execute("INSERT INTO ratings(rating) VALUES (?)", [r])?;
            Ok(tx.last_insert_rowid())
        })
        .collect::<rusqlite::Result<_>>()?;
    let series_names: Vec<String> = (0..(opts.books / 8).max(5))
        .map(|_| title(&mut rng, false))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let series = insert_names(&tx, "INSERT INTO series(name) VALUES (?)", &series_names)?;
    let genre_ids = insert_names(&tx, &format!("INSERT INTO custom_column_{genre}(value) VALUES (?)"), TAGS)?;
    let shelf_ids = insert_names(&tx, &format!("INSERT INTO custom_column_{shelf}(value) VALUES (?)"), SHELVES)?;
    let my_rating_ids: Vec<i64> = (1..=10)
        .map(|r| {
            tx.execute(&format!("INSERT INTO custom_column_{my_rating}(value) VALUES (?)"), [r])?;
            Ok(tx.last_insert_rowid())
        })
        .collect::<rusqlite::Result<_>>()?;
    let order_ids = insert_names(
        &tx,
        &format!("INSERT INTO custom_column_{reading_order}(value) VALUES (?)"),
        &["Hainish Cycle", "Culture", "Earthsea", "Discworld", "Imperial Radch", "Broken Earth"],
    )?;

    let epoch = Utc.with_ymd_and_hms(2008, 1, 1, 0, 0, 0).unwrap();
    let now = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
    let pub_from = Utc.with_ymd_and_hms(1900, 1, 1, 0, 0, 0).unwrap();

    {
        let mut ins_book = tx.prepare(
            "INSERT INTO books(title, author_sort, timestamp, pubdate, series_index, has_cover, last_modified) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )?;
        let mut set_path = tx.prepare("UPDATE books SET path = ? WHERE id = ?")?;
        let link =
            |sql: &str, a: i64, b: i64| -> rusqlite::Result<()> { tx.prepare_cached(sql)?.execute([a, b]).map(|_| ()) };

        for _ in 0..opts.books {
            let with_article = rng.chance(25);
            let t = title(&mut rng, with_article);
            let n_auth = 1 + usize::from(rng.chance(15)) + usize::from(rng.chance(5));
            let mut book_authors: Vec<&(i64, String, String)> = Vec::new();
            while book_authors.len() < n_auth {
                let a = rng.pick(&authors);
                if !book_authors.iter().any(|x| x.0 == a.0) {
                    book_authors.push(a);
                }
            }
            let author_sort = book_authors.iter().map(|a| a.2.as_str()).collect::<Vec<_>>().join(" & ");
            let added = rng.date_between(epoch, now);
            let has_cover = rng.chance(92);
            let in_series = rng.chance(30);
            let series_index = if in_series { (1 + rng.below(9)) as f64 } else { 1.0 };
            ins_book.execute(params![
                t,
                author_sort,
                dates::format(&added),
                dates::format(&rng.date_between(pub_from, now)),
                series_index,
                has_cover,
                dates::format(&added),
            ])?;
            let id = tx.last_insert_rowid();
            let rel_path = format!("{}/{} ({id})", book_authors[0].1, t);
            set_path.execute(params![rel_path, id])?;

            for a in &book_authors {
                link("INSERT INTO books_authors_link(book, author) VALUES (?, ?)", id, a.0)?;
            }
            let mut used = Vec::new();
            for _ in 0..rng.below(5) {
                let tag = *rng.pick(&tags);
                if !used.contains(&tag) {
                    used.push(tag);
                    link("INSERT INTO books_tags_link(book, tag) VALUES (?, ?)", id, tag)?;
                }
            }
            if in_series {
                link("INSERT INTO books_series_link(book, series) VALUES (?, ?)", id, *rng.pick(&series))?;
            }
            if rng.chance(85) {
                link("INSERT INTO books_publishers_link(book, publisher) VALUES (?, ?)", id, *rng.pick(&publishers))?;
            }
            if rng.chance(40) {
                link("INSERT INTO books_ratings_link(book, rating) VALUES (?, ?)", id, ratings[2 * rng.below(5) + 1])?;
            }
            let lang = langs[*rng.pick(LANG_WEIGHTS)];
            link("INSERT INTO books_languages_link(book, lang_code) VALUES (?, ?)", id, lang)?;
            tx.prepare_cached("INSERT INTO identifiers(book, type, val) VALUES (?, 'isbn', ?)")?
                .execute(params![id, format!("978{:010}", rng.next() % 10_000_000_000)])?;
            tx.prepare_cached("INSERT INTO comments(book, text) VALUES (?, ?)")?
                .execute(params![id, comment(&mut rng)])?;

            let stem = format!("{} - {}", t, book_authors[0].1);
            let mut fmts = vec!["EPUB"];
            if rng.chance(30) {
                fmts.push("AZW3");
            }
            if rng.chance(15) {
                fmts.push("PDF");
            }
            for fmt in &fmts {
                let body = format!("placeholder {fmt} for book {id}\n");
                tx.prepare_cached("INSERT INTO data(book, format, uncompressed_size, name) VALUES (?, ?, ?, ?)")?
                    .execute(params![id, fmt, body.len() as i64, stem])?;
                if opts.with_files {
                    let dir = rel_path.split('/').fold(dir.to_path_buf(), |p, c| p.join(c));
                    fs::create_dir_all(&dir)?;
                    fs::write(dir.join(format!("{stem}.{}", fmt.to_lowercase())), body)?;
                    if has_cover {
                        fs::write(dir.join("cover.jpg"), b"placeholder, not a real JPEG")?;
                    }
                }
            }

            // Custom column values.
            if rng.chance(70) {
                tx.prepare_cached(&format!("INSERT INTO custom_column_{read}(book, value) VALUES (?, ?)"))?
                    .execute(params![id, rng.chance(50)])?;
            }
            let mut used = Vec::new();
            for _ in 0..rng.below(3) {
                let g = *rng.pick(&genre_ids);
                if !used.contains(&g) {
                    used.push(g);
                    link(&format!("INSERT INTO books_custom_column_{genre}_link(book, value) VALUES (?, ?)"), id, g)?;
                }
            }
            if rng.chance(60) {
                link(
                    &format!("INSERT INTO books_custom_column_{shelf}_link(book, value) VALUES (?, ?)"),
                    id,
                    *rng.pick(&shelf_ids),
                )?;
            }
            if rng.chance(30) {
                link(
                    &format!("INSERT INTO books_custom_column_{my_rating}_link(book, value) VALUES (?, ?)"),
                    id,
                    *rng.pick(&my_rating_ids),
                )?;
            }
            if rng.chance(35) {
                tx.prepare_cached(&format!("INSERT INTO custom_column_{date_read}(book, value) VALUES (?, ?)"))?
                    .execute(params![id, dates::format(&rng.date_between(added, now))])?;
            }
            if rng.chance(10) {
                tx.prepare_cached(&format!(
                    "INSERT INTO books_custom_column_{reading_order}_link(book, value, extra) VALUES (?, ?, ?)"
                ))?
                .execute(params![id, *rng.pick(&order_ids), (1 + rng.below(12)) as f64])?;
            }
        }
    }
    tx.commit()?;
    conn.execute_batch("ANALYZE; VACUUM;")?;
    Ok(())
}

fn title(rng: &mut Rng, with_article: bool) -> String {
    let n = 1 + rng.below(4);
    let words: Vec<&str> = (0..n).map(|_| *rng.pick(WORDS)).collect();
    let t = words.join(" ");
    if with_article { format!("{} {t}", rng.pick(&["The", "A", "An"])) } else { t }
}

fn comment(rng: &mut Rng) -> String {
    let sentences: Vec<String> = (0..2 + rng.below(4))
        .map(|_| {
            let mut s = (0..6 + rng.below(10)).map(|_| *rng.pick(LOREM)).collect::<Vec<_>>().join(" ");
            s[..1].make_ascii_uppercase();
            s + "."
        })
        .collect();
    format!("<p>{}</p>", sentences.join(" "))
}

fn insert_names<S: AsRef<str>>(conn: &Connection, sql: &str, names: &[S]) -> Result<Vec<i64>> {
    let mut stmt = conn.prepare(sql)?;
    let mut ids = Vec::with_capacity(names.len());
    for n in names {
        stmt.execute([n.as_ref()])?;
        ids.push(conn.last_insert_rowid());
    }
    Ok(ids)
}

/// Port of the table/index/trigger DDL from calibre's
/// `DB.create_custom_column` (src/calibre/db/backend.py). The tag-browser
/// views are omitted because they need calibre's `books_list_filter` function.
fn create_custom_column(
    conn: &Connection,
    label: &str,
    name: &str,
    datatype: &str,
    is_multiple: bool,
    display: &str,
) -> Result<i64> {
    let normalized = !matches!(datatype, "datetime" | "comments" | "int" | "bool" | "float" | "composite");
    let is_multiple = is_multiple && matches!(datatype, "text" | "composite");
    conn.execute(
        "INSERT INTO custom_columns(label, name, datatype, is_multiple, editable, display, normalized) \
         VALUES (?, ?, ?, ?, 1, ?, ?)",
        params![label, name, datatype, is_multiple, display, normalized],
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
