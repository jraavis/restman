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
    // v2 — collections/folders, requests, tags, environments, variables,
    // history, tabs, plus FTS5 search over requests.
    r#"
    CREATE TABLE collections (
        id           TEXT PRIMARY KEY,
        workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
        parent_id    TEXT REFERENCES collections(id) ON DELETE CASCADE,
        name         TEXT NOT NULL,
        description  TEXT,
        sort_order   INTEGER NOT NULL DEFAULT 0,
        created_at   INTEGER NOT NULL,
        updated_at   INTEGER NOT NULL
    );
    CREATE INDEX idx_collections_workspace ON collections(workspace_id);
    CREATE INDEX idx_collections_parent ON collections(parent_id);

    CREATE TABLE requests (
        id            TEXT PRIMARY KEY,
        collection_id TEXT NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
        name          TEXT NOT NULL,
        method        TEXT NOT NULL DEFAULT 'GET',
        url           TEXT NOT NULL DEFAULT '',
        headers_json  TEXT NOT NULL DEFAULT '[]',
        query_json    TEXT NOT NULL DEFAULT '[]',
        body_json     TEXT NOT NULL DEFAULT '{"mode":"none"}',
        options_json  TEXT NOT NULL DEFAULT '{}',
        sort_order    INTEGER NOT NULL DEFAULT 0,
        created_at    INTEGER NOT NULL,
        updated_at    INTEGER NOT NULL,
        last_used_at  INTEGER
    );
    CREATE INDEX idx_requests_collection ON requests(collection_id);

    CREATE VIRTUAL TABLE requests_fts USING fts5(
        name, url, method,
        content='requests', content_rowid='rowid'
    );
    CREATE TRIGGER requests_fts_ai AFTER INSERT ON requests BEGIN
        INSERT INTO requests_fts(rowid, name, url, method) VALUES (new.rowid, new.name, new.url, new.method);
    END;
    CREATE TRIGGER requests_fts_ad AFTER DELETE ON requests BEGIN
        INSERT INTO requests_fts(requests_fts, rowid, name, url, method) VALUES('delete', old.rowid, old.name, old.url, old.method);
    END;
    CREATE TRIGGER requests_fts_au AFTER UPDATE ON requests BEGIN
        INSERT INTO requests_fts(requests_fts, rowid, name, url, method) VALUES('delete', old.rowid, old.name, old.url, old.method);
        INSERT INTO requests_fts(rowid, name, url, method) VALUES (new.rowid, new.name, new.url, new.method);
    END;

    CREATE TABLE tags (
        id           TEXT PRIMARY KEY,
        workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
        name         TEXT NOT NULL,
        color        TEXT NOT NULL DEFAULT '#64748b'
    );
    CREATE INDEX idx_tags_workspace ON tags(workspace_id);

    CREATE TABLE request_tags (
        request_id TEXT NOT NULL REFERENCES requests(id) ON DELETE CASCADE,
        tag_id     TEXT NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
        PRIMARY KEY (request_id, tag_id)
    );

    CREATE TABLE environments (
        id            TEXT PRIMARY KEY,
        workspace_id  TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
        collection_id TEXT REFERENCES collections(id) ON DELETE CASCADE,
        name          TEXT NOT NULL,
        group_name    TEXT,
        is_active     INTEGER NOT NULL DEFAULT 0,
        created_at    INTEGER NOT NULL,
        updated_at    INTEGER NOT NULL
    );
    CREATE INDEX idx_environments_workspace ON environments(workspace_id);
    CREATE INDEX idx_environments_collection ON environments(collection_id);

    CREATE TABLE variables (
        id             TEXT PRIMARY KEY,
        workspace_id   TEXT REFERENCES workspaces(id) ON DELETE CASCADE,
        collection_id  TEXT REFERENCES collections(id) ON DELETE CASCADE,
        environment_id TEXT REFERENCES environments(id) ON DELETE CASCADE,
        key            TEXT NOT NULL,
        value          TEXT NOT NULL DEFAULT '',
        var_type       TEXT NOT NULL DEFAULT 'string',
        is_secret      INTEGER NOT NULL DEFAULT 0,
        enabled        INTEGER NOT NULL DEFAULT 1,
        sort_order     INTEGER NOT NULL DEFAULT 0,
        created_at     INTEGER NOT NULL,
        updated_at     INTEGER NOT NULL,
        CHECK ((workspace_id IS NOT NULL) + (collection_id IS NOT NULL) + (environment_id IS NOT NULL) <= 1)
    );
    CREATE INDEX idx_variables_workspace ON variables(workspace_id);
    CREATE INDEX idx_variables_collection ON variables(collection_id);
    CREATE INDEX idx_variables_environment ON variables(environment_id);

    CREATE TABLE history (
        id            TEXT PRIMARY KEY,
        workspace_id  TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
        request_id    TEXT REFERENCES requests(id) ON DELETE SET NULL,
        name          TEXT NOT NULL,
        method        TEXT NOT NULL,
        url           TEXT NOT NULL,
        status        INTEGER,
        duration_ms   REAL,
        request_json  TEXT NOT NULL,
        response_json TEXT,
        error         TEXT,
        created_at    INTEGER NOT NULL
    );
    CREATE INDEX idx_history_workspace ON history(workspace_id, created_at DESC);

    CREATE TABLE tabs (
        id           TEXT PRIMARY KEY,
        workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
        request_id   TEXT REFERENCES requests(id) ON DELETE SET NULL,
        title        TEXT NOT NULL DEFAULT 'Untitled',
        draft_json   TEXT NOT NULL,
        sort_order   INTEGER NOT NULL DEFAULT 0,
        is_active    INTEGER NOT NULL DEFAULT 0,
        created_at   INTEGER NOT NULL,
        updated_at   INTEGER NOT NULL
    );
    CREATE INDEX idx_tabs_workspace ON tabs(workspace_id);
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
