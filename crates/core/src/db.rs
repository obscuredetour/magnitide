//! Opening the library database and keeping its schema current.

use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};

use crate::error::{Error, Result};

/// Migrations in order. Migration `n` (1-based) moves the schema from
/// version `n - 1` to `n`. Never edit a migration once released: add a new one.
const MIGRATIONS: &[&str] = &[
    include_str!("migrations/0001_initial.sql"),
    include_str!("migrations/0002_file_metadata_written.sql"),
    include_str!("migrations/0003_omnibus_series.sql"),
    include_str!("migrations/0004_shelves.sql"),
];

/// The schema version this build creates and understands.
pub const SCHEMA_VERSION: u32 = MIGRATIONS.len() as u32;

pub(crate) fn open(path: &Path, create: bool) -> Result<Connection> {
    let mut flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    if create {
        flags |= OpenFlags::SQLITE_OPEN_CREATE;
    }
    let conn = Connection::open_with_flags(path, flags)?;
    conn.busy_timeout(Duration::from_secs(5))?;
    // WAL lets readers proceed while a write is in progress; NORMAL sync is
    // durable across app crashes (not power loss mid-commit), the usual
    // trade-off for WAL.
    let mode: String = conn.pragma_update_and_check(None, "journal_mode", "WAL", |r| r.get(0))?;
    debug_assert!(mode.eq_ignore_ascii_case("wal") || mode.eq_ignore_ascii_case("memory"), "journal_mode {mode}");
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", true)?;
    Ok(conn)
}

pub(crate) fn schema_version(conn: &Connection) -> Result<u32> {
    Ok(conn.query_row("PRAGMA user_version", [], |r| r.get(0))?)
}

/// Bring the schema up to [`SCHEMA_VERSION`]. Each migration runs in its own
/// transaction together with the version bump, so a failure leaves the
/// library at the last complete version. Returns the version before migrating.
pub(crate) fn migrate(conn: &mut Connection) -> Result<u32> {
    let start = schema_version(conn)?;
    if start > SCHEMA_VERSION {
        return Err(Error::SchemaTooNew { found: start, supported: SCHEMA_VERSION });
    }
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(start as usize) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", i as u32 + 1)?;
        tx.commit()?;
    }
    Ok(start)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_fresh_database_once() {
        let mut conn = Connection::open_in_memory().unwrap();
        assert_eq!(migrate(&mut conn).unwrap(), 0);
        assert_eq!(schema_version(&conn).unwrap(), SCHEMA_VERSION);
        // Running again is a no-op.
        assert_eq!(migrate(&mut conn).unwrap(), SCHEMA_VERSION);
    }

    #[test]
    fn refuses_newer_schema() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "user_version", SCHEMA_VERSION + 1).unwrap();
        assert!(matches!(migrate(&mut conn), Err(Error::SchemaTooNew { .. })));
    }

    #[test]
    fn schema_passes_integrity_and_foreign_key_checks() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn).unwrap();
        let ok: String = conn.query_row("PRAGMA integrity_check", [], |r| r.get(0)).unwrap();
        assert_eq!(ok, "ok");
        let fk_problems: i64 =
            conn.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| r.get(0)).unwrap();
        assert_eq!(fk_problems, 0);
    }
}
