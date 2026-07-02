//! Database connection setup.

use crate::error::AppResult;
use rusqlite::Connection;
use std::path::Path;

use super::migrations;

/// Open (or create) the database at `path`, apply pragmas and run migrations.
pub fn open(path: &Path) -> AppResult<Connection> {
    let mut conn = Connection::open(path)?;
    configure(&conn)?;
    migrations::run(&mut conn)?;
    Ok(conn)
}

/// Open an in-memory database (used by tests).
#[cfg(test)]
pub fn open_in_memory() -> AppResult<Connection> {
    let mut conn = Connection::open_in_memory()?;
    configure(&conn)?;
    migrations::run(&mut conn)?;
    Ok(conn)
}

/// `pub(crate)`: `crate::backup::restore_backup` re-applies this after an
/// online-backup restore, since backing up *into* a connection overwrites
/// its database-file header (including the persisted WAL setting) with the
/// source's — see that module for why this matters.
pub(crate) fn configure(conn: &Connection) -> AppResult<()> {
    // WAL improves concurrency; setting it returns a row, so use query_row.
    let _: String = conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))?;
    conn.execute_batch("PRAGMA foreign_keys=ON;")?;
    Ok(())
}
