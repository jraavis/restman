//! IPC entry point for the OAuth2 Authorization Code browser flow: the one
//! piece `auth::oauth`'s module doc defers — opening the system browser and
//! running the loopback `TcpListener` that catches the redirect. Everything
//! protocol-specific (building the authorize URL, parsing the callback,
//! exchanging the code) stays in `auth::oauth`; this module is just the
//! socket/process glue around it.

use crate::auth::oauth::{self, token_store};
use crate::commands::http::resolve_owner_and_config;
use crate::error::{AppError, AppResult};
use crate::model::auth::{AuthConfig, OAuth2GrantType};
use crate::store::AppState;
use serde::Serialize;
use std::time::Duration;
use tauri::State;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

/// Non-secret summary of a completed connection — never the token itself,
/// which (per `crate::auth`'s mask-on-write contract) must not cross IPC.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuth2Status {
    pub connected: bool,
    pub expires_at: Option<i64>,
    pub scope: Option<String>,
}

/// How long to wait for the user to finish signing in before giving up.
const AUTHORIZE_TIMEOUT: Duration = Duration::from_secs(300);

/// Runs the Authorization Code browser flow for the OAuth2 config resolved
/// for this collection/request, caches the resulting token, and returns a
/// non-secret status. Only valid when that config's grant type is
/// `AuthorizationCode` — the other three grants never need a browser and are
/// fetched lazily by `send_request` instead, so this command is never on the
/// hot path of sending a request, only an explicit user "connect" action.
#[tauri::command]
pub async fn start_oauth2_authorization(
    state: State<'_, AppState>,
    collection_id: Option<String>,
    request_id: Option<String>,
) -> AppResult<OAuth2Status> {
    let (owner, hydrated) = resolve_owner_and_config(&state, collection_id.as_deref(), request_id.as_deref(), None)?;
    let cfg = match hydrated {
        AuthConfig::OAuth2(cfg) if cfg.grant_type == OAuth2GrantType::AuthorizationCode => cfg,
        AuthConfig::OAuth2(_) => {
            return Err(AppError::Other(
                "this grant type doesn't use a browser — its token is fetched automatically when the request is sent".into(),
            ))
        }
        _ => return Err(AppError::Other("not configured for OAuth2".into())),
    };

    let addr = oauth::loopback_bind_addr(&cfg.redirect_uri)?;
    let listener = TcpListener::bind(addr)
        .await
        .map_err(|e| AppError::Other(format!("couldn't start local OAuth2 listener on {addr}: {e}")))?;

    let request = oauth::authorize_url(&cfg)?;
    tauri_plugin_opener::open_url(&request.url, None::<&str>).map_err(|e| AppError::Other(format!("couldn't open browser: {e}")))?;

    let (stream, _) = tokio::time::timeout(AUTHORIZE_TIMEOUT, listener.accept())
        .await
        .map_err(|_| AppError::Other("timed out waiting for sign-in in the browser".into()))??;
    let callback = read_callback(stream).await?;

    if callback.state != request.csrf_state {
        return Err(AppError::Other("OAuth2 callback state mismatch — possible CSRF, aborting".into()));
    }

    let token = oauth::exchange_code(&cfg, &callback.code, &request.pkce_verifier).await?;
    let status = OAuth2Status { connected: true, expires_at: token.expires_at, scope: token.scope.clone() };
    {
        let conn = state.db.lock().unwrap();
        token_store::put(&conn, &owner, &token)?;
    }
    Ok(status)
}

/// Read-only counterpart to `start_oauth2_authorization`, for the Auth editor
/// to show current connection state (e.g. "Connected, expires in 47 min")
/// without ever popping a browser window itself. `None` means either this
/// isn't an OAuth2 config or no token has been obtained yet — both render
/// the same "Not connected" way in the UI, so they're not distinguished.
/// Never refreshes; a near-/already-expired token is still reported, since
/// `send_request` will silently refresh it on the next send regardless.
#[tauri::command]
pub fn get_oauth2_status(state: State<AppState>, collection_id: Option<String>, request_id: Option<String>) -> AppResult<Option<OAuth2Status>> {
    let (owner, hydrated) = resolve_owner_and_config(&state, collection_id.as_deref(), request_id.as_deref(), None)?;
    if !matches!(hydrated, AuthConfig::OAuth2(_)) {
        return Ok(None);
    }
    let conn = state.db.lock().unwrap();
    let cached = token_store::get(&conn, &owner)?;
    Ok(cached.map(|t| OAuth2Status { connected: true, expires_at: t.expires_at, scope: t.scope }))
}

/// Returns a server-side masked preview string (e.g. `tok-ab…xy9`) for the
/// cached access token belonging to this collection/request's OAuth2 config.
/// The raw token never crosses IPC — only the masked version does.
/// Returns `None` if no token is cached or the config isn't OAuth2.
#[tauri::command]
pub fn get_oauth_token_preview(
    state: State<AppState>,
    collection_id: Option<String>,
    request_id: Option<String>,
) -> AppResult<Option<String>> {
    let (owner, hydrated) = resolve_owner_and_config(
        &state,
        collection_id.as_deref(),
        request_id.as_deref(),
        None,
    )?;
    if !matches!(hydrated, AuthConfig::OAuth2(_)) {
        return Ok(None);
    }
    let conn = state.db.lock().unwrap();
    let cached = token_store::get(&conn, &owner)?;
    Ok(cached.map(|t| mask_token_preview(&t.access_token)))
}

/// Compute a short masked preview of a token.
/// Takes up to 4 chars from each end, separated by `…`.
/// E.g. "eyJhbGciOiJSUzI1NiJ9.abc.xyz" → "eyJh….xyz"
fn mask_token_preview(token: &str) -> String {
    const HEAD: usize = 4;
    const TAIL: usize = 3;
    if token.len() <= HEAD + TAIL + 1 {
        // Token is short enough that no useful masking is possible; still
        // don't return raw — return all-stars.
        return "****".into();
    }
    let head = &token[..HEAD];
    let tail = &token[token.len() - TAIL..];
    format!("{head}\u{2026}{tail}")
}

/// Reads just the request line off the one-shot callback connection, replies
/// with a small human-readable page, and hands the line to
/// `oauth::parse_callback_request_line`. Doesn't bother draining the rest of
/// the browser's request (headers, keep-alive) — the listener is dropped
/// right after, so the OS reclaims the socket regardless.
async fn read_callback(mut stream: TcpStream) -> AppResult<oauth::CallbackResult> {
    let mut line = String::new();
    BufReader::new(&mut stream).read_line(&mut line).await?;
    let result = oauth::parse_callback_request_line(&line);
    let body = if result.is_ok() {
        "<html><body>Sign-in complete. You can close this tab and return to Restman.</body></html>"
    } else {
        "<html><body>Sign-in failed. Close this tab and try again in Restman.</body></html>"
    };
    let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
    let _ = stream.write_all(response.as_bytes()).await;
    result
}
