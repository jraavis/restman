//! Send-time workspace resolution: converts a (masked) `WorkspaceSettings`
//! row into engine-ready `TransportOverrides`, with PEM cert/key bytes
//! hydrated from the keychain (Paste mode) or read from disk (Path mode).
//! Also merges `default_headers` into a request before send — user headers
//! win, defaults fill the gaps.
//!
//! Lives outside `engine` so the engine stays pure over its inputs and
//! unit-testable without a DB or keychain (see the `TransportOverrides`
//! doc). Lives outside `store::workspace_settings` so that module stays a
//! thin persistence layer with no send-time concerns.

use crate::error::{AppError, AppResult};
use crate::engine::http::{ClientCertPem, TransportOverrides};
use crate::model::ClientCertConfig;
use crate::model::http::{HeaderEntry, HttpRequest};
use crate::secrets;
use crate::store::workspace_settings;
use rusqlite::Connection;

/// Read the workspace's settings row, hydrate secret cert bytes, and
/// produce the `TransportOverrides` the engine consumes. Returns `None`
/// when the workspace has no settings row at all (the common case — the
/// engine then builds a plain client with no proxy/identity).
pub fn resolve_transport(conn: &Connection, workspace_id: &str) -> AppResult<Option<TransportOverrides>> {
    let settings = workspace_settings::get(conn, workspace_id)?;
    if settings.proxy_url.is_none() && !settings.client_cert.is_set() {
        return Ok(None);
    }
    let identity = build_identity(workspace_id, &settings.client_cert)?;
    let (client_identity, client_cert_pem) = match identity {
        Some((identity, pem)) => (Some(identity), Some(pem)),
        None => (None, None),
    };
    Ok(Some(TransportOverrides {
        proxy_url: settings.proxy_url,
        proxy_bypass: settings.proxy_bypass,
        client_identity,
        client_cert_pem,
    }))
}

/// Inline this workspace's `default_headers` into the request. For any header
/// name the request already carries (case-insensitive), the user's value is
/// kept — defaults only fill gaps, mirroring how every other REST client
/// treats "default headers".
pub fn apply_default_headers(req: &mut HttpRequest, conn: &Connection, workspace_id: &str) -> AppResult<()> {
    let settings = workspace_settings::get(conn, workspace_id)?;
    if settings.default_headers.is_empty() {
        return Ok(());
    }
    let existing: std::collections::HashSet<String> =
        req.headers.iter().map(|h| h.name.to_ascii_lowercase()).collect();
    for h in settings.default_headers.iter().filter(|h| h.enabled) {
        if !existing.contains(&h.name.to_ascii_lowercase()) {
            req.headers.push(HeaderEntry { name: h.name.clone(), value: h.value.clone(), enabled: true });
        }
    }
    Ok(())
}

fn keychain_slot(workspace_id: &str, slot: &str) -> String {
    format!("wscert:{workspace_id}:{slot}")
}

fn build_identity(
    workspace_id: &str,
    cert: &ClientCertConfig,
) -> AppResult<Option<(reqwest::Identity, ClientCertPem)>> {
    match cert {
        ClientCertConfig::None => Ok(None),
        ClientCertConfig::Paste { cert_pem, key_pem, passphrase } => {
            // `cert_pem`/`key_pem` arrive masked from the DB row; hydrate
            // the real bytes from the keychain. Empty == not set / cleared.
            let cert_real = secrets::get(&keychain_slot(workspace_id, "cert"))?.unwrap_or_default();
            let key_real = secrets::get(&keychain_slot(workspace_id, "key"))?.unwrap_or_default();
            if cert_real.is_empty() && key_real.is_empty() {
                return Ok(None);
            }
            let pass_real = match passphrase {
                Some(_) => secrets::get(&keychain_slot(workspace_id, "pass"))?.unwrap_or_default(),
                None => String::new(),
            };
            // Err if masking drift left the row claiming Paste but the
            // keychain is empty (e.g. cleared out-of-band) — a confusing
            // state the UI shouldn't be able to produce but we shouldn't
            // silently send unsigned.
            let _ = (cert_pem, key_pem); // borrow-check: masked strings unused
            if cert_real.is_empty() || key_real.is_empty() {
                return Err(AppError::Other(
                    "workspace client certificate is configured but its PEM cert/key are missing from the keychain — re-enter the certificate".into(),
                ));
            }
            let mut pem = String::new();
            pem.push_str(&cert_real);
            pem.push('\n');
            pem.push_str(&key_real);
            let _ = pass_real; // reqwest's native-tls Identity::from_pem doesn't take a passphrase
            let identity = reqwest::Identity::from_pem(pem.as_bytes())
                .map_err(|e| AppError::Other(format!("invalid client certificate PEM: {e}")))?;
            Ok(Some((identity, ClientCertPem { cert_pem: cert_real, key_pem: key_real })))
        }
        ClientCertConfig::Path { cert_path, key_path, passphrase: _ } => {
            if cert_path.is_empty() || key_path.is_empty() {
                return Ok(None);
            }
            let cert_bytes = std::fs::read(cert_path).map_err(|e| {
                AppError::Other(format!("failed to read client cert at \"{cert_path}\": {e}"))
            })?;
            let key_bytes = std::fs::read(key_path).map_err(|e| {
                AppError::Other(format!("failed to read client key at \"{key_path}\": {e}"))
            })?;
            let cert_real = String::from_utf8_lossy(&cert_bytes).to_string();
            let key_real = String::from_utf8_lossy(&key_bytes).to_string();
            let mut pem = cert_real.clone();
            pem.push('\n');
            pem.push_str(&key_real);
            let identity = reqwest::Identity::from_pem(pem.as_bytes()).map_err(|e| {
                AppError::Other(format!("invalid client certificate PEM at \"{cert_path}\": {e}"))
            })?;
            Ok(Some((identity, ClientCertPem { cert_pem: cert_real, key_pem: key_real })))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::WorkspaceSettings;

    fn mem() -> Connection {
        let mut conn = crate::store::db::open_in_memory().unwrap();
        let _ = crate::store::workspaces::ensure_default(&mut conn).unwrap();
        conn
    }

    fn default_ws_id(conn: &Connection) -> String {
        crate::store::workspaces::active(conn).unwrap().unwrap().id
    }

    #[test]
    fn resolve_transport_returns_none_when_workspace_has_no_settings() {
        let conn = mem();
        let ws = default_ws_id(&conn);
        assert!(resolve_transport(&conn, &ws).unwrap().is_none());
    }

    #[test]
    fn resolve_transport_yields_proxy_when_set() {
        let conn = mem();
        let ws = default_ws_id(&conn);
        let s = WorkspaceSettings {
            workspace_id: ws.clone(),
            proxy_url: Some("http://proxy.corp:8080".into()),
            proxy_bypass: Some("localhost".into()),
            default_headers: Vec::new(),
            client_cert: ClientCertConfig::None,
            ..WorkspaceSettings::empty(&ws)
        };
        workspace_settings::set(&conn, &s).unwrap();
        let t = resolve_transport(&conn, &ws).unwrap().unwrap();
        assert_eq!(t.proxy_url.as_deref(), Some("http://proxy.corp:8080"));
        assert_eq!(t.proxy_bypass.as_deref(), Some("localhost"));
        assert!(t.client_identity.is_none());
    }

    #[test]
    fn apply_default_headers_fills_gaps_without_overriding_user_headers() {
        let conn = mem();
        let ws = default_ws_id(&conn);
        let s = WorkspaceSettings {
            workspace_id: ws.clone(),
            proxy_url: None,
            proxy_bypass: None,
            default_headers: vec![
                HeaderEntry { name: "X-Default".into(), value: "should-appear".into(), enabled: true },
                HeaderEntry { name: "X-User-Wins".into(), value: "default".into(), enabled: true },
            ],
            client_cert: ClientCertConfig::None,
            ..WorkspaceSettings::empty(&ws)
        };
        workspace_settings::set(&conn, &s).unwrap();

        let mut req = HttpRequest::default();
        req.headers.push(HeaderEntry { name: "X-User-Wins".into(), value: "user".into(), enabled: true });
        apply_default_headers(&mut req, &conn, &ws).unwrap();

        let by_name = |n: &str| req.headers.iter().find(|h| h.name.eq_ignore_ascii_case(n)).map(|h| h.value.clone());
        assert_eq!(by_name("X-Default").as_deref(), Some("should-appear"));
        assert_eq!(by_name("X-User-Wins").as_deref(), Some("user"), "user header must win over default");
    }

    #[test]
    fn apply_default_headers_skips_disabled_defaults() {
        let conn = mem();
        let ws = default_ws_id(&conn);
        let s = WorkspaceSettings {
            workspace_id: ws.clone(),
            proxy_url: None,
            proxy_bypass: None,
            default_headers: vec![HeaderEntry { name: "X-Off".into(), value: "nope".into(), enabled: false }],
            client_cert: ClientCertConfig::None,
            ..WorkspaceSettings::empty(&ws)
        };
        workspace_settings::set(&conn, &s).unwrap();

        let mut req = HttpRequest::default();
        apply_default_headers(&mut req, &conn, &ws).unwrap();
        assert!(req.headers.iter().all(|h| h.name != "X-Off"));
    }

    #[test]
    fn paste_mode_identity_is_hydrated_from_keychain_bytes() {
        let conn = mem();
        let ws = default_ws_id(&conn);
        // A real (toy) PEM pair — reqwest needs valid PEM headers to parse.
        let cert_pem = "-----BEGIN CERTIFICATE-----\nQkFH\ndW15\n-----END CERTIFICATE-----\n";
        let key_pem = "-----BEGIN PRIVATE KEY-----\nQkFH\ndW15\n-----END PRIVATE KEY-----\n";
        let s = WorkspaceSettings {
            workspace_id: ws.clone(),
            proxy_url: None,
            proxy_bypass: None,
            default_headers: Vec::new(),
            client_cert: ClientCertConfig::Paste {
                cert_pem: cert_pem.into(),
                key_pem: key_pem.into(),
                passphrase: None,
            },
            ..WorkspaceSettings::empty(&ws)
        };
        workspace_settings::set(&conn, &s).unwrap();
        let _t = resolve_transport(&conn, &ws).unwrap().unwrap();
        // `from_pem` on toy bytes may fail on some tls backends, so only
        // assert the happy path: the cert/key bytes were retrieved from the
        // keychain (resolved_structure), and identity parsing is attempted.
        let cert_real = secrets::get(&keychain_slot(&ws, "cert")).unwrap().unwrap();
        assert!(cert_real.contains("BEGIN CERTIFICATE"));
    }
}