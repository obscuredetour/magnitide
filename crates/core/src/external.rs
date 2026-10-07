//! Applying changes made outside Magnitide (calibre plugins working on the
//! shadow library; see crates/plugins) through Magnitide's own edit paths,
//! so its rules hold for them too: folder and file names follow the
//! metadata, descriptions are sanitized, covers are stored as JPEG, and each
//! edit is all-or-nothing.
//!
//! Book by book: one that fails is reported and the rest carry on. Books
//! removed there are not deleted here; they're returned for the app to
//! confirm with the user first.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use magnitide_catalog::diff::{AddedBook, BookChanges, ChangedBook, Changes, CoverChange, FileChange};
use magnitide_catalog::{CustomValue, Datatype};
use rusqlite::{OptionalExtension, params};
use serde::Serialize;

use crate::catalog::custom_columns;
use crate::error::{Error, Result};
use crate::html;
use crate::import::calibre::{ImportReport, import_custom_fields, insert_custom_value, insert_custom_values};
use crate::import::store::{Cover, add_formats, store_book};
use crate::library::{BookId, Library, NewBook, now};

/// What applying changes did.
#[derive(Debug, Default, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ApplyReport {
    /// Books whose metadata, files or cover changed.
    pub updated: Vec<BookId>,
    /// New books, by their new ids here.
    pub added: Vec<BookId>,
    /// Books removed there, still here: the app asks before deleting them.
    pub removed: Vec<BookId>,
    pub fields_added: usize,
    /// What was done, a line per book or field: "“Dune”: title, tags, EPUB
    /// file replaced", "Added “Dune Omnibus” (EPUB)".
    pub details: Vec<String>,
    /// "Title: reason" for each book whose changes couldn't be applied.
    pub failed: Vec<String>,
    pub warnings: Vec<String>,
    /// Each new book's id there and its id here, in that order.
    #[serde(skip)]
    #[cfg_attr(feature = "ts", ts(skip))]
    pub added_from: Vec<(BookId, BookId)>,
    /// What `details` is written from: new fields, then a line per book.
    #[serde(skip)]
    #[cfg_attr(feature = "ts", ts(skip))]
    lines: Vec<Line>,
}

/// One line of an [`ApplyReport`]'s details, kept apart so reports from the
/// same run can be combined book by book.
#[derive(Debug, Clone, PartialEq)]
enum Line {
    Field(String),
    Added { id: BookId, title: String, formats: Vec<String> },
    Updated { id: BookId, title: String, parts: Vec<String>, added_formats: Vec<String>, removed_formats: Vec<String> },
}

impl ApplyReport {
    /// Add `other` (a later apply in the same run) to this report. A book
    /// added earlier in the run and changed since stays one added book, with
    /// its latest title and formats; a book updated twice is one update.
    pub fn absorb(&mut self, other: ApplyReport) {
        for id in other.updated {
            if !self.updated.contains(&id) && !self.added.contains(&id) {
                self.updated.push(id);
            }
        }
        self.added.extend(other.added);
        self.removed.extend(other.removed);
        self.fields_added += other.fields_added;
        self.failed.extend(other.failed);
        self.warnings.extend(other.warnings);
        self.added_from.extend(other.added_from);
        for line in other.lines {
            self.add_line(line);
        }
        self.details = self.lines.iter().map(Line::text).collect();
    }

    fn add_line(&mut self, line: Line) {
        let Line::Updated { id, title, parts, added_formats, removed_formats } = line else {
            self.lines.push(line);
            return;
        };
        match self
            .lines
            .iter_mut()
            .find(|l| matches!(l, Line::Added { id: i, .. } | Line::Updated { id: i, .. } if *i == id))
        {
            Some(Line::Added { title: t, formats, .. }) => {
                // New this run: its metadata isn't a change; its files are its formats.
                *t = title;
                formats.retain(|f| !removed_formats.contains(f));
                formats.extend(added_formats.into_iter().filter(|f| !formats.contains(f)).collect::<Vec<_>>());
            }
            Some(Line::Updated { title: t, parts: p, .. }) => {
                *t = title;
                p.extend(parts.into_iter().filter(|x| !p.contains(x)).collect::<Vec<_>>());
            }
            _ => self.lines.push(Line::Updated { id, title, parts, added_formats, removed_formats }),
        }
    }

    /// Nothing was changed, or found to need asking about.
    pub fn is_empty(&self) -> bool {
        self.updated.is_empty()
            && self.added.is_empty()
            && self.removed.is_empty()
            && self.fields_added == 0
            && self.failed.is_empty()
    }
}

impl Library {
    /// Apply `changes`. `discard` disposes of files a change removes (the app
    /// moves them to the Trash).
    pub fn apply_changes(
        &mut self,
        changes: &Changes,
        mut discard: impl FnMut(&Path) -> std::io::Result<()>,
    ) -> Result<ApplyReport> {
        let mut report = ApplyReport::default();
        if !changes.new_columns.is_empty() {
            let mut import = ImportReport::default();
            import_custom_fields(self, &changes.new_columns, &mut import)?;
            report.fields_added = import.custom_fields;
            report.warnings.extend(import.warnings);
            for col in &changes.new_columns {
                report.lines.push(Line::Field(format!("New field: {} (#{})", col.name, col.label)));
            }
        }
        let fields = self.custom_field_map()?;

        for change in &changes.books {
            let title = self.book(change.id).map(|b| b.title).unwrap_or_else(|_| format!("Book {}", change.id));
            match self.apply_book_changes(change, &fields, &mut discard) {
                Ok(()) => {
                    report.updated.push(change.id);
                    let title = self.book(change.id).map(|b| b.title).unwrap_or(title);
                    let formats = |want: fn(&FileChange) -> Option<&String>| {
                        change.files.iter().filter_map(want).cloned().collect::<Vec<_>>()
                    };
                    report.lines.push(Line::Updated {
                        id: change.id,
                        title,
                        parts: describe(change),
                        added_formats: formats(|f| match f {
                            FileChange::Added { format, .. } => Some(format),
                            _ => None,
                        }),
                        removed_formats: formats(|f| match f {
                            FileChange::Removed { format } => Some(format),
                            _ => None,
                        }),
                    });
                }
                Err(e) => report.failed.push(format!("{title}: {e}")),
            }
        }
        for added in &changes.added {
            match self.add_external_book(added, &fields) {
                Ok(id) => {
                    report.added.push(id);
                    report.added_from.push((added.book.id, id));
                    let formats = added.files.iter().map(|(f, _)| f.to_uppercase()).collect();
                    report.lines.push(Line::Added { id, title: added.book.title.clone(), formats });
                }
                Err(e) => report.failed.push(format!("{}: {e}", added.book.title)),
            }
        }
        report.removed = changes.removed.iter().copied().filter(|&id| self.book(id).is_ok()).collect();
        report.details = report.lines.iter().map(Line::text).collect();
        Ok(report)
    }

    fn apply_book_changes(
        &mut self,
        change: &ChangedBook,
        fields: &HashMap<String, (i64, Datatype)>,
        discard: &mut impl FnMut(&Path) -> std::io::Result<()>,
    ) -> Result<()> {
        let id = change.id;
        if has_book_edit(&change.fields) {
            let mut edit = self.book_edit(id)?;
            let c = &change.fields;
            if let Some(t) = &c.title {
                edit.title = t.clone();
                edit.title_sort = String::new();
            }
            if let Some(s) = &c.sort {
                edit.title_sort = s.clone();
            }
            if let Some(a) = &c.authors {
                edit.authors = a.clone();
            }
            if let Some(s) = &c.series {
                edit.series = s.as_ref().map(|(name, _)| name.clone());
                edit.series_index = s.as_ref().map_or(1.0, |(_, index)| *index);
                edit.series_end = None;
            }
            if let Some(t) = &c.tags {
                edit.tags = t.clone();
            }
            if let Some(p) = &c.publisher {
                edit.publisher = p.clone();
            }
            if let Some(r) = c.rating {
                edit.rating = r;
            }
            if let Some(l) = &c.languages {
                edit.languages = l.clone();
            }
            if let Some(i) = &c.identifiers {
                edit.identifiers = i.clone();
            }
            if let Some(d) = &c.comments {
                edit.description_html = d.clone();
            }
            if let Some(p) = c.published {
                edit.published = p.map(|d| d.to_string());
            }
            self.update_book(id, &edit)?;
        }
        for (key, value) in &change.fields.custom {
            self.set_custom_value(id, key, value.as_ref(), fields)?;
        }
        for file in &change.files {
            match file {
                FileChange::Added { format, path } => self.add_format_file(id, format, path)?,
                FileChange::Replaced { format, path } => self.replace_format_file(id, format, path)?,
                FileChange::Removed { format } => self.remove_format(id, format, &mut *discard)?,
            }
        }
        match &change.cover {
            Some(CoverChange::Set(path)) => self.set_cover(id, &fs::read(path)?)?,
            Some(CoverChange::Removed) => self.remove_cover(id)?,
            None => {}
        }
        Ok(())
    }

    /// Set (or with `None`, clear) one custom field ("#genre") of a book.
    fn set_custom_value(
        &mut self,
        id: BookId,
        key: &str,
        value: Option<&CustomValue>,
        fields: &HashMap<String, (i64, Datatype)>,
    ) -> Result<()> {
        let Some((field, datatype)) = fields.get(key) else {
            return Err(Error::InvalidBook(format!("this library has no field {key}")));
        };
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM custom_values WHERE book_id = ? AND field_id = ?", params![id, field])?;
        if let Some(value) = value {
            insert_custom_value(&tx, id, *field, datatype, value)?;
        }
        tx.execute("UPDATE books SET modified_at = ? WHERE id = ?", params![now(), id])?;
        tx.commit()?;
        Ok(())
    }

    /// Add a format to a book from `src` (cloned or copied in).
    pub fn add_format_file(&mut self, id: BookId, format: &str, src: &Path) -> Result<()> {
        add_formats(self, id, &[(format.to_uppercase(), src.to_path_buf())]).map(|_| ())
    }

    /// Replace the contents of one of a book's formats with `src`. Written
    /// beside the file, then renamed over it, so a failure leaves it as it was.
    pub fn replace_format_file(&mut self, id: BookId, format: &str, src: &Path) -> Result<()> {
        let book = self.book(id)?;
        let filename: String = self
            .conn
            .query_row("SELECT filename FROM files WHERE book_id = ? AND format = ?", params![id, format], |r| r.get(0))
            .optional()?
            .ok_or_else(|| Error::InvalidBook(format!("this book has no {format} file")))?;
        let dest = book.path.split('/').fold(self.root.clone(), |p, c| p.join(c)).join(&filename);
        let tmp = dest.with_file_name(format!(".{filename}.incoming"));
        let _ = fs::remove_file(&tmp);
        reflink_copy::reflink_or_copy(src, &tmp)?;
        let size = fs::metadata(&tmp)?.len();
        if let Err(e) = fs::rename(&tmp, &dest) {
            let _ = fs::remove_file(&tmp);
            return Err(e.into());
        }
        // The new contents may hold other metadata: check before trusting them.
        self.conn.execute(
            "UPDATE files SET size = ?, metadata_written_at = NULL WHERE book_id = ? AND format = ?",
            params![size as i64, id, format],
        )?;
        self.conn.execute("UPDATE books SET modified_at = ? WHERE id = ?", params![now(), id])?;
        Ok(())
    }

    fn add_external_book(&mut self, added: &AddedBook, fields: &HashMap<String, (i64, Datatype)>) -> Result<BookId> {
        let b = &added.book;
        let uuid = b.uuid.clone().filter(|u| {
            self.conn
                .query_row("SELECT 1 FROM books WHERE uuid = ?", [u], |_| Ok(()))
                .optional()
                .ok()
                .flatten()
                .is_none()
        });
        let new = NewBook {
            title: b.title.clone(),
            title_sort: b.sort.clone(),
            authors: b.authors.clone(),
            author_sorts: Default::default(),
            series: b.series.clone().map(|s| (s, b.series_index)),
            publisher: b.publisher.clone(),
            published: b.pubdate.map(|d| d.date_naive()),
            description_html: b.comments.as_deref().map(html::sanitize),
            tags: b.tags.clone(),
            languages: b.languages.clone(),
            identifiers: b.identifiers.clone(),
            rating: b.rating.filter(|&r| r > 0),
            uuid,
            added_at: b.timestamp,
        };
        let files: Vec<(String, std::path::PathBuf)> =
            added.files.iter().map(|(f, p)| (f.to_uppercase(), p.clone())).collect();
        let stored = store_book(self, &new, &files, added.cover.as_deref().map(Cover::File), |tx, id| {
            insert_custom_values(tx, id, b, fields)
        })?;
        Ok(stored.id)
    }

    /// Lookup name ("#genre") → field id and type, for every custom field.
    fn custom_field_map(&self) -> Result<HashMap<String, (i64, Datatype)>> {
        Ok(custom_columns(&self.conn)?.into_iter().map(|c| (c.lookup_name(), (c.id, c.datatype))).collect())
    }
}

impl Line {
    fn text(&self) -> String {
        match self {
            Line::Field(text) => text.clone(),
            Line::Added { title, formats, .. } if formats.is_empty() => format!("Added “{title}”"),
            Line::Added { title, formats, .. } => format!("Added “{title}” ({})", formats.join(", ")),
            Line::Updated { title, parts, .. } => format!("“{title}”: {}", parts.join(", ")),
        }
    }
}

/// What changed in a book, for the report: "title", "tags", "EPUB file replaced", "new cover".
fn describe(change: &ChangedBook) -> Vec<String> {
    let mut parts = change.fields.fields();
    for f in &change.files {
        parts.push(match f {
            FileChange::Added { format, .. } => format!("{format} file added"),
            FileChange::Replaced { format, .. } => format!("{format} file replaced"),
            FileChange::Removed { format } => format!("{format} file removed"),
        });
    }
    match change.cover {
        Some(CoverChange::Set(_)) => parts.push("new cover".into()),
        Some(CoverChange::Removed) => parts.push("cover removed".into()),
        None => {}
    }
    parts
}

/// Whether any field the edit form holds changed (custom fields go separately).
fn has_book_edit(c: &BookChanges) -> bool {
    BookChanges { custom: Default::default(), ..c.clone() } != BookChanges::default()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use chrono::NaiveDate;
    use magnitide_catalog::{Book, CustomColumn};

    use super::*;

    fn library() -> (tempfile::TempDir, Library, BookId) {
        let dir = tempfile::tempdir().unwrap();
        let mut lib = Library::create(dir.path().join("lib"), "Test").unwrap();
        let src = dir.path().join("Dune.epub");
        fs::write(&src, "original epub").unwrap();
        let id = store_book(
            &mut lib,
            &NewBook { title: "Dune".into(), authors: vec!["Frank Herbert".into()], ..Default::default() },
            &[("EPUB".into(), src)],
            Some(Cover::Bytes(&jpeg())),
            |_, _| Ok(()),
        )
        .unwrap()
        .id;
        (dir, lib, id)
    }

    fn jpeg() -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        image::RgbImage::new(2, 3).write_to(&mut out, image::ImageFormat::Jpeg).unwrap();
        out.into_inner()
    }

    fn pages_column() -> CustomColumn {
        CustomColumn {
            id: 50,
            label: "pages".into(),
            name: "Pages".into(),
            datatype: Datatype::Int,
            is_multiple: false,
            normalized: false,
            editable: true,
            display: serde_json::json!({"number_format": null}),
        }
    }

    #[test]
    fn applies_fields_custom_values_files_and_cover() {
        let (dir, mut lib, id) = library();
        let new_epub = dir.path().join("rewritten.epub");
        fs::write(&new_epub, "epub with page count").unwrap();
        let txt = dir.path().join("notes.txt");
        fs::write(&txt, "notes").unwrap();
        let cover = dir.path().join("cover.png");
        fs::write(&cover, {
            let mut out = std::io::Cursor::new(Vec::new());
            image::RgbImage::new(4, 4).write_to(&mut out, image::ImageFormat::Png).unwrap();
            out.into_inner()
        })
        .unwrap();

        let changes = Changes {
            new_columns: vec![pages_column()],
            books: vec![ChangedBook {
                id,
                fields: BookChanges {
                    title: Some("Dune Messiah".into()),
                    tags: Some(vec!["Science Fiction".into()]),
                    series: Some(Some(("Dune".into(), 2.0))),
                    published: Some(NaiveDate::from_ymd_opt(1969, 1, 1)),
                    comments: Some(Some("<p>Second <script>x</script>book</p>".into())),
                    custom: BTreeMap::from([("#pages".into(), Some(CustomValue::Int(256)))]),
                    ..Default::default()
                },
                files: vec![
                    FileChange::Replaced { format: "EPUB".into(), path: new_epub },
                    FileChange::Added { format: "TXT".into(), path: txt },
                ],
                cover: Some(CoverChange::Set(cover)),
            }],
            ..Default::default()
        };
        let report = lib.apply_changes(&changes, |_| Ok(())).unwrap();
        assert_eq!((report.updated.as_slice(), report.fields_added), (&[id][..], 1), "{report:?}");
        assert_eq!(
            report.details,
            [
                "New field: Pages (#pages)",
                "“Dune Messiah”: title, series, tags, description, published, #pages, EPUB file replaced, TXT file added, new cover"
            ]
        );
        assert!(report.failed.is_empty(), "{report:?}");

        let b = lib.book(id).unwrap();
        assert_eq!(b.title, "Dune Messiah");
        assert_eq!(b.title_sort, "Dune Messiah", "derived again for the new title");
        assert_eq!(b.path, format!("Frank Herbert/Dune Messiah ({id})"), "folder follows the title");
        assert_eq!(b.series, Some(("Dune".into(), 2.0)));
        assert_eq!(b.published, NaiveDate::from_ymd_opt(1969, 1, 1));
        assert!(!b.description_html.unwrap().contains("script"), "sanitized");
        let dir = lib.root().join(&b.path);
        assert_eq!(fs::read_to_string(dir.join("Dune Messiah - Frank Herbert.epub")).unwrap(), "epub with page count");
        assert_eq!(fs::read_to_string(dir.join("Dune Messiah - Frank Herbert.txt")).unwrap(), "notes");
        assert!(fs::read(dir.join("cover.jpg")).unwrap().starts_with(&[0xFF, 0xD8, 0xFF]), "stored as JPEG");
        let cat = lib.load_catalog().unwrap();
        assert_eq!(cat.get(id).unwrap().custom.get("#pages"), Some(&CustomValue::Int(256)));

        // Clearing a custom value.
        let clear = Changes {
            books: vec![ChangedBook {
                id,
                fields: BookChanges { custom: BTreeMap::from([("#pages".into(), None)]), ..Default::default() },
                files: vec![FileChange::Removed { format: "TXT".into() }],
                cover: Some(CoverChange::Removed),
            }],
            ..Default::default()
        };
        let mut discarded = Vec::new();
        lib.apply_changes(&clear, |p| {
            discarded.push(p.to_path_buf());
            fs::remove_file(p)
        })
        .unwrap();
        let cat = lib.load_catalog().unwrap();
        let b = cat.get(id).unwrap();
        assert!(b.custom.is_empty() && !b.has_cover);
        assert_eq!(b.formats.iter().map(|f| f.format.as_str()).collect::<Vec<_>>(), ["EPUB"]);
        assert_eq!(discarded.len(), 1);
    }

    #[test]
    fn adds_new_books_and_reports_removed_ones() {
        let (dir, mut lib, id) = library();
        let epub = dir.path().join("merged.epub");
        fs::write(&epub, "merged").unwrap();
        let book: Book = Book {
            id: 999,
            title: "Dune Omnibus".into(),
            sort: None,
            authors: vec!["Frank Herbert".into()],
            author_sort: None,
            series: None,
            series_index: 1.0,
            series_end: None,
            more_series: vec![],
            collects: vec![],
            collected_in: vec![],
            tags: vec!["Merged".into()],
            shelves: vec![],
            publisher: None,
            rating: None,
            languages: vec![],
            identifiers: BTreeMap::new(),
            comments: None,
            timestamp: None,
            pubdate: None,
            last_modified: None,
            path: "Frank Herbert/Dune Omnibus (999)".into(),
            uuid: None,
            has_cover: false,
            formats: vec![],
            custom: BTreeMap::from([("#pages".into(), CustomValue::Int(900))]),
        };
        let changes = Changes {
            new_columns: vec![pages_column()],
            added: vec![AddedBook { book, files: vec![("EPUB".into(), epub)], cover: None }],
            removed: vec![id, 12345],
            ..Default::default()
        };
        let report = lib.apply_changes(&changes, |_| Ok(())).unwrap();
        assert_eq!(report.added.len(), 1, "{report:?}");
        assert_eq!(report.details.last().unwrap(), "Added “Dune Omnibus” (EPUB)");
        assert_eq!(report.added_from, [(999, report.added[0])]);
        assert_eq!(report.removed, [id], "only books still here; and not deleted");
        assert!(lib.book(id).is_ok());
        let cat = lib.load_catalog().unwrap();
        let new = cat.get(report.added[0]).unwrap();
        assert_eq!((new.title.as_str(), new.tags.as_slice()), ("Dune Omnibus", &["Merged".to_owned()][..]));
        assert_eq!(new.custom.get("#pages"), Some(&CustomValue::Int(900)));
        assert_eq!(new.formats.len(), 1);
        let _: PathBuf = new.format_path(lib.root(), &new.formats[0]);
    }

    #[test]
    fn a_run_reports_each_book_once() {
        let (dir, mut lib, id) = library();
        let epub = dir.path().join("merged.epub");
        fs::write(&epub, "merged").unwrap();
        let mut new = lib.load_catalog().unwrap().get(id).unwrap().clone();
        (new.id, new.title, new.formats) = (500, "Untitled Merge".into(), vec![]);
        // First sync point: the plugin made a book and edited Dune.
        let mut run = lib
            .apply_changes(
                &Changes {
                    added: vec![AddedBook { book: new, files: vec![], cover: None }],
                    books: vec![ChangedBook {
                        id,
                        fields: BookChanges { tags: Some(vec!["Classic".into()]), ..Default::default() },
                        files: vec![],
                        cover: None,
                    }],
                    ..Default::default()
                },
                |_| Ok(()),
            )
            .unwrap();
        let added = run.added[0];
        // Then, after the user named it, a file for it, and Dune again.
        let later = lib
            .apply_changes(
                &Changes {
                    books: vec![
                        ChangedBook {
                            id: added,
                            fields: BookChanges { title: Some("The Collected Dune".into()), ..Default::default() },
                            files: vec![FileChange::Added { format: "EPUB".into(), path: epub }],
                            cover: None,
                        },
                        ChangedBook {
                            id,
                            fields: BookChanges { publisher: Some(Some("Ace".into())), ..Default::default() },
                            files: vec![],
                            cover: None,
                        },
                    ],
                    ..Default::default()
                },
                |_| Ok(()),
            )
            .unwrap();
        run.absorb(later);
        assert_eq!((run.updated.as_slice(), run.added.as_slice()), (&[id][..], &[added][..]));
        assert_eq!(run.details, ["“Dune”: tags, publisher", "Added “The Collected Dune” (EPUB)"]);
    }

    #[test]
    fn a_failing_book_is_reported_and_others_apply() {
        let (_dir, mut lib, id) = library();
        let changes = Changes {
            books: vec![
                ChangedBook {
                    id: 4242,
                    fields: BookChanges { title: Some("Nope".into()), ..Default::default() },
                    files: vec![],
                    cover: None,
                },
                ChangedBook {
                    id,
                    fields: BookChanges { tags: Some(vec!["Classic".into()]), ..Default::default() },
                    files: vec![],
                    cover: None,
                },
            ],
            ..Default::default()
        };
        let report = lib.apply_changes(&changes, |_| Ok(())).unwrap();
        assert_eq!(report.updated, [id]);
        assert_eq!(report.failed.len(), 1, "{report:?}");
        assert_eq!(lib.book(id).unwrap().tags, ["Classic"]);
    }
}
