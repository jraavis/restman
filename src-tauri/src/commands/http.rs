use crate::auth;
use crate::auth::oauth::{self, token_store};
use crate::error::AppResult;
use crate::model::auth::{AuthConfig, OAuth2Config, RequestAuth};
use crate::model::http::{HttpRequest, HttpResponse};
use crate::scripting::{
    run_post_script, run_pre_script, PostScriptContext, PreScriptContext, ScriptResult,
};
use crate::store::{collections, history, requests, AppState};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tauri::State;

/// The full result of a `send_request` call: the HTTP response plus the
/// outcomes of any pre- and post-request scripts.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendResponse {
    pub response: HttpResponse,
    /// Result of the pre-request script, if one was configured.
    pub pre_script: Option<ScriptResult>,
    /// Result of the post-response script, if one was configured.
    pub post_script: Option<ScriptResult>,
}

/// Liveness check used to verify the IPC bridge end-to-end.
#[tauri::command]
pub fn ping() -> String {
    "pong".into()
}

/// Resolve `{{var}}`s and auth for the given workspace/collection/request,
/// run pre/post scripts, send, and record the attempt to history regardless
/// of outcome.
#[tauri::command]
pub async fn send_request(
    state: State<'_, AppState>,
    mut req: HttpRequest,
    workspace_id: String,
    collection_id: Option<String>,
    request_id: Option<String>,
    name: Option<String>,
) -> AppResult<SendResponse> {
    // 1. Resolve variables and interpolate.
    let resolved = {
        let conn = state.db.lock().unwrap();
        crate::vars::resolve(&conn, &workspace_id, collection_id.as_deref())?
    };
    crate::vars::interpolate_request(&mut req, &resolved.values);

    // 2. Resolve auth (collection → request, keychain hydration, OAuth2 token
    //    exchange). Lock is released before any .await.
    req.auth = resolve_auth(&state, collection_id.as_deref(), request_id.as_deref()).await?;

    // 3. Load scripts from the saved request (if any).
    let (pre_script_src, post_script_src) = if let Some(rid) = request_id.as_deref() {
        let conn = state.db.lock().unwrap();
        let saved = requests::get(&conn, rid)?;
        (saved.pre_request_script, saved.post_response_script)
    } else {
        (String::new(), String::new())
    };

    // 4. Run pre-request script.  If it aborts, skip the send.
    let mut env_overrides: HashMap<String, String> = HashMap::new();
    let pre_result = if !pre_script_src.trim().is_empty() {
        let ctx = PreScriptContext {
            method: req.method.clone(),
            url: req.url.clone(),
            headers: req.headers.iter().filter(|h| h.enabled)
                .map(|h| (h.name.clone(), h.value.clone()))
                .collect(),
            query: req.query.iter().filter(|q| q.enabled)
                .map(|q| (q.key.clone(), q.value.clone()))
                .collect(),
            env: resolved.values.clone(),
        };
        let r = tokio::task::spawn_blocking(move || run_pre_script(&pre_script_src, &ctx))
            .await
            .map_err(|e| crate::error::AppError::Other(format!("pre-request script task panicked: {e}")))??;
        // Collect env mutations so post-script and interpolation see them.
        for (k, v) in &r.env_mutations {
            env_overrides.insert(k.clone(), v.clone());
        }
        if r.aborted {
            return Err(crate::error::AppError::Other(
                "Request aborted by pre-request script".into(),
            ));
        }
        Some(r)
    } else {
        None
    };

    // Re-interpolate with any env mutations the pre-script applied.
    if !env_overrides.is_empty() {
        let merged: HashMap<String, String> = resolved.values.iter()
            .chain(env_overrides.iter())
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        crate::vars::interpolate_request(&mut req, &merged);
    }

    // 4b. Apply per-workspace transport config: inline default headers
    //     (user headers win) and resolve proxy + mTLS identity from the
    //     workspace settings row, hydrating any pasted PEM bytes from the
    //     keychain / reading path-mode PEMs from disk.
    let transport = {
        let conn = state.db.lock().unwrap();
        crate::workspace::apply_default_headers(&mut req, &conn, &workspace_id)?;
        crate::workspace::resolve_transport(&conn, &workspace_id)?
    };

    // 5. Send.
    let label = name.unwrap_or_else(|| format!("{} {}", req.method, req.url));
    let result = crate::engine::http::send(req.clone(), Some(Arc::clone(&state.cookie_jar)), transport.as_ref()).await;

    // 6. Run post-response script (only if the send succeeded).
    let post_result = if !post_script_src.trim().is_empty() {
        if let Ok(resp) = &result {
            let body = base64::engine::general_purpose::STANDARD
                .decode(&resp.body_base64)
                .ok()
                .and_then(|b| String::from_utf8(b).ok())
                .unwrap_or_default();
            let ctx = PostScriptContext {
                method: req.method.clone(),
                url: req.url.clone(),
                request_headers: req.headers.iter().filter(|h| h.enabled)
                    .map(|h| (h.name.clone(), h.value.clone()))
                    .collect(),
                status: resp.status,
                status_text: resp.status_text.clone(),
                response_headers: resp.headers.iter()
                    .map(|h| (h.name.clone(), h.value.clone()))
                    .collect(),
                body,
                duration_ms: resp.timing.total_ms,
                env: {
                    let mut e = resolved.values.clone();
                    e.extend(env_overrides.clone());
                    e
                },
            };
            Some(
                tokio::task::spawn_blocking(move || run_post_script(&post_script_src, &ctx))
                    .await
                    .map_err(|e| crate::error::AppError::Other(format!("post-response script task panicked: {e}")))??,
            )
        } else {
            None
        }
    } else {
        None
    };

    // 7. Persist to history (best-effort — failures are dropped).
    let mut history_req = crate::vars::redact_request(&req, &resolved.secrets);
    history_req.auth = auth::mask_secrets(req.auth.clone());

    // Merge pre + post test results for the history row.
    let all_tests: Vec<crate::scripting::TestResult> = pre_result
        .as_ref()
        .map(|r| r.tests.clone())
        .unwrap_or_default()
        .into_iter()
        .chain(
            post_result.as_ref()
                .map(|r| r.tests.clone())
                .unwrap_or_default()
                .into_iter(),
        )
        .collect();
    let test_results_json = if all_tests.is_empty() {
        None
    } else {
        serde_json::to_string(&all_tests).ok()
    };

    let conn = state.db.lock().unwrap();
    match &result {
        Ok(resp) => {
            let _ = history::insert_with_tests(
                &conn,
                &workspace_id,
                request_id.as_deref(),
                &label,
                &history_req,
                Some(resp),
                None,
                test_results_json.as_deref(),
            );
            if let Some(id) = request_id.as_deref() {
                let _ = requests::touch_last_used(&conn, id);
            }
        }
        Err(e) => {
            let _ = history::insert_with_tests(
                &conn,
                &workspace_id,
                request_id.as_deref(),
                &label,
                &history_req,
                None,
                Some(&e.to_string()),
                None,
            );
        }
    }
    drop(conn);

    result.map(|response| SendResponse { response, pre_script: pre_result, post_script: post_result })
}

/// Resolves owner + effective `AuthConfig` for a request (collection→request
/// inheritance, see `auth::resolve`) and recovers any real secret from the
/// keychain. Shared by `send_request` and `commands::oauth::start_oauth2_authorization`.
pub(crate) fn resolve_owner_and_config(
    state: &State<'_, AppState>,
    collection_id: Option<&str>,
    request_id: Option<&str>,
) -> AppResult<(String, AuthConfig)> {
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

/// Resolves the request's effective auth, recovers secrets from the keychain,
/// and collapses OAuth2 to a bearer token. Never holds the DB lock across an
/// `.await`.
async fn resolve_auth(
    state: &State<'_, AppState>,
    collection_id: Option<&str>,
    request_id: Option<&str>,
) -> AppResult<AuthConfig> {
    let (owner, hydrated) = resolve_owner_and_config(state, collection_id, request_id)?;
    match hydrated {
        AuthConfig::OAuth2(cfg) => collapse_oauth2(state, &owner, &cfg).await,
        other => Ok(other),
    }
}

/// Returns a cached token if still fresh, refreshes if expired+refreshable,
/// otherwise runs a from-scratch grant exchange, caches, and returns Bearer.
async fn collapse_oauth2(
    state: &State<'_, AppState>,
    owner: &str,
    cfg: &OAuth2Config,
) -> AppResult<AuthConfig> {
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

/// Clear all cookies from the shared jar.
#[tauri::command]
pub fn clear_cookies(state: State<'_, AppState>) -> AppResult<()> {
    state
        .cookie_jar
        .lock()
        .map_err(|e| crate::error::AppError::Other(format!("cookie jar lock poisoned: {e}")))?
        .clear();
    Ok(())
}
