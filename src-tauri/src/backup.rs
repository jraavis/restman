//! Full-app ZIP backup/restore (Phase 8). Unlike every other export path in
//! this codebase (`interop`, `crate::sync`), a backup is meant for local
//! disaster recovery — restoring onto the *same* machine after e.g. a
//! reinstall — so it deliberately does NOT follow the mask-on-write
//! contract: it bundles every real secret this app has ever written to the
//! OS keychain (auth tokens/passwords, secret variables, workspace client
//! certs) in cleartext inside the archive. To keep that safe at rest, every
//! entry is AES-256 encrypted with a password the caller must supply — see
//! `zip`'s `aes-crypto` feature in `Cargo.toml`. There is no "skip the
//! password" path: `create_backup` refuses an empty password rather than
//! silently writing a plaintext-secrets file to disk.
//!
//! The archive holds the *entire* DB across every workspace (not scoped to
//! one, unlike `crate::sync`) plus a flat `secrets.json` of every keychain
//! entry this app could have written. Restoring replaces the live DB
//! wholesale via SQLite's online backup API (`rusqlite::backup`), which is
//! WAL-safe and needs no app restart — see `restore_backup`.

use crate::error::{AppError, AppResult};
use crate::model::auth::AuthConfig;
use crate::store::db;
use rusqlite::backup::Backup;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::io::{Cursor, Read, Write};
use std::path::PathBuf;
use std::time::Duration;
use zip::write::{SimpleFileOptions, ZipWriter};
use zip::{AesMode, CompressionMethod, ZipArchive};

const MANIFEST_ENTRY: &str = "manifest.json";
const DB_ENTRY: &str = "restman.sqlite3";
const SECRETS_ENTRY: &str = "secrets.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    restman_backup_version: u32,
    created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SecretRecord {
    key: String,
    value: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreReport {
    pub secrets_restored: usize,
    pub workspaces: usize,
    pub collections: usize,
    pub requests: usize,
    pub environments: usize,
    pub history_entries: usize,
}

/// Post-restore row counts for the report — read from the freshly-restored
/// live connection, so they reflect exactly what the user ended up with.
fn count_rows(conn: &Connection, table: &str) -> AppResult<usize> {
    // `table` is one of the fixed names below, never user input.
    let n: i64 = conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))?;
    Ok(n as usize)
}

fn temp_db_path() -> PathBuf {
    std::env::temp_dir().join(format!("restman-backup-{}.sqlite3", uuid::Uuid::new_v4()))
}

/// Every keychain key this app could plausibly have written, derived the
/// same way each domain already builds its own keys: `crate::auth::owner_key`
/// combined with `AuthConfig::secret_slots()`, `store::variables::keychain_key`,
/// and the `wscert:{workspace_id}:{slot}` scheme `workspace::build_identity`
/// and `store::workspace_settings` also use. Brute-force candidate generation
/// rather than reconstructing each domain's typed config: `secrets::get`
/// cheaply returns `None` for anything not actually present, so trying every
/// structurally-possible key is simpler and can't miss a slot that a more
/// "precise" per-variant walk might.
fn collect_secret_keys(conn: &Connection) -> AppResult<Vec<String>> {
    let mut keys = Vec::new();

    let mut collections = conn.prepare("SELECT id FROM collections")?;
    let collection_ids: Vec<String> = collections.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
    let mut requests = conn.prepare("SELECT id FROM requests")?;
    let request_ids: Vec<String> = requests.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
    for (kind, ids) in [("collection", &collection_ids), ("request", &request_ids)] {
        for id in ids {
            let owner = crate::auth::owner_key(kind, id);
            for slot in AuthConfig::secret_slots() {
                keys.push(format!("{owner}:{slot}"));
            }
            keys.push(format!("{owner}:token:access"));
            keys.push(format!("{owner}:token:refresh"));
        }
    }

    let mut secret_vars = conn.prepare("SELECT id FROM variables WHERE is_secret = 1")?;
    for id in secret_vars.query_map([], |r| r.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()? {
        keys.push(crate::store::variables::keychain_key(&id));
    }

    let mut workspaces = conn.prepare("SELECT id FROM workspaces")?;
    for id in workspaces.query_map([], |r| r.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()? {
        for slot in ["cert", "key", "pass"] {
            keys.push(format!("wscert:{id}:{slot}"));
        }
    }

    Ok(keys)
}

fn collect_real_secrets(conn: &Connection) -> AppResult<Vec<SecretRecord>> {
    let mut out = Vec::new();
    for key in collect_secret_keys(conn)? {
        if let Some(value) = crate::secrets::get(&key)? {
            out.push(SecretRecord { key, value });
        }
    }
    Ok(out)
}

/// Snapshot the live DB into a fresh on-disk SQLite file via SQLite's online
/// backup API (WAL-safe, unlike a raw `std::fs::copy` of a WAL-mode
/// database), then read it back as bytes. The temp file is deleted before
/// returning either way.
fn snapshot_db_bytes(conn: &Connection) -> AppResult<Vec<u8>> {
    let path = temp_db_path();
    let result = (|| -> AppResult<Vec<u8>> {
        let mut dest = Connection::open(&path)?;
        Backup::new(conn, &mut dest)?.run_to_completion(100, Duration::from_millis(10), None)?;
        drop(dest);
        Ok(std::fs::read(&path)?)
    })();
    std::fs::remove_file(&path).ok();
    result
}

/// Build a password-encrypted ZIP: `manifest.json`, a full snapshot of the
/// live DB (every workspace, secrets already masked in-column same as
/// always), and `secrets.json` (every real keychain value `collect_real_secrets`
/// found). Errors if `password` is empty — see module doc.
pub fn create_backup(conn: &Connection, password: &str) -> AppResult<Vec<u8>> {
    if password.is_empty() {
        return Err(AppError::Other(
            "a backup password is required — the archive bundles real secrets from the keychain".into(),
        ));
    }

    let manifest = Manifest { restman_backup_version: 1, created_at: crate::util::now_millis() };
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)?;
    let db_bytes = snapshot_db_bytes(conn)?;
    let secrets = collect_real_secrets(conn)?;
    let secrets_bytes = serde_json::to_vec(&secrets)?;

    let mut buf = Cursor::new(Vec::new());
    {
        let mut zip = ZipWriter::new(&mut buf);
        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .with_aes_encryption(AesMode::Aes256, password);
        for (name, bytes) in [(MANIFEST_ENTRY, &manifest_bytes), (DB_ENTRY, &db_bytes), (SECRETS_ENTRY, &secrets_bytes)] {
            zip.start_file(name, options).map_err(|e| AppError::Other(format!("zip write failed: {e}")))?;
            zip.write_all(bytes)?;
        }
        zip.finish().map_err(|e| AppError::Other(format!("zip finalize failed: {e}")))?;
    }
    Ok(buf.into_inner())
}

/// Replace the live DB wholesale from a backup archive, then restore every
/// bundled keychain secret verbatim under its original key (no need to
/// route through `auth::persist`/`hydrate` — the manifest already carries
/// the exact keychain key each value belongs under). `conn` is the live
/// `AppState` connection (behind its existing `Mutex` — the caller already
/// holds the lock) — the restore runs in-process via `rusqlite::backup`,
/// no app restart or file-handle juggling required.
pub fn restore_backup(conn: &mut Connection, bytes: &[u8], password: &str) -> AppResult<RestoreReport> {
    let cursor = Cursor::new(bytes);
    let mut archive = ZipArchive::new(cursor).map_err(|e| AppError::Other(format!("not a valid backup archive: {e}")))?;

    let mut read_entry = |name: &str| -> AppResult<Vec<u8>> {
        let mut file = archive
            .by_name_decrypt(name, password.as_bytes())
            .map_err(|e| AppError::Other(format!("failed to read \"{name}\" from backup (wrong password?): {e}")))?;
        let mut out = Vec::new();
        file.read_to_end(&mut out)?;
        Ok(out)
    };

    let _manifest: Manifest = serde_json::from_slice(&read_entry(MANIFEST_ENTRY)?)
        .map_err(|e| AppError::Other(format!("invalid backup manifest: {e}")))?;
    let db_bytes = read_entry(DB_ENTRY)?;
    let secrets: Vec<SecretRecord> = serde_json::from_slice(&read_entry(SECRETS_ENTRY)?)
        .map_err(|e| AppError::Other(format!("invalid backup secrets bundle: {e}")))?;

    let path = temp_db_path();
    let restore_result = (|| -> AppResult<()> {
        std::fs::write(&path, &db_bytes)?;
        let mut source = Connection::open(&path)?;
        // Forward-compat: a backup taken on an older schema version still
        // restores cleanly onto a newer build.
        crate::store::migrations::run(&mut source)?;
        Backup::new(&source, conn)?.run_to_completion(100, Duration::from_millis(10), None)?;
        drop(source);
        // The backup step overwrote `conn`'s database-file header (which
        // persists the WAL setting) with the temp source's plain-journal
        // default — reassert this app's pragmas.
        db::configure(conn)?;
        Ok(())
    })();
    std::fs::remove_file(&path).ok();
    restore_result?;

    let mut secrets_restored = 0;
    for rec in &secrets {
        crate::secrets::set(&rec.key, &rec.value)?;
        secrets_restored += 1;
    }

    Ok(RestoreReport {
        secrets_restored,
        workspaces: count_rows(conn, "workspaces")?,
        collections: count_rows(conn, "collections")?,
        requests: count_rows(conn, "requests")?,
        environments: count_rows(conn, "environments")?,
        history_entries: count_rows(conn, "history")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::auth::AuthConfig;

    fn seeded_conn() -> (Connection, String) {
        let mut conn = crate::store::db::open_in_memory().unwrap();
        let ws = crate::store::workspaces::ensure_default(&mut conn).unwrap();
        let c = crate::store::collections::create(&conn, &ws.id, None, "Secret Collection", None).unwrap();
        crate::store::collections::update_auth(&conn, &c.id, AuthConfig::Bearer { token: "very-real-token".into() }).unwrap();
        (conn, ws.id)
    }

    #[test]
    fn create_backup_rejects_empty_password() {
        let (conn, _ws) = seeded_conn();
        let err = create_backup(&conn, "").unwrap_err();
        assert!(err.to_string().contains("password"));
    }

    #[test]
    fn round_trip_restores_data_and_real_secret() {
        let (conn, ws) = seeded_conn();
        let bytes = create_backup(&conn, "correct horse battery staple").unwrap();

        // The test-mode keychain fake (`crate::secrets`) is one process-
        // global map — restoring under the *same* key a value that was
        // never actually removed would let this assertion pass even if
        // `restore_backup`'s `secrets::set` loop were deleted. Prove the
        // restore path actually does the writing by wiping every key this
        // backup carries before restoring, and confirming they're gone.
        let carried_keys: Vec<String> = collect_real_secrets(&conn).unwrap().into_iter().map(|s| s.key).collect();
        assert!(!carried_keys.is_empty(), "expected at least the bearer token to be captured");
        for key in &carried_keys {
            crate::secrets::delete(key).unwrap();
        }
        for key in &carried_keys {
            assert!(crate::secrets::get(key).unwrap().is_none(), "test setup: {key} should be wiped before restore");
        }

        // Restore into a completely fresh, empty connection — simulates a
        // clean-reinstall disaster-recovery scenario.
        let mut fresh = crate::store::db::open_in_memory().unwrap();
        let report = restore_backup(&mut fresh, &bytes, "correct horse battery staple").unwrap();
        assert_eq!(report.secrets_restored, carried_keys.len());
        assert_eq!(report.workspaces, 1);
        assert_eq!(report.collections, 1);
        assert_eq!(report.requests, 0);
        assert_eq!(report.environments, 0);
        assert_eq!(report.history_entries, 0);
        for key in &carried_keys {
            assert!(crate::secrets::get(key).unwrap().is_some(), "restore_backup did not actually rewrite {key}");
        }

        let roots = crate::store::collections::list_children(&fresh, &ws, None).unwrap();
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].name, "Secret Collection");

        let owner = crate::auth::owner_key("collection", &roots[0].id);
        let real = crate::auth::hydrate(&owner, roots[0].auth.clone()).unwrap();
        assert_eq!(real, AuthConfig::Bearer { token: "very-real-token".into() });
    }

    #[test]
    fn wrong_password_fails_to_decrypt_rather_than_silently_corrupting() {
        let (conn, _ws) = seeded_conn();
        let bytes = create_backup(&conn, "right-password").unwrap();
        let mut fresh = crate::store::db::open_in_memory().unwrap();
        let err = restore_backup(&mut fresh, &bytes, "wrong-password").unwrap_err();
        assert!(err.to_string().contains("backup"), "{err}");
    }

    #[test]
    fn secrets_bundle_carries_the_real_value_not_a_mask() {
        let (conn, _ws) = seeded_conn();
        let secrets = collect_real_secrets(&conn).unwrap();
        assert!(secrets.iter().any(|s| s.value == "very-real-token"), "{secrets:?}");
    }
}
