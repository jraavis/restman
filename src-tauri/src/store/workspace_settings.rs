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
use crate::model::{ClientCertConfig, WorkspaceSettings};
use crate::model::http::HeaderEntry;
use crate::secrets;
use crate::model::variable::SECRET_MASK;
use rusqlite::{params, Connection};

const SLOTS: &[&str] = &["cert", "key", "pass"];

fn keychain_slot(workspace_id: &str, slot: &str) -> String {
    format!("wscert:{workspace_id}:{slot}")
}

/// Mask pasted-PEM secrets before they're persisted: real bytes go to the
/// keychain, the JSON row holds `SECRET_MASK` so re-reading on the frontend
/// displays "set but masked" without exposing the value. Path mode stores
/// only filesystem paths (no keychain use).
fn persist_cert_secrets(workspace_id: &str, cert: &ClientCertConfig) -> AppResult<ClientCertConfig> {
    Ok(match cert {
        ClientCertConfig::None => ClientCertConfig::None,
        ClientCertConfig::Paste { cert_pem, key_pem, passphrase } => {
            secrets::set(&keychain_slot(workspace_id, "cert"), cert_pem)?;
            secrets::set(&keychain_slot(workspace_id, "key"), key_pem)?;
            if let Some(p) = passphrase {
                secrets::set(&keychain_slot(workspace_id, "pass"), p)?;
            } else {
                secrets::delete(&keychain_slot(workspace_id, "pass")).ok();
            }
            ClientCertConfig::Paste {
                cert_pem: if cert_pem.is_empty() { String::new() } else { SECRET_MASK.into() },
                key_pem: if key_pem.is_empty() { String::new() } else { SECRET_MASK.into() },
                passphrase: passphrase.as_ref().map(|p| if p.is_empty() { String::new() } else { SECRET_MASK.into() }),
            }
        }
        ClientCertConfig::Path { cert_path, key_path, passphrase } => {
            // Path mode keeps the passphrase in the keychain too — paths are
            // round-tripped through the row, but a passphrase is a credential
            // even if it unlocks a file.
            if let Some(p) = passphrase {
                secrets::set(&keychain_slot(workspace_id, "pass"), p)?;
            } else {
                secrets::delete(&keychain_slot(workspace_id, "pass")).ok();
            }
            ClientCertConfig::Path {
                cert_path: cert_path.clone(),
                key_path: key_path.clone(),
                passphrase: passphrase.as_ref().map(|p| if p.is_empty() { String::new() } else { SECRET_MASK.into() }),
            }
        }
    })
}

/// Reverse of `persist_cert_secrets`: a freshly-saved row's masked PEM fields
/// should *not* be treated as literal bytes if the secret isn't there yet.
/// For get/display we return the masked row as-is; hydration happens at send
/// time (see `hydrate_client_cert_material`).
pub fn get(conn: &Connection, workspace_id: &str) -> AppResult<WorkspaceSettings> {
    let row: Option<(Option<String>, Option<String>, String, String)> = conn
        .query_row(
            "SELECT proxy_url, proxy_bypass, default_headers_json, client_cert_json
             FROM workspace_settings WHERE workspace_id = ?1",
            params![workspace_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?)),
        )
        .map(Some)
        .or_else(|e| if matches!(e, rusqlite::Error::QueryReturnedNoRows) { Ok(None) } else { Err(e) })?;

    let Some((proxy_url, proxy_bypass, headers_json, cert_json)) = row else {
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
    })
}

/// Persist `settings`. Secret PEM bytes/routes go to the keychain; the row
/// references them masked. `upsert` semantics — one row per workspace.
pub fn set(conn: &Connection, settings: &WorkspaceSettings) -> AppResult<WorkspaceSettings> {
    let masked_cert = persist_cert_secrets(&settings.workspace_id, &settings.client_cert)?;
    let headers_json = serde_json::to_string(&settings.default_headers).unwrap_or_else(|_| "[]".into());
    let cert_json = serde_json::to_string(&masked_cert).unwrap_or_else(|_| r#"{"mode":"none"}"#.into());
    conn.execute(
        "INSERT INTO workspace_settings (workspace_id, proxy_url, proxy_bypass, default_headers_json, client_cert_json)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(workspace_id) DO UPDATE SET
            proxy_url = excluded.proxy_url,
            proxy_bypass = excluded.proxy_bypass,
            default_headers_json = excluded.default_headers_json,
            client_cert_json = excluded.client_cert_json",
        params![settings.workspace_id, settings.proxy_url, settings.proxy_bypass, headers_json, cert_json],
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
}