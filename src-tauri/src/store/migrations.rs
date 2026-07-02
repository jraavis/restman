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
    // v3 — auth. Static config is embedded JSON on the owning row, same
    // convention as requests.headers_json/body_json/etc; collections store a
    // plain AuthConfig (default none), requests store a RequestAuth
    // (inherit | own, default inherit). Mutable OAuth token *state* lives
    // separately so a background refresh never has to rewrite the user's
    // saved auth config — at most one token row per owner (see the auth
    // module, which enforces this with delete-then-insert on every write).
    r#"
    ALTER TABLE collections ADD COLUMN auth_json TEXT NOT NULL DEFAULT '{"type":"none"}';
    ALTER TABLE requests ADD COLUMN auth_json TEXT NOT NULL DEFAULT '{"mode":"inherit"}';

    CREATE TABLE oauth_tokens (
        id                TEXT PRIMARY KEY,
        collection_id     TEXT REFERENCES collections(id) ON DELETE CASCADE,
        request_id        TEXT REFERENCES requests(id) ON DELETE CASCADE,
        token_type        TEXT NOT NULL DEFAULT 'Bearer',
        scope             TEXT,
        expires_at        INTEGER,
        has_refresh_token INTEGER NOT NULL DEFAULT 0,
        obtained_at       INTEGER NOT NULL,
        updated_at        INTEGER NOT NULL,
        CHECK ((collection_id IS NOT NULL) + (request_id IS NOT NULL) = 1)
    );
    CREATE INDEX idx_oauth_tokens_collection ON oauth_tokens(collection_id);
    CREATE INDEX idx_oauth_tokens_request ON oauth_tokens(request_id);
    "#,
    // v4 — scripting. Pre/post scripts stored on requests; test results
    // stored on history rows so the runner can show past pass/fail state.
    // The oauth_tokens table gains a masked_preview column so the frontend
    // can display a truncated token string without the raw value ever
    // crossing IPC.
    r#"
    ALTER TABLE requests
        ADD COLUMN pre_request_script  TEXT NOT NULL DEFAULT '';
    ALTER TABLE requests
        ADD COLUMN post_response_script TEXT NOT NULL DEFAULT '';
    ALTER TABLE history
        ADD COLUMN test_results_json TEXT;
    ALTER TABLE oauth_tokens
        ADD COLUMN masked_preview TEXT;
    "#,
    // v5 — per-workspace transport settings (proxy, default headers, mTLS
    // client cert). One row per workspace; secrets (pasted PEM bytes +
    // passphrase) live in the keychain and only their keychain slot names are
    // reflected in `client_cert_json`. Same mask-on-write contract as auth.
    r#"
    CREATE TABLE workspace_settings (
        workspace_id        TEXT PRIMARY KEY REFERENCES workspaces(id) ON DELETE CASCADE,
        proxy_url           TEXT,
        proxy_bypass        TEXT,
        default_headers_json TEXT NOT NULL DEFAULT '[]',
        client_cert_json    TEXT NOT NULL DEFAULT '{"mode":"none"}'
    );
    "#,
    // v6 — user-authored JS plugins (custom code-generators, custom
    // import/export formats), sandbox-executed rather than compiled into the
    // Rust binary. Many rows per workspace, same shape as tags/environments
    // rather than the one-row-per-workspace `workspace_settings` singleton.
    // `kind` distinguishes codegen vs import vs export; `language_label` is a
    // free-form display label (language name for codegen, format name for
    // import/export). Timestamps are Unix milliseconds, matching every other
    // table in this schema (see `crate::util::now_millis`).
    r#"
    CREATE TABLE plugins (
        id              TEXT PRIMARY KEY,
        workspace_id    TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
        name            TEXT NOT NULL,
        kind            TEXT NOT NULL,
        language_label  TEXT NOT NULL,
        source          TEXT NOT NULL,
        enabled         INTEGER NOT NULL DEFAULT 1,
        created_at      INTEGER NOT NULL,
        updated_at      INTEGER NOT NULL
    );
    CREATE INDEX idx_plugins_workspace ON plugins(workspace_id);
    "#,
    // v7 — local mock servers. Workspace-scoped, many rows per workspace
    // (same shape as plugins), each server owning an ordered list of rules
    // matched method+path -> canned response. `port` is user-configured and
    // fixed (unlike the engine's test spike, which binds an OS-assigned port
    // purely to avoid test-to-test collisions) — a mock server is only
    // useful if it answers on a predictable address the user can point a
    // client at. Running state (the live socket/task) is ephemeral and lives
    // in `AppState.mock_servers`, not here — restarting the app never
    // auto-starts a server, same manual-control posture as the SSE/WS/gRPC
    // panels.
    r#"
    CREATE TABLE mock_servers (
        id           TEXT PRIMARY KEY,
        workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
        name         TEXT NOT NULL,
        port         INTEGER NOT NULL,
        created_at   INTEGER NOT NULL,
        updated_at   INTEGER NOT NULL
    );
    CREATE INDEX idx_mock_servers_workspace ON mock_servers(workspace_id);

    CREATE TABLE mock_rules (
        id             TEXT PRIMARY KEY,
        mock_server_id TEXT NOT NULL REFERENCES mock_servers(id) ON DELETE CASCADE,
        method         TEXT,
        path_pattern   TEXT NOT NULL,
        status         INTEGER NOT NULL DEFAULT 200,
        headers_json   TEXT NOT NULL DEFAULT '[]',
        body           TEXT NOT NULL DEFAULT '',
        delay_ms       INTEGER NOT NULL DEFAULT 0,
        sort_order     INTEGER NOT NULL DEFAULT 0
    );
    CREATE INDEX idx_mock_rules_server ON mock_rules(mock_server_id);
    "#,
    // v8 — file-based `.restman/` sync config, per workspace. One-directional
    // (DB is always the source of truth): `manual` means the user triggers
    // export/import explicitly; `live` means the app re-exports to
    // `sync_folder_path` automatically after every relevant mutation (see
    // `crate::sync`). There is deliberately no filesystem watcher pulling
    // external edits back in — that would need a conflict-resolution engine
    // this phase doesn't build; import always stays a manual, explicit action.
    r#"
    ALTER TABLE workspace_settings ADD COLUMN sync_folder_path TEXT;
    ALTER TABLE workspace_settings ADD COLUMN sync_mode TEXT NOT NULL DEFAULT 'off';
    ALTER TABLE workspace_settings ADD COLUMN sync_format TEXT NOT NULL DEFAULT 'json';
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
