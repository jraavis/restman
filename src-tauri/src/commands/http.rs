use crate::auth;
use crate::auth::oauth::{self, token_store};
use crate::error::AppResult;
use crate::model::auth::{AuthConfig, OAuth2Config, RequestAuth};
use crate::model::http::{CookieEntry, HttpRequest, HttpResponse};
use crate::scripting::{
    run_post_script, run_pre_script, PostScriptContext, PreScriptContext, ScriptResult,
};
use crate::model::{VarScope, VarType, VariableInput};
use crate::store::{collections, environments, history, requests, variables, AppState};
use rusqlite::Connection;
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
#[allow(clippy::too_many_arguments)]
pub async fn send_request(
    state: State<'_, AppState>,
    mut req: HttpRequest,
    workspace_id: String,
    collection_id: Option<String>,
    request_id: Option<String>,
    name: Option<String>,
    pre_request_script: Option<String>,
    post_response_script: Option<String>,
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

    // 3. Load scripts — prefer non-empty draft overrides, else saved request.
    let (pre_script_src, post_script_src) = {
        let (saved_pre, saved_post) = if let Some(rid) = request_id.as_deref() {
            let conn = state.db.lock().unwrap();
            let saved = requests::get(&conn, rid)?;
            (saved.pre_request_script, saved.post_response_script)
        } else {
            (String::new(), String::new())
        };
        let pre = pre_request_script
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(saved_pre);
        let post = post_response_script
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(saved_post);
        (pre, post)
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

    // 7b. Persist script env changes (best-effort).
    let mut env_mutations: Vec<(String, String)> = Vec::new();
    let mut env_unsets: Vec<String> = Vec::new();
    for r in pre_result.iter().chain(post_result.iter()) {
        for (k, v) in &r.env_mutations {
            env_mutations.retain(|(key, _)| key != k);
            env_mutations.push((k.clone(), v.clone()));
            env_unsets.retain(|key| key != k);
        }
        for k in &r.env_unsets {
            env_mutations.retain(|(key, _)| key != k);
            if !env_unsets.contains(k) {
                env_unsets.push(k.clone());
            }
        }
    }
    let _ = persist_env_mutations(
        &conn,
        &workspace_id,
        collection_id.as_deref(),
        &env_mutations,
        &env_unsets,
    );

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

/// Target scope for script-persisted variables: active environment when it
/// applies to the send's collection, otherwise workspace scope.
fn persist_scope(
    conn: &Connection,
    workspace_id: &str,
    collection_id: Option<&str>,
) -> AppResult<VarScope> {
    if let Some(env) = environments::active_for_workspace(conn, workspace_id)? {
        let applies = match env.collection_id.as_deref() {
            None => true,
            Some(env_cid) => collection_id == Some(env_cid),
        };
        if applies {
            return Ok(VarScope::Environment(env.id));
        }
    }
    Ok(VarScope::Workspace(workspace_id.to_string()))
}

/// Apply `pm.environment` mutations from a script run to the variables table.
pub(crate) fn persist_env_mutations(
    conn: &Connection,
    workspace_id: &str,
    collection_id: Option<&str>,
    mutations: &[(String, String)],
    unsets: &[String],
) -> AppResult<()> {
    if mutations.is_empty() && unsets.is_empty() {
        return Ok(());
    }
    let scope = persist_scope(conn, workspace_id, collection_id)?;
    let existing: HashMap<String, crate::model::Variable> = variables::list(conn, &scope)?
        .into_iter()
        .map(|v| (v.key.clone(), v))
        .collect();

    for (key, value) in mutations {
        if let Some(var) = existing.get(key) {
            variables::update(
                conn,
                &var.id,
                &VariableInput {
                    key: key.clone(),
                    value: value.clone(),
                    var_type: var.var_type,
                    is_secret: var.is_secret,
                    enabled: var.enabled,
                },
            )?;
        } else {
            variables::create(
                conn,
                &scope,
                &VariableInput {
                    key: key.clone(),
                    value: value.clone(),
                    var_type: VarType::String,
                    is_secret: false,
                    enabled: true,
                },
            )?;
        }
    }

    for key in unsets {
        if let Some(var) = existing.get(key) {
            variables::delete(conn, &var.id)?;
        }
    }

    Ok(())
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
/// `.await`. Shared by `send_request` and `commands::graphql::introspect_graphql_schema`
/// (introspection is a genuine live fetch, unlike codegen's preview-only path,
/// so it needs the real OAuth2 exchange here, not a cached-or-placeholder token).
pub(crate) async fn resolve_auth(
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

/// List all unexpired cookies currently held in the shared jar, sorted by
/// domain then name for stable display.
#[tauri::command]
pub fn list_cookies(state: State<'_, AppState>) -> AppResult<Vec<CookieEntry>> {
    let jar = state
        .cookie_jar
        .lock()
        .map_err(|e| crate::error::AppError::Other(format!("cookie jar lock poisoned: {e}")))?;
    let mut cookies: Vec<CookieEntry> = jar
        .iter_unexpired()
        .map(|c| CookieEntry {
            name: c.name().to_string(),
            value: c.value().to_string(),
            domain: String::from(&c.domain),
            path: String::from(&c.path),
            secure: c.secure().unwrap_or(false),
            http_only: c.http_only().unwrap_or(false),
            same_site: c.same_site().map(|s| s.to_string()),
            expires_at: match c.expires {
                cookie_store::CookieExpiration::AtUtc(t) => Some(t.unix_timestamp()),
                cookie_store::CookieExpiration::SessionEnd => None,
            },
        })
        .collect();
    cookies.sort_by(|a, b| (&a.domain, &a.name).cmp(&(&b.domain, &b.name)));
    Ok(cookies)
}

/// Remove a single cookie identified by its domain/path/name triple.
#[tauri::command]
pub fn delete_cookie(state: State<'_, AppState>, domain: String, path: String, name: String) -> AppResult<()> {
    state
        .cookie_jar
        .lock()
        .map_err(|e| crate::error::AppError::Other(format!("cookie jar lock poisoned: {e}")))?
        .remove(&domain, &path, &name);
    Ok(())
}

#[cfg(test)]
mod persist_tests {
    use super::persist_env_mutations;
    use crate::model::{VarScope, VarType, VariableInput};
    use crate::store::{collections, environments, variables, workspaces};

    #[test]
    fn persists_to_active_environment_when_applicable() {
        let mut conn = crate::store::db::open_in_memory().unwrap();
        let ws = workspaces::ensure_default(&mut conn).unwrap();
        let env = environments::create(&conn, &ws.id, None, "Dev", None).unwrap();
        environments::set_active(&mut conn, &ws.id, Some(&env.id)).unwrap();

        persist_env_mutations(&conn, &ws.id, None, &[("token".into(), "abc".into())], &[]).unwrap();

        let vars = variables::list(&conn, &VarScope::Environment(env.id)).unwrap();
        assert_eq!(vars.len(), 1);
        assert_eq!(vars[0].key, "token");
        assert_eq!(vars[0].value, "abc");
    }

    #[test]
    fn falls_back_to_workspace_when_env_not_applicable() {
        let mut conn = crate::store::db::open_in_memory().unwrap();
        let ws = workspaces::ensure_default(&mut conn).unwrap();
        let col = collections::create(&conn, &ws.id, None, "API", None).unwrap();
        let env = environments::create(&conn, &ws.id, Some(&col.id), "Col env", None).unwrap();
        environments::set_active(&mut conn, &ws.id, Some(&env.id)).unwrap();

        persist_env_mutations(&conn, &ws.id, None, &[("token".into(), "ws-val".into())], &[]).unwrap();

        let ws_vars = variables::list(&conn, &VarScope::Workspace(ws.id.clone())).unwrap();
        assert_eq!(ws_vars.len(), 1);
        assert_eq!(ws_vars[0].value, "ws-val");
        assert!(variables::list(&conn, &VarScope::Environment(env.id)).unwrap().is_empty());
    }

    #[test]
    fn updates_existing_preserving_type_and_secret() {
        let mut conn = crate::store::db::open_in_memory().unwrap();
        let ws = workspaces::ensure_default(&mut conn).unwrap();
        let var = variables::create(
            &conn,
            &VarScope::Workspace(ws.id.clone()),
            &VariableInput {
                key: "token".into(),
                value: "old".into(),
                var_type: VarType::Json,
                is_secret: true,
                enabled: true,
            },
        )
        .unwrap();

        persist_env_mutations(&conn, &ws.id, None, &[("token".into(), "new".into())], &[]).unwrap();

        let updated = variables::get(&conn, &var.id).unwrap();
        assert_eq!(updated.var_type, VarType::Json);
        assert!(updated.is_secret);
        assert_eq!(updated.value, "");
        let secret = crate::secrets::get(&variables::keychain_key(&var.id)).unwrap().unwrap();
        assert_eq!(secret, "new");
    }

    #[test]
    fn unset_deletes_variable() {
        let mut conn = crate::store::db::open_in_memory().unwrap();
        let ws = workspaces::ensure_default(&mut conn).unwrap();
        variables::create(
            &conn,
            &VarScope::Workspace(ws.id.clone()),
            &VariableInput {
                key: "token".into(),
                value: "gone".into(),
                var_type: VarType::String,
                is_secret: false,
                enabled: true,
            },
        )
        .unwrap();

        persist_env_mutations(&conn, &ws.id, None, &[], &["token".into()]).unwrap();

        assert!(variables::list(&conn, &VarScope::Workspace(ws.id)).unwrap().is_empty());
    }
}

#[cfg(test)]
mod cookie_tests {
    use cookie_store::CookieStore;
    use url::Url;

    /// `list_cookies` extracts `domain`/`path`/`name` via `String::from(&c.domain)` /
    /// `String::from(&c.path)` / `c.name().to_string()` — the same conversions
    /// `CookieStore::insert` uses as its map keys — and `delete_cookie` forwards them
    /// verbatim to `remove()`. This locks that the round-trip actually removes the
    /// cookie instead of silently no-op'ing on a mismatched key.
    #[test]
    fn list_then_delete_round_trip_removes_cookie() {
        let mut store = CookieStore::default();
        let url = Url::parse("https://example.com/").unwrap();
        store.parse("session_id=abc123", &url).unwrap();

        let c = store.iter_unexpired().next().unwrap();
        let (domain, path, name) = (
            String::from(&c.domain),
            String::from(&c.path),
            c.name().to_string(),
        );

        assert!(store.remove(&domain, &path, &name).is_some());
        assert_eq!(store.iter_unexpired().count(), 0);
    }
}
