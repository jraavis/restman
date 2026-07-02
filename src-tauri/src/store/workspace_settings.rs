//! Per-workspace transport settings repository.
//!
//! Secrets handling: pasted PEM cert/key/passphrase bytes never land in the
//! SQLite row in cleartext. `set()` writes them to the OS keychain first
//! (under `wscert:{workspace_id}:{slot}` slots), then stores *only* an
//! `SECRET_MASK` placeholder in the row's `client_cert_json`. `get()` returns
//! that masked JSON unchanged for frontend display — send-time hydration reads
//! the real bytes back from the keychain (see
//! `crate::workspace::hydrate_client_cert_material`). Same mask-on-write
//! contract the auth module already follows for OAuth/SigV4 credentials.

use crate::error::AppResult;
use crate::model::{ClientCertConfig, SyncFormat, SyncMode, WorkspaceSettings};
use crate::model::http::HeaderEntry;
use crate::secrets;
use crate::model::variable::SECRET_MASK;
use rusqlite::{params, Connection};

const SLOTS: &[&str] = &["cert", "key", "pass"];

fn keychain_slot(workspace_id: &str, slot: &str) -> String {
    format!("wscert:{workspace_id}:{slot}")
}

/// Per-slot mask-on-write contract, mirroring `auth::persist`
/// (`src-tauri/src/auth/mod.rs`): a value still equal to `SECRET_MASK` is
/// left untouched — the keychain already holds the real bytes, and writing
/// the mask string over it would destroy them. An empty value clears the
/// slot. Anything else is a real new secret, written for real. Returns the
/// value the DB row should display.
fn persist_secret_slot(slot: &str, value: &str) -> AppResult<String> {
    if value == SECRET_MASK {
        return Ok(SECRET_MASK.to_string());
    }
    if value.is_empty() {
        secrets::delete(slot).ok();
        return Ok(String::new());
    }
    secrets::set(slot, value)?;
    Ok(SECRET_MASK.to_string())
}

/// Which keychain slots a cert config variant actually owns. Drives the
/// sweep in `persist_cert_secrets` — any slot *not* in this set gets
/// deleted, mirroring how `auth::persist` (`src-tauri/src/auth/mod.rs`)
/// sweeps `AuthConfig::secret_slots()` for any slot outside the live
/// variant's `secret_fields()`.
fn live_secret_slots(cert: &ClientCertConfig) -> Vec<&'static str> {
    match cert {
        ClientCertConfig::None => vec![],
        ClientCertConfig::Paste { passphrase, .. } => {
            let mut slots = vec!["cert", "key"];
            if passphrase.is_some() {
                slots.push("pass");
            }
            slots
        }
        ClientCertConfig::Path { passphrase, .. } => {
            if passphrase.is_some() { vec!["pass"] } else { vec![] }
        }
    }
}

/// Mask pasted-PEM secrets before they're persisted: real bytes go to the
/// keychain, the JSON row holds `SECRET_MASK` so re-reading on the frontend
/// displays "set but masked" without exposing the value. Path mode stores
/// only filesystem paths (no keychain use) except for the passphrase, which
/// is a credential even though it unlocks a file rather than gating an API.
/// Slots outside the new config's `live_secret_slots` are swept *first*, so
/// switching away from Paste (to None or Path) doesn't leave the old
/// cert/key/pass bytes orphaned in the OS keychain.
fn persist_cert_secrets(workspace_id: &str, cert: &ClientCertConfig) -> AppResult<ClientCertConfig> {
    let live = live_secret_slots(cert);
    for slot in SLOTS {
        if !live.contains(slot) {
            secrets::delete(&keychain_slot(workspace_id, slot)).ok();
        }
    }
    Ok(match cert {
        ClientCertConfig::None => ClientCertConfig::None,
        ClientCertConfig::Paste { cert_pem, key_pem, passphrase } => {
            let cert_pem = persist_secret_slot(&keychain_slot(workspace_id, "cert"), cert_pem)?;
            let key_pem = persist_secret_slot(&keychain_slot(workspace_id, "key"), key_pem)?;
            let passphrase = match passphrase {
                Some(p) => Some(persist_secret_slot(&keychain_slot(workspace_id, "pass"), p)?),
                None => None,
            };
            ClientCertConfig::Paste { cert_pem, key_pem, passphrase }
        }
        ClientCertConfig::Path { cert_path, key_path, passphrase } => {
            let passphrase = match passphrase {
                Some(p) => Some(persist_secret_slot(&keychain_slot(workspace_id, "pass"), p)?),
                None => None,
            };
            ClientCertConfig::Path {
                cert_path: cert_path.clone(),
                key_path: key_path.clone(),
                passphrase,
            }
        }
    })
}

/// Reverse of `persist_cert_secrets`: a freshly-saved row's masked PEM fields
/// should *not* be treated as literal bytes if the secret isn't there yet.
/// For get/display we return the masked row as-is; hydration happens at send
/// time (see `hydrate_client_cert_material`).
type SettingsRow = (Option<String>, Option<String>, String, String, Option<String>, String, String);

pub fn get(conn: &Connection, workspace_id: &str) -> AppResult<WorkspaceSettings> {
    let row: Option<SettingsRow> = conn
        .query_row(
            "SELECT proxy_url, proxy_bypass, default_headers_json, client_cert_json,
                    sync_folder_path, sync_mode, sync_format
             FROM workspace_settings WHERE workspace_id = ?1",
            params![workspace_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get(4)?, r.get::<_, String>(5)?, r.get::<_, String>(6)?)),
        )
        .map(Some)
        .or_else(|e| if matches!(e, rusqlite::Error::QueryReturnedNoRows) { Ok(None) } else { Err(e) })?;

    let Some((proxy_url, proxy_bypass, headers_json, cert_json, sync_folder_path, sync_mode, sync_format)) = row else {
        return Ok(WorkspaceSettings::empty(workspace_id));
    };
    let default_headers: Vec<HeaderEntry> = serde_json::from_str(&headers_json).unwrap_or_default();
    let client_cert: ClientCertConfig =
        serde_json::from_str(&cert_json).unwrap_or(ClientCertConfig::None);
    Ok(WorkspaceSettings {
        workspace_id: workspace_id.to_string(),
        proxy_url,
        proxy_bypass,
        default_headers,
        client_cert,
        sync_folder_path,
        sync_mode: SyncMode::parse(&sync_mode),
        sync_format: SyncFormat::parse(&sync_format),
    })
}

/// Persist `settings`. Secret PEM bytes/routes go to the keychain; the row
/// references them masked. `upsert` semantics — one row per workspace.
pub fn set(conn: &Connection, settings: &WorkspaceSettings) -> AppResult<WorkspaceSettings> {
    let masked_cert = persist_cert_secrets(&settings.workspace_id, &settings.client_cert)?;
    let headers_json = serde_json::to_string(&settings.default_headers).unwrap_or_else(|_| "[]".into());
    let cert_json = serde_json::to_string(&masked_cert).unwrap_or_else(|_| r#"{"mode":"none"}"#.into());
    conn.execute(
        "INSERT INTO workspace_settings (workspace_id, proxy_url, proxy_bypass, default_headers_json, client_cert_json, sync_folder_path, sync_mode, sync_format)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(workspace_id) DO UPDATE SET
            proxy_url = excluded.proxy_url,
            proxy_bypass = excluded.proxy_bypass,
            default_headers_json = excluded.default_headers_json,
            client_cert_json = excluded.client_cert_json,
            sync_folder_path = excluded.sync_folder_path,
            sync_mode = excluded.sync_mode,
            sync_format = excluded.sync_format",
        params![
            settings.workspace_id, settings.proxy_url, settings.proxy_bypass, headers_json, cert_json,
            settings.sync_folder_path, settings.sync_mode.as_str(), settings.sync_format.as_str()
        ],
    )?;
    // Return the persisted (masked) view so the caller (IPC) reflects what was
    // actually stored, not the plaintext it was handed.
    get(conn, &settings.workspace_id)
}

/// Drop any keychain slots a workspace's cert config owns. Called on
/// workspace delete (cascade) to avoid orphaned credentials.
pub fn delete_cert_secrets(workspace_id: &str) -> AppResult<()> {
    for slot in SLOTS {
        secrets::delete(&keychain_slot(workspace_id, slot)).ok();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem() -> Connection {
        let mut conn = crate::store::db::open_in_memory().unwrap();
        let _ = crate::store::workspaces::ensure_default(&mut conn).unwrap();
        conn
    }

    fn default_ws_id(conn: &Connection) -> String {
        crate::store::workspaces::active(conn).unwrap().unwrap().id
    }

    #[test]
    fn get_on_missing_workspace_returns_empty_defaults() {
        let conn = mem();
        let s = get(&conn, "no-such-ws").unwrap();
        assert_eq!(s.proxy_url, None);
        assert_eq!(s.default_headers, Vec::new());
        assert_eq!(s.client_cert, ClientCertConfig::None);
    }

    #[test]
    fn set_then_get_round_trips_proxy_and_default_headers() {
        let conn = mem();
        let ws = default_ws_id(&conn);
        let s = WorkspaceSettings {
            workspace_id: ws.clone(),
            proxy_url: Some("http://proxy.corp:8080".into()),
            proxy_bypass: Some("localhost,*.corp".into()),
            default_headers: vec![
                HeaderEntry { name: "X-Team".into(), value: "platform".into(), enabled: true },
                HeaderEntry { name: "X-Off".into(), value: "1".into(), enabled: false },
            ],
            client_cert: ClientCertConfig::None,
            ..WorkspaceSettings::empty(&ws)
        };
        let saved = set(&conn, &s).unwrap();
        assert_eq!(saved.proxy_url.as_deref(), Some("http://proxy.corp:8080"));
        assert_eq!(saved.default_headers.len(), 2);

        let again = get(&conn, &ws).unwrap();
        assert_eq!(again.default_headers, s.default_headers);
    }

    #[test]
    fn paste_mode_pem_bytes_go_to_keychain_not_the_db_row() {
        let conn = mem();
        let ws = default_ws_id(&conn);
        let s = WorkspaceSettings {
            workspace_id: ws.clone(),
            proxy_url: None,
            proxy_bypass: None,
            default_headers: Vec::new(),
            client_cert: ClientCertConfig::Paste {
                cert_pem: "-----BEGIN CERTIFICATE-----\nreal\n-----END CERTIFICATE-----\n".into(),
                key_pem: "-----BEGIN PRIVATE KEY-----\nreal\n-----END PRIVATE KEY-----\n".into(),
                passphrase: Some("hunter2".into()),
            },
            ..WorkspaceSettings::empty(&ws)
        };
        let saved = set(&conn, &s).unwrap();
        // DB-facing view is masked.
        match &saved.client_cert {
            ClientCertConfig::Paste { cert_pem, key_pem, passphrase } => {
                assert_eq!(cert_pem, SECRET_MASK);
                assert_eq!(key_pem, SECRET_MASK);
                assert_eq!(passphrase.as_deref(), Some(SECRET_MASK));
            }
            other => panic!("expected Paste, got {other:?}"),
        }
        // Keychain holds the real bytes.
        let cert = secrets::get(&keychain_slot(&ws, "cert")).unwrap().unwrap();
        assert!(cert.contains("BEGIN CERTIFICATE"));
        let key = secrets::get(&keychain_slot(&ws, "key")).unwrap().unwrap();
        assert!(key.contains("BEGIN PRIVATE KEY"));
        let pass = secrets::get(&keychain_slot(&ws, "pass")).unwrap().unwrap();
        assert_eq!(pass, "hunter2");
    }

    #[test]
    fn resaving_already_masked_paste_cert_does_not_clobber_keychain() {
        let conn = mem();
        let ws = default_ws_id(&conn);
        let s = WorkspaceSettings {
            workspace_id: ws.clone(),
            proxy_url: None,
            proxy_bypass: None,
            default_headers: Vec::new(),
            client_cert: ClientCertConfig::Paste {
                cert_pem: "-----BEGIN CERTIFICATE-----\nreal\n-----END CERTIFICATE-----\n".into(),
                key_pem: "-----BEGIN PRIVATE KEY-----\nreal\n-----END PRIVATE KEY-----\n".into(),
                passphrase: Some("hunter2".into()),
            },
            ..WorkspaceSettings::empty(&ws)
        };
        let saved = set(&conn, &s).unwrap();

        // Simulate the frontend round-trip: fetch settings (masked), edit an
        // unrelated field, save again with the cert config untouched — i.e.
        // still carrying SECRET_MASK placeholders, not the real PEM bytes.
        let resaved = WorkspaceSettings {
            proxy_url: Some("http://proxy.corp:8080".into()),
            ..saved
        };
        let resaved = set(&conn, &resaved).unwrap();
        match &resaved.client_cert {
            ClientCertConfig::Paste { cert_pem, key_pem, passphrase } => {
                assert_eq!(cert_pem, SECRET_MASK);
                assert_eq!(key_pem, SECRET_MASK);
                assert_eq!(passphrase.as_deref(), Some(SECRET_MASK));
            }
            other => panic!("expected Paste, got {other:?}"),
        }

        // The keychain must still hold the ORIGINAL real bytes, not the
        // literal SECRET_MASK placeholder string.
        let cert = secrets::get(&keychain_slot(&ws, "cert")).unwrap().unwrap();
        assert!(cert.contains("BEGIN CERTIFICATE"), "cert was clobbered: {cert:?}");
        let key = secrets::get(&keychain_slot(&ws, "key")).unwrap().unwrap();
        assert!(key.contains("BEGIN PRIVATE KEY"), "key was clobbered: {key:?}");
        let pass = secrets::get(&keychain_slot(&ws, "pass")).unwrap().unwrap();
        assert_eq!(pass, "hunter2", "passphrase was clobbered");
    }

    #[test]
    fn switching_from_paste_to_none_sweeps_orphaned_keychain_slots() {
        let conn = mem();
        let ws = default_ws_id(&conn);
        let pasted = WorkspaceSettings {
            workspace_id: ws.clone(),
            proxy_url: None,
            proxy_bypass: None,
            default_headers: Vec::new(),
            client_cert: ClientCertConfig::Paste {
                cert_pem: "-----BEGIN CERTIFICATE-----\nreal\n-----END CERTIFICATE-----\n".into(),
                key_pem: "-----BEGIN PRIVATE KEY-----\nreal\n-----END PRIVATE KEY-----\n".into(),
                passphrase: Some("hunter2".into()),
            },
            ..WorkspaceSettings::empty(&ws)
        };
        set(&conn, &pasted).unwrap();
        // Sanity check: the keychain actually holds the pasted bytes before we
        // switch away.
        assert!(secrets::get(&keychain_slot(&ws, "cert")).unwrap().is_some());
        assert!(secrets::get(&keychain_slot(&ws, "key")).unwrap().is_some());
        assert!(secrets::get(&keychain_slot(&ws, "pass")).unwrap().is_some());

        let cleared = WorkspaceSettings { client_cert: ClientCertConfig::None, ..pasted };
        set(&conn, &cleared).unwrap();

        assert!(secrets::get(&keychain_slot(&ws, "cert")).unwrap().is_none(), "cert slot orphaned");
        assert!(secrets::get(&keychain_slot(&ws, "key")).unwrap().is_none(), "key slot orphaned");
        assert!(secrets::get(&keychain_slot(&ws, "pass")).unwrap().is_none(), "pass slot orphaned");
    }

    #[test]
    fn empty_passphrase_clears_the_slot_instead_of_storing_empty_string() {
        let conn = mem();
        let ws = default_ws_id(&conn);
        let s = WorkspaceSettings {
            workspace_id: ws.clone(),
            proxy_url: None,
            proxy_bypass: None,
            default_headers: Vec::new(),
            client_cert: ClientCertConfig::Paste {
                cert_pem: "-----BEGIN CERTIFICATE-----\nreal\n-----END CERTIFICATE-----\n".into(),
                key_pem: "-----BEGIN PRIVATE KEY-----\nreal\n-----END PRIVATE KEY-----\n".into(),
                passphrase: Some(String::new()),
            },
            ..WorkspaceSettings::empty(&ws)
        };
        let saved = set(&conn, &s).unwrap();
        match &saved.client_cert {
            ClientCertConfig::Paste { passphrase, .. } => assert_eq!(passphrase.as_deref(), Some("")),
            other => panic!("expected Paste, got {other:?}"),
        }
        assert!(secrets::get(&keychain_slot(&ws, "pass")).unwrap().is_none());
    }
}