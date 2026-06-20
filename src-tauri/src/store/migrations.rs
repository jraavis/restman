//! Schema migrations.
//!
//! Migrations are an ordered list of SQL batches. The current schema version is
//! tracked via SQLite's `PRAGMA user_version`. To evolve the schema, append a
//! new entry — never edit an existing one. Later phases (collections,
//! environments, history, FTS5 search) add their tables here.

use crate::error::AppResult;
use rusqlite::Connection;

const MIGRATIONS: &[&str] = &[
    // v1 — workspaces (top-level container) + key/value settings.
    r#"
    CREATE TABLE workspaces (
        id         TEXT PRIMARY KEY,
        name       TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        is_active  INTEGER NOT NULL DEFAULT 0
    );

    CREATE TABLE settings (
        key   TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );
    "#,
];

/// Apply any migrations newer than the database's current `user_version`.
///
/// Each migration runs inside a transaction that also bumps `user_version`
/// (a transactional part of the DB header), so a migration is applied
/// all-or-nothing — a mid-batch failure rolls back cleanly and is retried on
/// next launch rather than leaving a half-applied schema.
pub fn run(conn: &mut Connection) -> AppResult<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    let mut version = current as usize;
    while version < MIGRATIONS.len() {
        let tx = conn.transaction()?;
        tx.execute_batch(MIGRATIONS[version])?;
        tx.pragma_update(None, "user_version", (version + 1) as i64)?;
        tx.commit()?;
        version += 1;
    }
    Ok(())
}
