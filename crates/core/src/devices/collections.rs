//! Writing Magnitide's shelves to a Kobo as collections, for both of the
//! reading apps it may have: Kobo's own (shelves in `.kobo/KoboReader.sqlite`,
//! written the way calibre's KoboTouch driver writes them) and KOReader
//! (`.adds/koreader/settings/collection.lua`).
//!
//! Careful by design: only collections named after Magnitide shelves are
//! touched, only books Magnitide recognises on the device are added to or
//! taken off them, empty shelves aren't created, and nothing is deleted.
//! Kobo's database is backed up before each change.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

use super::lua::{self, Key, Table, Value};
use super::{MatchedBy, Scan};
use crate::library::BookId;

/// Where the device's storage is mounted, as the device itself sees it.
const ONBOARD: &str = "/mnt/onboard/";

/// Kobo's own collections, which a shelf mustn't be named after (calibre's list).
const KOBO_RESERVED: &[&str] =
    &["Im_Reading", "Read", "Closed", "Shortlist", "Archived", "Deleted", "OverDrive", "Preview", "Recommendation"];

/// What a sync did.
#[derive(Debug, Default, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct CollectionsReport {
    /// Shelves written to Kobo's own app (None: no Kobo database).
    pub kobo: Option<usize>,
    /// Collections written to KOReader (None: KOReader isn't installed).
    pub koreader: Option<usize>,
    /// Books on the device placed on at least one shelf.
    pub books: usize,
    pub warnings: Vec<String>,
}

/// What should be on the device: each shelf's books there (device paths,
/// relative), and every device path Magnitide recognises (the only books it
/// takes off shelves).
#[derive(Debug, Default)]
pub struct Plan {
    pub shelves: Vec<(String, Vec<String>)>,
    pub known: HashSet<String>,
}

impl Plan {
    pub fn new(shelf_books: &[(String, Vec<BookId>)], scan: &Scan, matches: &[Option<(BookId, MatchedBy)>]) -> Self {
        let mut on_device: HashMap<BookId, Vec<&str>> = HashMap::new();
        let mut known = HashSet::new();
        for (file, m) in scan.files.iter().zip(matches) {
            if let Some((book, _)) = m {
                on_device.entry(*book).or_default().push(&file.path);
                known.insert(file.path.clone());
            }
        }
        let shelves = shelf_books
            .iter()
            .map(|(name, books)| {
                let paths = books.iter().flat_map(|b| on_device.get(b).into_iter().flatten()).map(|p| (*p).to_owned());
                (name.clone(), paths.collect())
            })
            .collect();
        Self { shelves, known }
    }

    fn books(&self) -> usize {
        self.shelves.iter().flat_map(|(_, p)| p).collect::<HashSet<_>>().len()
    }
}

/// Write the plan to a Kobo mounted at `root`. Kobo's database is copied into
/// `backups` first (the last few are kept).
pub fn sync_kobo(root: &Path, plan: &Plan, backups: &Path) -> CollectionsReport {
    let mut report = CollectionsReport { books: plan.books(), ..Default::default() };
    let plan_names: Vec<&str> = plan.shelves.iter().map(|(n, _)| n.as_str()).collect();
    for name in &plan_names {
        if KOBO_RESERVED.iter().any(|r| r.eq_ignore_ascii_case(name)) {
            report.warnings.push(format!("“{name}” is one of Kobo's own collection names, so it isn't written there"));
        }
    }
    let db = root.join(".kobo/KoboReader.sqlite");
    if db.is_file() {
        match backup(&db, backups).and_then(|()| write_nickel(&db, plan)) {
            Ok(n) => report.kobo = Some(n),
            Err(e) => report.warnings.push(format!("Kobo's collections weren't changed: {e}")),
        }
    }
    let koreader = root.join(".adds/koreader");
    if koreader.is_dir() {
        match write_koreader(&koreader.join("settings/collection.lua"), plan, backups) {
            Ok(n) => report.koreader = Some(n),
            Err(e) => report.warnings.push(format!("KOReader's collections weren't changed: {e}")),
        }
    }
    report
}

fn now() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// Copy a file into `dir` with a timestamp, keeping the newest five of its kind.
fn backup(file: &Path, dir: &Path) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let stem = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let dest = dir.join(format!("{}-{stem}", Utc::now().format("%Y%m%d-%H%M%S")));
    fs::copy(file, &dest).map_err(|e| format!("couldn't back it up first: {e}"))?;
    let mut old: Vec<PathBuf> = fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().ends_with(&format!("-{stem}"))))
        .collect();
    old.sort();
    for p in old.iter().rev().skip(5) {
        let _ = fs::remove_file(p);
    }
    Ok(())
}

/// Kobo's own app: shelves in `Shelf`, books on them in `ShelfContent`
/// (keyed by name and `file:///mnt/onboard/<path>`). Booleans are the text
/// 'true'/'false'. One transaction; how many shelves now hold our books.
fn write_nickel(db: &Path, plan: &Plan) -> Result<usize, String> {
    let mut conn = Connection::open(db).map_err(|e| e.to_string())?;
    let columns = |table: &str| -> Result<Vec<String>, String> {
        let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})")).map_err(|e| e.to_string())?;
        let cols = stmt.query_map([], |r| r.get::<_, String>(1)).map_err(|e| e.to_string())?;
        cols.collect::<rusqlite::Result<_>>().map_err(|e| e.to_string())
    };
    let shelf_cols = columns("Shelf")?;
    let content_cols = columns("ShelfContent")?;
    let needs = |cols: &[String], want: &[&str]| want.iter().all(|w| cols.iter().any(|c| c == w));
    if !needs(
        &shelf_cols,
        &["Name", "InternalName", "CreationDate", "LastModified", "_IsDeleted", "_IsVisible", "_IsSynced"],
    ) || !needs(&content_cols, &["ShelfName", "ContentId", "DateModified", "_IsDeleted", "_IsSynced"])
    {
        return Err("its database isn't laid out the way this version expects".into());
    }
    let with_id_type = needs(&shelf_cols, &["Id", "Type"]);
    let content_id = |path: &str| format!("file://{ONBOARD}{path}");
    let known: HashSet<String> = plan.known.iter().map(|p| content_id(p)).collect();

    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let mut written = 0;
    for (name, paths) in &plan.shelves {
        if KOBO_RESERVED.iter().any(|r| r.eq_ignore_ascii_case(name)) {
            continue;
        }
        let want: Vec<String> = paths.iter().map(|p| content_id(p)).collect();
        let deleted: Option<String> = tx
            .query_row("SELECT _IsDeleted FROM Shelf WHERE Name = ?", [name], |r| r.get(0))
            .optional()
            .map_err(|e| e.to_string())?;
        match deleted.as_deref() {
            // No shelf yet: only make one for books that are on the device.
            None if want.is_empty() => continue,
            None => {
                let at = now();
                if with_id_type {
                    tx.execute(
                        "INSERT INTO Shelf (CreationDate, InternalName, LastModified, Name, _IsDeleted, _IsVisible, _IsSynced, Id, Type)
                         VALUES (?1, ?2, ?1, ?2, 'false', 'true', 'false', ?2, 'UserTag')",
                        params![at, name],
                    )
                } else {
                    tx.execute(
                        "INSERT INTO Shelf (CreationDate, InternalName, LastModified, Name, _IsDeleted, _IsVisible, _IsSynced)
                         VALUES (?1, ?2, ?1, ?2, 'false', 'true', 'false')",
                        params![at, name],
                    )
                }
                .map_err(|e| e.to_string())?;
            }
            Some("true") if !want.is_empty() => {
                tx.execute("UPDATE Shelf SET _IsDeleted = 'false' WHERE Name = ?", [name])
                    .map_err(|e| e.to_string())?;
            }
            _ => {}
        }
        // Rows already there, and whether each is deleted.
        let mut existing: HashMap<String, bool> = HashMap::new();
        {
            let mut stmt = tx
                .prepare("SELECT ContentId, _IsDeleted FROM ShelfContent WHERE ShelfName = ?")
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map([name], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)))
                .map_err(|e| e.to_string())?;
            for row in rows {
                let (id, del) = row.map_err(|e| e.to_string())?;
                existing.insert(id, del.as_deref() == Some("true"));
            }
        }
        for id in &want {
            match existing.get(id) {
                None => tx.execute(
                    "INSERT INTO ShelfContent (ShelfName, ContentId, DateModified, _IsDeleted, _IsSynced)
                     VALUES (?, ?, ?, 'false', 'false')",
                    params![name, id, now()],
                ),
                Some(true) => tx.execute(
                    "UPDATE ShelfContent SET _IsDeleted = 'false' WHERE ShelfName = ? AND ContentId = ?",
                    params![name, id],
                ),
                Some(false) => Ok(0),
            }
            .map_err(|e| e.to_string())?;
        }
        // Books we know that are no longer on the shelf come off it; others stay.
        for id in existing.keys().filter(|id| known.contains(*id) && !want.contains(id)) {
            tx.execute("DELETE FROM ShelfContent WHERE ShelfName = ? AND ContentId = ?", params![name, id])
                .map_err(|e| e.to_string())?;
        }
        if !want.is_empty() {
            written += 1;
        }
    }
    tx.commit().map_err(|e| e.to_string())?;
    conn.close().map_err(|(_, e)| e.to_string())?;
    Ok(written)
}

/// KOReader: a Lua table keyed by collection name, each an array of
/// `{ file = "/mnt/onboard/…", order = n }` plus a `settings` table. Other
/// collections, unknown files and every setting are kept as they were.
fn write_koreader(file: &Path, plan: &Plan, backups: &Path) -> Result<usize, String> {
    let mut root = if file.is_file() {
        let text = fs::read_to_string(file).map_err(|e| e.to_string())?;
        match lua::parse(&text)? {
            Value::Table(t) => t,
            _ => return Err("its collections file isn't a table".into()),
        }
    } else {
        Table::default()
    };
    let path_of = |p: &str| format!("{ONBOARD}{p}");
    let known: HashSet<String> = plan.known.iter().map(|p| path_of(p)).collect();
    let next_order = |root: &Table| -> f64 {
        root.0
            .iter()
            .filter_map(|(_, v)| v.as_table()?.get("settings")?.as_table()?.get("order")?.as_f64())
            .fold(0.0, f64::max)
            + 1.0
    };

    let mut written = 0;
    let mut changed = false;
    for (name, paths) in &plan.shelves {
        let want: Vec<String> = paths.iter().map(|p| path_of(p)).collect();
        if root.get(name).is_none() {
            if want.is_empty() {
                continue;
            }
            let mut settings = Table::default();
            settings.set("order", Value::Number(format_number(next_order(&root))));
            let mut coll = Table::default();
            coll.set("settings", Value::Table(settings));
            root.set(name, Value::Table(coll));
        }
        let Some(Value::Table(coll)) = root.get_mut(name) else {
            return Err(format!("its collection “{name}” isn't a table"));
        };
        // Items already there (file → order), in order.
        let mut items: Vec<(String, f64)> = coll
            .array()
            .iter()
            .filter_map(|v| {
                let t = v.as_table()?;
                Some((t.get("file")?.as_str()?.to_owned(), t.get("order").and_then(Value::as_f64).unwrap_or(f64::MAX)))
            })
            .collect();
        items.sort_by(|a, b| a.1.total_cmp(&b.1));
        let before: Vec<String> = items.iter().map(|(f, _)| f.clone()).collect();
        // Keep files we don't know about and books still on the shelf; add the rest.
        let mut files: Vec<String> =
            before.iter().filter(|f| !known.contains(*f) || want.contains(f)).cloned().collect();
        for f in &want {
            if !files.contains(f) {
                files.push(f.clone());
            }
        }
        if files != before {
            changed = true;
            let entries = files
                .iter()
                .enumerate()
                .map(|(i, f)| {
                    let mut t = Table::default();
                    t.set("file", Value::Str(f.clone()));
                    t.set("order", Value::Number((i + 1).to_string()));
                    Value::Table(t)
                })
                .collect();
            coll.set_array(entries);
        }
        if !want.is_empty() {
            written += 1;
        }
    }
    // KOReader expects its default collection to exist.
    if root.get("favorites").is_none() && !root.0.is_empty() {
        let mut settings = Table::default();
        settings.set("order", Value::Number("1".into()));
        let mut fav = Table::default();
        fav.set("settings", Value::Table(settings));
        root.0.push((Key::Str("favorites".into()), Value::Table(fav)));
        changed = true;
    }
    if changed {
        if file.is_file() {
            backup(file, backups)?;
        }
        if let Some(dir) = file.parent() {
            fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let text = lua::write("./settings/collection.lua", &Value::Table(root));
        // Check it reads back before it replaces anything.
        lua::parse(&text).map_err(|e| format!("couldn't write it ({e})"))?;
        let tmp = file.with_extension("lua.magnitide-tmp");
        fs::write(&tmp, text).map_err(|e| e.to_string())?;
        fs::rename(&tmp, file).map_err(|e| e.to_string())?;
    }
    Ok(written)
}

fn format_number(n: f64) -> String {
    if n.fract() == 0.0 { format!("{}", n as i64) } else { n.to_string() }
}

/// Names written, for tests.
#[cfg(test)]
fn collection_files(file: &Path) -> std::collections::BTreeMap<String, Vec<String>> {
    let v = lua::parse(&fs::read_to_string(file).unwrap()).unwrap();
    let mut out = std::collections::BTreeMap::new();
    for (k, v) in &v.as_table().unwrap().0 {
        if let (Key::Str(name), Some(t)) = (k, v.as_table()) {
            let files =
                t.array().iter().filter_map(|i| i.as_table()?.get("file")?.as_str().map(str::to_owned)).collect();
            out.insert(name.clone(), files);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn plan(shelves: &[(&str, &[&str])], known: &[&str]) -> Plan {
        Plan {
            shelves: shelves
                .iter()
                .map(|(n, p)| ((*n).to_owned(), p.iter().map(|s| (*s).to_owned()).collect()))
                .collect(),
            known: known.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    /// A Kobo database with the shelf tables (as Kobo's current firmware has them).
    fn kobo(root: &Path) -> PathBuf {
        fs::create_dir_all(root.join(".kobo")).unwrap();
        let db = root.join(".kobo/KoboReader.sqlite");
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE Shelf (CreationDate TEXT, Id TEXT, InternalName TEXT, LastModified TEXT, Name TEXT,
                 Type TEXT, _IsDeleted BOOL, _IsVisible BOOL, _IsSynced BOOL, _SyncTime TEXT, LastAccessed TEXT);
             CREATE TABLE ShelfContent (ShelfName TEXT, ContentId TEXT, DateModified TEXT, _IsDeleted BOOL, _IsSynced BOOL);
             INSERT INTO Shelf (Name, InternalName, Id, _IsDeleted, _IsVisible, _IsSynced) VALUES ('Mine', 'Mine', 'Mine', 'false', 'true', 'false');
             INSERT INTO ShelfContent VALUES ('Mine', 'file:///mnt/onboard/A/a.epub', '', 'false', 'false');
             INSERT INTO Shelf (Name, InternalName, Id, _IsDeleted, _IsVisible, _IsSynced) VALUES ('To Read', 'To Read', 'To Read', 'false', 'true', 'false');
             INSERT INTO ShelfContent VALUES ('To Read', 'file:///mnt/onboard/B/b.epub', '', 'false', 'false');
             INSERT INTO ShelfContent VALUES ('To Read', 'file:///mnt/onboard/X/not-in-library.epub', '', 'false', 'false');",
        )
        .unwrap();
        db
    }

    fn shelf_rows(db: &Path) -> Vec<(String, String, String)> {
        let conn = Connection::open(db).unwrap();
        let mut stmt = conn
            .prepare("SELECT ShelfName, ContentId, _IsDeleted FROM ShelfContent ORDER BY ShelfName, ContentId")
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap().map(Result::unwrap).collect()
    }

    #[test]
    fn writes_kobo_shelves_carefully() {
        let tmp = tempfile::tempdir().unwrap();
        let db = kobo(tmp.path());
        // To Read: C is added, B (known) comes off, the unknown book stays.
        // Favourites: a new shelf for A. Empty: no books on the device, so no shelf.
        let p = plan(
            &[("To Read", &["C/c.kepub.epub"]), ("Favourites", &["A/a.epub"]), ("Empty", &[]), ("Read", &["A/a.epub"])],
            &["A/a.epub", "B/b.epub", "C/c.kepub.epub"],
        );
        let report = sync_kobo(tmp.path(), &p, &tmp.path().join("backups"));
        assert_eq!(report.kobo, Some(2));
        assert_eq!(report.koreader, None, "no KOReader on this one");
        assert_eq!(report.books, 2);
        assert!(report.warnings[0].contains("“Read”"), "{:?}", report.warnings);
        assert_eq!(
            shelf_rows(&db),
            [
                ("Favourites".into(), "file:///mnt/onboard/A/a.epub".into(), "false".into()),
                ("Mine".into(), "file:///mnt/onboard/A/a.epub".into(), "false".into()),
                ("To Read".into(), "file:///mnt/onboard/C/c.kepub.epub".into(), "false".into()),
                ("To Read".into(), "file:///mnt/onboard/X/not-in-library.epub".into(), "false".into()),
            ]
        );
        let conn = Connection::open(&db).unwrap();
        let shelves: Vec<(String, String, String, String)> = conn
            .prepare("SELECT Name, Id, Type, _IsVisible FROM Shelf WHERE Type IS NOT NULL")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(shelves, [("Favourites".into(), "Favourites".into(), "UserTag".into(), "true".into())]);
        assert!(!conn.prepare("SELECT 1 FROM Shelf WHERE Name = 'Empty'").unwrap().exists([]).unwrap());
        assert_eq!(fs::read_dir(tmp.path().join("backups")).unwrap().count(), 1, "backed up first");

        // Again: nothing changes.
        sync_kobo(tmp.path(), &p, &tmp.path().join("backups"));
        assert_eq!(shelf_rows(&db).len(), 4);
    }

    #[test]
    fn refuses_an_unexpected_kobo_database() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join(".kobo")).unwrap();
        Connection::open(tmp.path().join(".kobo/KoboReader.sqlite"))
            .unwrap()
            .execute_batch("CREATE TABLE Shelf (Name TEXT);")
            .unwrap();
        let report = sync_kobo(tmp.path(), &plan(&[("A", &["a.epub"])], &["a.epub"]), &tmp.path().join("backups"));
        assert_eq!(report.kobo, None);
        assert!(report.warnings[0].contains("weren't changed"), "{:?}", report.warnings);
    }

    #[test]
    fn writes_koreader_collections_keeping_everything_else() {
        let tmp = tempfile::tempdir().unwrap();
        let settings = tmp.path().join(".adds/koreader/settings");
        fs::create_dir_all(&settings).unwrap();
        let file = settings.join("collection.lua");
        fs::write(
            &file,
            r#"-- ./settings/collection.lua
return {
    ["To Read"] = {
        [1] = { ["file"] = "/mnt/onboard/X/mine.epub", ["order"] = 1, },
        [2] = { ["file"] = "/mnt/onboard/B/b.epub", ["order"] = 2, },
        ["settings"] = { ["collate"] = "title", ["order"] = 2, },
    },
    ["favorites"] = { ["settings"] = { ["order"] = 1, }, },
}
"#,
        )
        .unwrap();
        let p = plan(
            &[("To Read", &["C/c.kepub.epub"]), ("Favourites", &["A/a.epub"]), ("Empty", &[])],
            &["A/a.epub", "B/b.epub", "C/c.kepub.epub"],
        );
        let report = sync_kobo(tmp.path(), &p, &tmp.path().join("backups"));
        assert_eq!(report.koreader, Some(2));
        assert_eq!(report.kobo, None, "no Kobo database here");
        assert_eq!(
            collection_files(&file),
            BTreeMap::from([
                ("Favourites".into(), vec!["/mnt/onboard/A/a.epub".into()]),
                ("To Read".into(), vec!["/mnt/onboard/X/mine.epub".into(), "/mnt/onboard/C/c.kepub.epub".into()]),
                ("favorites".into(), vec![]),
            ])
        );
        let text = fs::read_to_string(&file).unwrap();
        assert!(text.starts_with("-- ./settings/collection.lua\nreturn {\n"));
        assert!(text.contains(r#"["collate"] = "title","#), "settings kept");
        assert!(text.contains("[\"Favourites\"] = {\n        [1] = {"), "KOReader's layout");
        // The new collection goes after the others.
        let v = lua::parse(&text).unwrap();
        let order = v
            .as_table()
            .unwrap()
            .get("Favourites")
            .unwrap()
            .as_table()
            .unwrap()
            .get("settings")
            .unwrap()
            .as_table()
            .unwrap()
            .get("order")
            .unwrap()
            .clone();
        assert_eq!(order, Value::Number("3".into()));
        assert_eq!(fs::read_dir(tmp.path().join("backups")).unwrap().count(), 1);
    }

    #[test]
    fn leaves_an_unreadable_koreader_file_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let settings = tmp.path().join(".adds/koreader/settings");
        fs::create_dir_all(&settings).unwrap();
        fs::write(settings.join("collection.lua"), "return { broken").unwrap();
        let report = sync_kobo(tmp.path(), &plan(&[("A", &["a.epub"])], &["a.epub"]), &tmp.path().join("backups"));
        assert_eq!(report.koreader, None);
        assert_eq!(fs::read_to_string(settings.join("collection.lua")).unwrap(), "return { broken");
    }
}
