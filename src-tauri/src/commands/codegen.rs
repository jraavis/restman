//! Code generation IPC: resolve vars + auth into an `HttpRequest` the same
//! way `send_request` would, then hand off to the pure `codegen::generate`.
//! Deliberately skips `send_request`'s full `resolve_auth` — that collapses
//! OAuth2 via a live token exchange, which would fire a network request just
//! to preview code. Here OAuth2 reuses a fresh cached token (DB/keychain
//! only) if one exists, else falls back to a visible placeholder.

use crate::auth::oauth::token_store;
use crate::codegen::{self, CodeLanguage, CodegenOptions, OAUTH2_TOKEN_PLACEHOLDER};
use crate::commands::http::resolve_owner_and_config;
use crate::error::AppResult;
use crate::model::auth::AuthConfig;
use crate::model::http::HttpRequest;
use crate::store::AppState;
use tauri::State;

#[tauri::command]
pub fn generate_code(
    state: State<'_, AppState>,
    mut req: HttpRequest,
    workspace_id: String,
    collection_id: Option<String>,
    request_id: Option<String>,
    language: CodeLanguage,
    options: CodegenOptions,
) -> AppResult<String> {
    let resolved = {
        let conn = state.db.lock().unwrap();
        crate::vars::resolve(&conn, &workspace_id, collection_id.as_deref())?
    };
    crate::vars::interpolate_request(&mut req, &resolved.values);

    let (owner, hydrated) = resolve_owner_and_config(&state, collection_id.as_deref(), request_id.as_deref())?;
    req.auth = match hydrated {
        AuthConfig::OAuth2(_) => {
            let conn = state.db.lock().unwrap();
            let cached = token_store::get(&conn, &owner)?;
            let token = match cached {
                Some(t) if token_store::is_fresh(&t) => t.access_token,
                _ => OAUTH2_TOKEN_PLACEHOLDER.to_string(),
            };
            AuthConfig::Bearer { token }
        }
        other => other,
    };

    codegen::generate(language, &req, &options)
}
