//! Reading positions synced from e-readers, matched to the library's books.
//!
//! KOReader's progress sync names a book by a fingerprint of the file the
//! reader opened ([`crate::fingerprint`]), nothing the library knows a book
//! by. So the library keeps the fingerprints of its own files (taken when
//! first asked for, again once a file changes) and of every copy it hands
//! out: a copy sent to a device has the library's metadata written into it,
//! and one downloaded for a Kobo is converted to KEPUB on the way, so each
//! has a fingerprint of its own. A synced position whose document matches
//! any of them moves that book's reading state on, when it's newer than
//! what the library already holds.

use std::collections::HashMap;
use std::time::UNIX_EPOCH;

use chrono::{DateTime, Utc};
use rusqlite::{OptionalExtension, params};

use crate::error::Result;
use crate::fingerprint::partial_md5;
use crate::library::{BookId, LOCAL_USER, Library, now, timestamp};
use crate::pages::ReadingState;

/// A position as a reader synced it.
#[derive(Debug, Clone, PartialEq)]
pub struct SyncedPosition {
    /// The reader's fingerprint of the file it opened.
    pub document: String,
    /// 0–1 through the book.
    pub progress: f64,
    /// When the reader synced it.
    pub at: DateTime<Utc>,
    /// The reader's name for itself ("Kobo Libra 2"), if it gave one.
    pub device: Option<String>,
}

/// How a copy of a file left the library.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Via {
    /// Sent to a connected e-reader.
    Device,
    /// Downloaded from the library server.
    Server,
}

impl Via {
    fn as_str(self) -> &'static str {
        match self {
            Via::Device => "device",
            Via::Server => "server",
        }
    }
}

/// A book whose reading state a synced position moved on.
#[derive(Debug, Clone, PartialEq)]
pub struct Applied {
    pub book: BookId,
    pub state: ReadingState,
}

impl Library {
    /// Remember that a copy of `book`'s `format` file left the library with
    /// this fingerprint, so a position synced from it finds the book.
    pub fn note_copy(
        &mut self,
        book: BookId,
        format: &str,
        fingerprint: &str,
        via: Via,
        target: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO file_copies (fingerprint, book_id, format, via, target, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (fingerprint, book_id) DO UPDATE SET
               format = excluded.format, via = excluded.via, target = excluded.target, at = excluded.at",
            params![fingerprint, book, format, via.as_str(), target, now()],
        )?;
        Ok(())
    }

    /// Every fingerprint the library knows a book by: its files' own (taken
    /// now where missing or stale; a file missing from disk has none) and
    /// those of the copies handed out.
    pub fn fingerprints(&mut self) -> Result<HashMap<String, BookId>> {
        let mut known = HashMap::new();
        let mut stale = Vec::new();
        {
            let mut stmt = self.conn.prepare(
                "SELECT f.id, f.book_id, b.path, f.filename, f.fingerprint, f.fingerprint_mtime
                 FROM files f JOIN books b ON b.id = f.book_id",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, BookId>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, Option<i64>>(5)?,
                ))
            })?;
            for row in rows {
                let (file_id, book, dir, filename, fingerprint, taken_at) = row?;
                let path = dir.split('/').fold(self.root.clone(), |p, c| p.join(c)).join(filename);
                let Some(mtime) = mtime_of(&path) else { continue };
                match fingerprint {
                    Some(fp) if taken_at == Some(mtime) => {
                        known.insert(fp, book);
                    }
                    _ => stale.push((file_id, book, path, mtime)),
                }
            }
        }
        if !stale.is_empty() {
            let tx = self.conn.transaction()?;
            for (file_id, book, path, mtime) in stale {
                let Ok(fp) = partial_md5(&path) else { continue };
                tx.execute(
                    "UPDATE files SET fingerprint = ?1, fingerprint_mtime = ?2 WHERE id = ?3",
                    params![fp, mtime, file_id],
                )?;
                known.insert(fp, book);
            }
            tx.commit()?;
        }
        let mut stmt = self.conn.prepare("SELECT fingerprint, book_id FROM file_copies")?;
        for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, BookId>(1)?)))? {
            let (fp, book) = row?;
            known.entry(fp).or_insert(book);
        }
        Ok(known)
    }

    /// Match `positions` to books, and move each matched book's reading
    /// state on to its newest position, where that's later than the last
    /// the library knew of (so an older sync, or one seen before, changes
    /// nothing). At the end of the book, the book is read.
    pub fn apply_positions(&mut self, positions: &[SyncedPosition]) -> Result<Vec<Applied>> {
        if positions.is_empty() {
            return Ok(Vec::new());
        }
        let known = self.fingerprints()?;
        // One copy per reader, or several readers: the newest position wins.
        let mut newest: HashMap<BookId, &SyncedPosition> = HashMap::new();
        for p in positions {
            if let Some(&book) = known.get(&p.document) {
                let entry = newest.entry(book).or_insert(p);
                if p.at > entry.at {
                    *entry = p;
                }
            }
        }
        let mut matched: Vec<(BookId, &SyncedPosition)> = newest.into_iter().collect();
        matched.sort_by_key(|(book, _)| *book);

        let mut moved = Vec::new();
        let tx = self.conn.transaction()?;
        for (book, p) in matched {
            let last: Option<String> = tx
                .query_row(
                    "SELECT last_read_at FROM user_books WHERE user_id = ?1 AND book_id = ?2",
                    params![LOCAL_USER, book],
                    |r| r.get(0),
                )
                .optional()?
                .flatten();
            let at = timestamp(p.at);
            if last.is_some_and(|l| l >= at) {
                continue;
            }
            let progress = p.progress.clamp(0.0, 1.0);
            let status = if progress >= 1.0 { "read" } else { "reading" };
            tx.execute(
                "INSERT INTO user_books (user_id, book_id, status, progress, last_read_at, device)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT (user_id, book_id) DO UPDATE SET
                   status = CASE WHEN ?3 = 'read' OR status = 'read' THEN 'read' ELSE 'reading' END,
                   progress = ?4, last_read_at = ?5, device = ?6",
                params![LOCAL_USER, book, status, progress, at, p.device],
            )?;
            moved.push(book);
        }
        tx.commit()?;
        moved.into_iter().map(|book| Ok(Applied { book, state: self.reading_state(book)? })).collect()
    }

    /// The reading state of every book that has one (being read, read, or
    /// with a position), for showing across the library.
    pub fn reading_states(&self) -> Result<Vec<(BookId, ReadingState)>> {
        let mut stmt = self.conn.prepare(
            "SELECT book_id, status, progress, last_read_at, device FROM user_books
             WHERE user_id = ?1 AND (status != 'unread' OR progress IS NOT NULL) ORDER BY book_id",
        )?;
        let rows = stmt.query_map([LOCAL_USER], |r| {
            Ok((
                r.get(0)?,
                ReadingState { status: r.get(1)?, progress: r.get(2)?, last_read_at: r.get(3)?, device: r.get(4)? },
            ))
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}

/// The file's modification time in whole seconds, if it's there.
fn mtime_of(path: &std::path::Path) -> Option<i64> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    Some(modified.duration_since(UNIX_EPOCH).ok()?.as_secs() as i64)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use chrono::TimeZone;

    use super::*;
    use crate::import::files::add_files;
    use crate::metadata::epub::tests::{EPUB2_OPF, make_epub};

    fn library_with_epub() -> (tempfile::TempDir, Library, BookId, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("The Dispossessed - Ursula K. Le Guin.epub");
        make_epub(&src, EPUB2_OPF, None);
        let mut lib = Library::create(tmp.path().join("lib"), "Test").unwrap();
        let id = add_files(&mut lib, &[src], |_| {}).unwrap().added[0];
        let catalog = lib.load_catalog().unwrap();
        let book = catalog.get(id).unwrap();
        let path = book.format_path(lib.root(), &book.formats[0]);
        (tmp, lib, id, path)
    }

    fn at(secs: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(secs, 0).unwrap()
    }

    fn position(document: &str, progress: f64, secs: i64, device: &str) -> SyncedPosition {
        SyncedPosition { document: document.into(), progress, at: at(secs), device: Some(device.into()) }
    }

    #[test]
    fn library_files_are_fingerprinted_once_and_again_when_they_change() {
        let (_tmp, mut lib, id, path) = library_with_epub();
        let fp = partial_md5(&path).unwrap();
        assert_eq!(lib.fingerprints().unwrap().get(&fp), Some(&id));
        let stored: (String, i64) = lib
            .conn
            .query_row("SELECT fingerprint, fingerprint_mtime FROM files WHERE book_id = ?", [id], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(stored.0, fp);

        // The file changes (and so does its modification time): a new fingerprint.
        let mut bytes = std::fs::read(&path).unwrap();
        bytes.extend_from_slice(b"trailing junk that changes the samples");
        std::fs::write(&path, &bytes).unwrap();
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
        std::fs::File::options().write(true).open(&path).unwrap().set_modified(later).unwrap();
        let fp2 = partial_md5(&path).unwrap();
        assert_ne!(fp2, fp);
        let known = lib.fingerprints().unwrap();
        assert_eq!(known.get(&fp2), Some(&id));
        assert!(!known.contains_key(&fp));

        // A file missing from disk has none, and nothing fails.
        std::fs::remove_file(&path).unwrap();
        assert!(lib.fingerprints().unwrap().is_empty());
    }

    #[test]
    fn copies_handed_out_find_their_book() {
        let (_tmp, mut lib, id, _path) = library_with_epub();
        lib.note_copy(id, "KEPUB", "abc123", Via::Device, Some("Kobo Libra 2")).unwrap();
        lib.note_copy(id, "EPUB", "def456", Via::Server, Some("KOReader")).unwrap();
        // Noting the same copy again is fine.
        lib.note_copy(id, "KEPUB", "abc123", Via::Device, Some("Kobo Libra 2")).unwrap();
        let known = lib.fingerprints().unwrap();
        assert_eq!((known.get("abc123"), known.get("def456")), (Some(&id), Some(&id)));
        let via: Vec<(String, String, Option<String>)> = lib
            .conn
            .prepare("SELECT format, via, target FROM file_copies ORDER BY fingerprint")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(
            via,
            vec![
                ("KEPUB".into(), "device".into(), Some("Kobo Libra 2".into())),
                ("EPUB".into(), "server".into(), Some("KOReader".into()))
            ]
        );
    }

    #[test]
    fn positions_move_the_book_on_only_when_newer() {
        let (_tmp, mut lib, id, path) = library_with_epub();
        let fp = partial_md5(&path).unwrap();
        lib.note_copy(id, "KEPUB", "sent-copy", Via::Device, Some("Kobo Libra 2")).unwrap();

        // Nothing matches: nothing happens.
        assert!(lib.apply_positions(&[position("unknown", 0.5, 1_000, "Kobo")]).unwrap().is_empty());
        assert_eq!(lib.reading_state(id).unwrap().status, "unread");

        // The sent copy's position, and an older one from the library file itself: the newest wins.
        let moved = lib
            .apply_positions(&[position(&fp, 0.2, 900, "Phone"), position("sent-copy", 0.42, 1_000, "Kobo Libra 2")])
            .unwrap();
        assert_eq!(moved.len(), 1);
        let state = &moved[0].state;
        assert_eq!((moved[0].book, state.status.as_str(), state.progress), (id, "reading", Some(0.42)));
        assert_eq!(state.device.as_deref(), Some("Kobo Libra 2"));
        assert_eq!(state.last_read_at.as_deref(), Some("1970-01-01T00:16:40Z"));

        // The same sync again, or an older one, changes nothing.
        assert!(lib.apply_positions(&[position("sent-copy", 0.42, 1_000, "Kobo Libra 2")]).unwrap().is_empty());
        assert!(lib.apply_positions(&[position("sent-copy", 0.1, 500, "Kobo Libra 2")]).unwrap().is_empty());
        assert_eq!(lib.reading_state(id).unwrap().progress, Some(0.42));

        // A newer one does; at the end, the book is read, and stays read.
        let moved = lib.apply_positions(&[position(&fp, 1.0, 2_000, "Phone")]).unwrap();
        assert_eq!((moved[0].state.status.as_str(), moved[0].state.device.as_deref()), ("read", Some("Phone")));
        let moved = lib.apply_positions(&[position(&fp, 0.3, 3_000, "Phone")]).unwrap();
        assert_eq!((moved[0].state.status.as_str(), moved[0].state.progress), ("read", Some(0.3)));

        // The app's own reader is newer still, and names no device.
        let state = lib.set_reading_progress(id, 0.5).unwrap();
        assert_eq!((state.device, state.progress), (None, Some(0.5)));
        assert!(lib.apply_positions(&[position(&fp, 0.6, 3_500, "Phone")]).unwrap().is_empty());

        let states = lib.reading_states().unwrap();
        assert_eq!(states.len(), 1);
        assert_eq!((states[0].0, states[0].1.status.as_str()), (id, "read"));
    }
}
