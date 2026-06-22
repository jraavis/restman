use crate::auth;
use crate::auth::oauth::{self, token_store};
use crate::error::AppResult;
use crate::model::auth::{AuthConfig, OAuth2Config, RequestAuth};
use crate::model::http::{HttpRequest, HttpResponse};
use crate::store::{collections, history, requests, AppState};
use std::sync::Arc;
use tauri::State;

/// Liveness check used to verify the IPC bridge end-to-end.
#[tauri::command]
pub fn ping() -> String {
    "pong".into()
}

/// Resolve `{{var}}`s and auth for the given workspace/collection/request,
/// send, and record the attempt to history regardless of outcome.
#[tauri::command]
pub async fn send_request(
    state: State<'_, AppState>,
    mut req: HttpRequest,
    workspace_id: String,
    collection_id: Option<String>,
    request_id: Option<String>,
    name: Option<String>,
) -> AppResult<HttpResponse> {
    let resolved = {
        let conn = state.db.lock().unwrap();
        crate::vars::resolve(&conn, &workspace_id, collection_id.as_deref())?
    }; // lock dropped before the .await below — see AppState's doc comment.
    crate::vars::interpolate_request(&mut req, &resolved.values);

    req.auth = resolve_auth(&state, collection_id.as_deref(), request_id.as_deref()).await?;

    let label = name.unwrap_or_else(|| format!("{} {}", req.method, req.url));
    let result = crate::engine::http::send(req.clone(), Some(Arc::clone(&state.cookie_jar))).await;

    // Secrets are redacted out of the copy written to history — see
    // `vars::redact_request` — so the live `req` above keeps real values
    // (needed for the send) while the persisted row never does. `auth` is
    // masked separately: `resolve_auth` just hydrated it to a real secret
    // (bearer token / password / API key / AWS secret key), and
    // `redact_request` only scrubs url/headers/body, not the `auth` field
    // itself — left alone, the real secret would land in `history_json` in
    // plaintext on every single send.
    let mut history_req = crate::vars::redact_request(&req, &resolved.secrets);
    history_req.auth = auth::mask_secrets(req.auth.clone());
    let conn = state.db.lock().unwrap();
    // History is best-effort logging; a write failure here must not mask the
    // actual send result, so errors are dropped rather than propagated.
    match &result {
        Ok(resp) => {
            let _ = history::insert(&conn, &workspace_id, request_id.as_deref(), &label, &history_req, Some(resp), None);
            if let Some(id) = request_id.as_deref() {
                let _ = requests::touch_last_used(&conn, id);
            }
        }
        Err(e) => {
            let _ = history::insert(&conn, &workspace_id, request_id.as_deref(), &label, &history_req, None, Some(&e.to_string()));
        }
    }
    result
}

/// Resolves owner + effective `AuthConfig` for a request (collection→request
/// inheritance, see `auth::resolve`) and recovers any real secret from the
/// keychain. Shared by `send_request` below (which may further collapse an
/// `OAuth2` result to a `Bearer`) and `commands::oauth::start_oauth2_authorization`
/// (which needs the raw `OAuth2Config` before any collapsing). Sync and
/// DB-only, so the lock it takes is always dropped well before any caller's
/// first `.await`.
pub(crate) fn resolve_owner_and_config(state: &State<'_, AppState>, collection_id: Option<&str>, request_id: Option<&str>) -> AppResult<(String, AuthConfig)> {
    let conn = state.db.lock().unwrap();
    let collection_owner = match collection_id {
        Some(cid) => Some((cid, collections::get(&conn, cid)?.auth)),
        None => None,
    };
    let request_auth = match request_id {
        Some(rid) => requests::get(&conn, rid)?.auth,
        None => RequestAuth::Inherit,
    };
    let (owner, masked) = auth::resolve(collection_owner, request_auth, request_id.unwrap_or(""));
    let hydrated = auth::hydrate(&owner, masked)?;
    Ok((owner, hydrated))
}

/// Resolves the request's effective auth (collection→request inheritance),
/// recovers any real secret from the keychain, and — if it's OAuth2 —
/// collapses it to a concrete bearer token, fetching or refreshing one if
/// the cache is stale. `engine::http::send` never sees an `OAuth2` variant.
async fn resolve_auth(state: &State<'_, AppState>, collection_id: Option<&str>, request_id: Option<&str>) -> AppResult<AuthConfig> {
    let (owner, hydrated) = resolve_owner_and_config(state, collection_id, request_id)?;
    match hydrated {
        AuthConfig::OAuth2(cfg) => collapse_oauth2(state, &owner, &cfg).await,
        other => Ok(other),
    }
}

/// Returns a cached token if still fresh, refreshes if expired-but-refreshable,
/// otherwise runs a from-scratch grant exchange — then caches the result and
/// hands back a plain `Bearer`. Never holds the DB lock across an `.await`:
/// each lock/unlock here brackets a network call, exactly as `auth::oauth`'s
/// module doc requires.
async fn collapse_oauth2(state: &State<'_, AppState>, owner: &str, cfg: &OAuth2Config) -> AppResult<AuthConfig> {
    let cached = {
        let conn = state.db.lock().unwrap();
        token_store::get(&conn, owner)?
    };
    let token = match cached {
        Some(t) if token_store::is_fresh(&t) => t,
        Some(t) if t.refresh_token.is_some() => {
            let refresh_token = t.refresh_token.clone().unwrap();
            match oauth::exchange_refresh_token(cfg, &refresh_token).await {
                Ok(fresh) => fresh,
                // Refresh token itself may be expired/revoked — fall back to
                // a fresh from-scratch exchange rather than failing the send.
                Err(_) => oauth::fetch_token(cfg).await?,
            }
        }
        _ => oauth::fetch_token(cfg).await?,
    };
    {
        let conn = state.db.lock().unwrap();
        token_store::put(&conn, owner, &token)?;
    }
    Ok(AuthConfig::Bearer { token: token.access_token })
}

/// Clear all cookies from the shared jar. Called from the frontend when the
/// user wants to reset session state.
#[tauri::command]
pub fn clear_cookies(state: State<'_, AppState>) -> AppResult<()> {
    state.cookie_jar.lock().map_err(|e| crate::error::AppError::Other(format!("cookie jar lock poisoned: {e}")))?.clear();
    Ok(())
}
