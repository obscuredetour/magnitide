//! Merging, renaming, splitting and removing the names books share: tags,
//! series, publishers and identifier schemes. "scifi", "Sci-fi" and "SF"
//! become one tag; a spelling changes everywhere at once; "Fiction / Fantasy"
//! becomes two tags; a tag goes from every book. Authors aren't here:
//! renaming one moves book folders, so that stays a per-book edit.

use rusqlite::{OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::library::{Library, now, prune_unused_names, tag_id};

/// A kind of shared name.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "lowercase")]
pub enum ValueField {
    Tags,
    Series,
    Publishers,
    Identifiers,
}

/// What a merge, split or removal did.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct MergeReport {
    /// Books whose metadata changed.
    pub books: usize,
}

/// A tag that holds several ("Fiction / Fantasy / Military"), and what it becomes.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct TagSplit {
    pub from: String,
    pub into: Vec<String>,
}

impl Library {
    /// Every book with any of the `from` names gets `to` instead (names
    /// compare case-insensitively; `to` may be one of them, or a new
    /// spelling of one). Returns how many books changed.
    pub fn merge_values(&mut self, field: ValueField, from: &[String], to: &str) -> Result<MergeReport> {
        let to = to.trim();
        if to.is_empty() {
            return Err(Error::InvalidBook("a name can't be empty".into()));
        }
        let from: Vec<&str> = from.iter().map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
        if from.is_empty() {
            return Ok(MergeReport { books: 0 });
        }
        let tx = self.conn.transaction()?;
        let books = match field {
            ValueField::Tags => merge_table(&tx, "tags", "book_tags", "tag_id", &from, to, None)?,
            ValueField::Series => merge_series(&tx, &from, to)?,
            ValueField::Publishers => merge_publishers(&tx, &from, to)?,
            ValueField::Identifiers => merge_schemes(&tx, &from, &to.to_lowercase())?,
        };
        touch(&tx, &books)?;
        prune_unused_names(&tx)?;
        tx.commit()?;
        Ok(MergeReport { books: books.len() })
    }
}

impl Library {
    /// Each book with a `from` tag gets its `into` tags instead (ones it
    /// already has stay as they are). Returns how many books changed.
    pub fn split_tags(&mut self, splits: &[TagSplit]) -> Result<MergeReport> {
        let tx = self.conn.transaction()?;
        let mut books: Vec<i64> = Vec::new();
        for split in splits {
            let into: Vec<&str> = split.into.iter().map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
            if into.is_empty() {
                return Err(Error::InvalidBook(format!("nothing to split \u{201c}{}\u{201d} into", split.from)));
            }
            let Some(old) = ids_named(&tx, "tags", &[split.from.trim()])?.first().copied() else { continue };
            let linked = books_linked(&tx, "book_tags", "tag_id", &[old])?;
            let parts: Vec<i64> = into.iter().map(|p| tag_id(&tx, p)).collect::<Result<_>>()?;
            for b in &linked {
                for part in &parts {
                    tx.execute("INSERT OR IGNORE INTO book_tags (book_id, tag_id) VALUES (?, ?)", params![b, part])?;
                }
                if !books.contains(b) {
                    books.push(*b);
                }
            }
            if !parts.contains(&old) {
                tx.execute("DELETE FROM book_tags WHERE tag_id = ?", [old])?;
            }
        }
        touch(&tx, &books)?;
        prune_unused_names(&tx)?;
        tx.commit()?;
        Ok(MergeReport { books: books.len() })
    }

    /// Take these names off every book (a series or publisher is unset; an
    /// identifier scheme's values are dropped). Returns how many books changed.
    pub fn remove_values(&mut self, field: ValueField, names: &[String]) -> Result<MergeReport> {
        let names: Vec<&str> = names.iter().map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
        let tx = self.conn.transaction()?;
        let books = match field {
            ValueField::Tags => {
                let ids = ids_named(&tx, "tags", &names)?;
                let books = books_linked(&tx, "book_tags", "tag_id", &ids)?;
                for id in &ids {
                    tx.execute("DELETE FROM book_tags WHERE tag_id = ?", [id])?;
                }
                books
            }
            ValueField::Series => {
                let ids = ids_named(&tx, "series", &names)?;
                let mut books = books_linked(&tx, "book_more_series", "series_id", &ids)?;
                let mut stmt = tx.prepare("SELECT id FROM books WHERE series_id = ?")?;
                for id in &ids {
                    for b in stmt.query_map([id], |r| r.get::<_, i64>(0))? {
                        let b = b?;
                        if !books.contains(&b) {
                            books.push(b);
                        }
                    }
                    tx.execute("DELETE FROM book_more_series WHERE series_id = ?", [id])?;
                    tx.execute("UPDATE books SET series_id = NULL, series_index = NULL WHERE series_id = ?", [id])?;
                }
                books
            }
            ValueField::Publishers => {
                let ids = ids_named(&tx, "publishers", &names)?;
                let mut books = Vec::new();
                let mut stmt = tx.prepare("SELECT id FROM books WHERE publisher_id = ?")?;
                for id in &ids {
                    for b in stmt.query_map([id], |r| r.get::<_, i64>(0))? {
                        let b = b?;
                        if !books.contains(&b) {
                            books.push(b);
                        }
                    }
                    tx.execute("UPDATE books SET publisher_id = NULL WHERE publisher_id = ?", [id])?;
                }
                books
            }
            ValueField::Identifiers => {
                let mut books = Vec::new();
                let mut stmt = tx.prepare("SELECT book_id FROM identifiers WHERE scheme = ?")?;
                for scheme in &names {
                    for b in stmt.query_map([scheme], |r| r.get::<_, i64>(0))? {
                        let b = b?;
                        if !books.contains(&b) {
                            books.push(b);
                        }
                    }
                    tx.execute("DELETE FROM identifiers WHERE scheme = ?", [scheme])?;
                }
                books
            }
        };
        touch(&tx, &books)?;
        prune_unused_names(&tx)?;
        tx.commit()?;
        Ok(MergeReport { books: books.len() })
    }
}

/// Mark books as modified now (their files' metadata is out of date).
fn touch(tx: &Transaction<'_>, books: &[i64]) -> Result<()> {
    let at = now();
    for id in books {
        tx.execute("UPDATE books SET modified_at = ? WHERE id = ?", params![at, id])?;
    }
    Ok(())
}

/// The ids of a name table's rows called any of `names` (case-insensitively).
fn ids_named(tx: &Transaction<'_>, table: &str, names: &[&str]) -> Result<Vec<i64>> {
    let mut stmt = tx.prepare(&format!("SELECT id FROM {table} WHERE name = ?"))?;
    let mut ids = Vec::new();
    for name in names {
        if let Some(id) = stmt.query_row([name], |r| r.get::<_, i64>(0)).optional()?
            && !ids.contains(&id)
        {
            ids.push(id);
        }
    }
    Ok(ids)
}

/// The row to merge into: the one already called `to` (renamed to this exact
/// spelling), or a new row. `sort` is the sort name for tables that have one.
fn target_id(tx: &Transaction<'_>, table: &str, to: &str, sort: Option<&str>) -> Result<i64> {
    if let Some(id) = tx
        .prepare(&format!("SELECT id FROM {table} WHERE name = ?"))?
        .query_row([to], |r| r.get::<_, i64>(0))
        .optional()?
    {
        match sort {
            Some(s) => {
                tx.execute(&format!("UPDATE {table} SET name = ?, sort_name = ? WHERE id = ?"), params![to, s, id])?
            }
            None => tx.execute(&format!("UPDATE {table} SET name = ? WHERE id = ?"), params![to, id])?,
        };
        return Ok(id);
    }
    match sort {
        Some(s) => tx.execute(&format!("INSERT INTO {table} (name, sort_name) VALUES (?, ?)"), params![to, s])?,
        None => tx.execute(&format!("INSERT INTO {table} (name) VALUES (?)"), [to])?,
    };
    Ok(tx.last_insert_rowid())
}

/// The books linked to any of `ids` through a link table.
fn books_linked(tx: &Transaction<'_>, link: &str, col: &str, ids: &[i64]) -> Result<Vec<i64>> {
    let mut out = Vec::new();
    let mut stmt = tx.prepare(&format!("SELECT DISTINCT book_id FROM {link} WHERE {col} = ?"))?;
    for id in ids {
        for b in stmt.query_map([id], |r| r.get::<_, i64>(0))? {
            let b = b?;
            if !out.contains(&b) {
                out.push(b);
            }
        }
    }
    Ok(out)
}

/// Merge through a (book_id, name_id) link table: links move to the target,
/// a book that had both keeps one.
fn merge_table(
    tx: &Transaction<'_>,
    table: &str,
    link: &str,
    col: &str,
    from: &[&str],
    to: &str,
    sort: Option<&str>,
) -> Result<Vec<i64>> {
    let target = target_id(tx, table, to, sort)?;
    let old: Vec<i64> = ids_named(tx, table, from)?.into_iter().filter(|id| *id != target).collect();
    // A rename to the same spelling changes nothing; to another spelling, the books with it.
    let renamed = if from.iter().any(|f| f.eq_ignore_ascii_case(to)) && !from.contains(&to) {
        books_linked(tx, link, col, &[target])?
    } else {
        Vec::new()
    };
    let mut books = books_linked(tx, link, col, &old)?;
    for id in &old {
        tx.execute(&format!("UPDATE OR IGNORE {link} SET {col} = ? WHERE {col} = ?"), params![target, id])?;
        tx.execute(&format!("DELETE FROM {link} WHERE {col} = ?"), [id])?;
    }
    for b in renamed {
        if !books.contains(&b) {
            books.push(b);
        }
    }
    Ok(books)
}

fn merge_series(tx: &Transaction<'_>, from: &[&str], to: &str) -> Result<Vec<i64>> {
    let sort = crate::names::title_sort(to);
    let mut books = merge_table(tx, "series", "book_more_series", "series_id", from, to, Some(&sort))?;
    let target = ids_named(tx, "series", &[to])?[0];
    let old: Vec<i64> = ids_named(tx, "series", from)?.into_iter().filter(|id| *id != target).collect();
    let mut stmt = tx.prepare("SELECT id FROM books WHERE series_id = ?")?;
    for id in &old {
        for b in stmt.query_map([id], |r| r.get::<_, i64>(0))? {
            let b = b?;
            if !books.contains(&b) {
                books.push(b);
            }
        }
        tx.execute("UPDATE books SET series_id = ? WHERE series_id = ?", params![target, id])?;
    }
    Ok(books)
}

fn merge_publishers(tx: &Transaction<'_>, from: &[&str], to: &str) -> Result<Vec<i64>> {
    let target = target_id(tx, "publishers", to, None)?;
    let old: Vec<i64> = ids_named(tx, "publishers", from)?.into_iter().filter(|id| *id != target).collect();
    let mut books = Vec::new();
    let mut stmt = tx.prepare("SELECT id FROM books WHERE publisher_id = ?")?;
    let renamed = from.iter().any(|f| f.eq_ignore_ascii_case(to)) && !from.contains(&to);
    for id in old.iter().chain(renamed.then_some(&target)) {
        for b in stmt.query_map([id], |r| r.get::<_, i64>(0))? {
            let b = b?;
            if !books.contains(&b) {
                books.push(b);
            }
        }
    }
    for id in &old {
        tx.execute("UPDATE books SET publisher_id = ? WHERE publisher_id = ?", params![target, id])?;
    }
    Ok(books)
}

/// Identifier schemes ("isbn13" → "isbn"); a book that had both keeps the target's value.
fn merge_schemes(tx: &Transaction<'_>, from: &[&str], to: &str) -> Result<Vec<i64>> {
    let mut books = Vec::new();
    let mut stmt = tx.prepare("SELECT book_id FROM identifiers WHERE scheme = ? AND scheme <> ?")?;
    for scheme in from {
        for b in stmt.query_map(params![scheme, to], |r| r.get::<_, i64>(0))? {
            let b = b?;
            if !books.contains(&b) {
                books.push(b);
            }
        }
        tx.execute("UPDATE OR IGNORE identifiers SET scheme = ? WHERE scheme = ?", params![to, scheme])?;
        tx.execute("DELETE FROM identifiers WHERE scheme = ? AND scheme <> ?", params![scheme, to])?;
    }
    Ok(books)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::import::add_files;
    use crate::library::BookId;

    fn library() -> (tempfile::TempDir, Library, Vec<BookId>) {
        let tmp = tempfile::tempdir().unwrap();
        let mut lib = Library::create(tmp.path().join("lib"), "Test").unwrap();
        let ids = ["Red Sister - Mark Lawrence.epub", "Grey Sister - Mark Lawrence.epub", "Dune - Frank Herbert.epub"]
            .iter()
            .map(|name| {
                let src = tmp.path().join(name);
                fs::write(&src, name).unwrap();
                add_files(&mut lib, &[src], |_| {}).unwrap().added[0]
            })
            .collect();
        (tmp, lib, ids)
    }

    fn set_tags(lib: &mut Library, id: BookId, tags: &[&str]) {
        let mut e = lib.book_edit(id).unwrap();
        e.tags = tags.iter().map(|t| t.to_string()).collect();
        lib.update_book(id, &e).unwrap();
    }

    fn tags_of(lib: &Library, id: BookId) -> Vec<String> {
        lib.load_catalog().unwrap().get(id).unwrap().tags.clone()
    }

    #[test]
    fn merges_and_renames_tags() {
        let (_tmp, mut lib, ids) = library();
        set_tags(&mut lib, ids[0], &["scifi", "Fantasy"]);
        set_tags(&mut lib, ids[1], &["Sci-Fi", "scifi"]);
        set_tags(&mut lib, ids[2], &["SF"]);

        let r = lib.merge_values(ValueField::Tags, &["scifi".into(), "SF".into()], "Sci-Fi").unwrap();
        assert_eq!(r.books, 3);
        assert_eq!(tags_of(&lib, ids[0]), ["Fantasy", "Sci-Fi"]);
        assert_eq!(tags_of(&lib, ids[1]), ["Sci-Fi"], "a book with both keeps one");
        assert_eq!(tags_of(&lib, ids[2]), ["Sci-Fi"]);

        // A change of case alone.
        let r = lib.merge_values(ValueField::Tags, &["fantasy".into()], "FANTASY").unwrap();
        assert_eq!(r.books, 1);
        assert_eq!(tags_of(&lib, ids[0]), ["FANTASY", "Sci-Fi"]);
        assert_eq!(lib.merge_values(ValueField::Tags, &["nope".into()], "Whatever").unwrap().books, 0);
        assert!(lib.merge_values(ValueField::Tags, &["Sci-Fi".into()], "  ").is_err());
    }

    #[test]
    fn splits_and_removes_tags() {
        let (_tmp, mut lib, ids) = library();
        set_tags(&mut lib, ids[0], &["Fiction / Fantasy / Military", "Fantasy"]);
        set_tags(&mut lib, ids[1], &["Dragons;epic", "Fiction"]);
        let splits = vec![
            TagSplit {
                from: "Fiction / Fantasy / Military".into(),
                into: vec!["Fiction".into(), "Fantasy".into(), "Military".into()],
            },
            TagSplit { from: "Dragons;epic".into(), into: vec!["Dragons".into(), "epic".into()] },
        ];
        assert_eq!(lib.split_tags(&splits).unwrap().books, 2);
        assert_eq!(tags_of(&lib, ids[0]), ["Fantasy", "Fiction", "Military"]);
        assert_eq!(tags_of(&lib, ids[1]), ["Dragons", "epic", "Fiction"]);
        assert_eq!(lib.remove_values(ValueField::Tags, &["fiction".into(), "nope".into()]).unwrap().books, 2);
        assert_eq!(tags_of(&lib, ids[0]), ["Fantasy", "Military"]);
        assert!(lib.split_tags(&[TagSplit { from: "Fantasy".into(), into: vec![" ".into()] }]).is_err());
    }

    #[test]
    fn merges_series_publishers_and_schemes() {
        let (_tmp, mut lib, ids) = library();
        for (i, (series, publisher)) in
            [("Book of the Ancestor", "Ace"), ("book of the ancestor", "ACE Books")].iter().enumerate()
        {
            let mut e = lib.book_edit(ids[i]).unwrap();
            e.series = Some(series.to_string());
            e.series_index = 1.0 + i as f64;
            e.publisher = Some(publisher.to_string());
            e.identifiers.insert("isbn13".into(), format!("978{i}"));
            lib.update_book(ids[i], &e).unwrap();
        }
        assert_eq!(
            lib.merge_values(ValueField::Series, &["Book of the Ancestor".into()], "Ancestor").unwrap().books,
            2
        );
        assert_eq!(lib.merge_values(ValueField::Publishers, &["ACE Books".into()], "Ace").unwrap().books, 1);
        assert_eq!(lib.merge_values(ValueField::Identifiers, &["isbn13".into()], "ISBN").unwrap().books, 2);
        let cat = lib.load_catalog().unwrap();
        let b = cat.get(ids[1]).unwrap();
        assert_eq!((b.series.as_deref(), b.series_index, b.publisher.as_deref()), (Some("Ancestor"), 2.0, Some("Ace")));
        assert_eq!(b.identifiers.get("isbn").map(String::as_str), Some("9781"));
        assert_eq!(cat.books.iter().filter(|b| b.publisher.as_deref() == Some("Ace")).count(), 2);
    }
}
