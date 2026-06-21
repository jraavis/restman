use crate::error::AppResult;
use crate::model::http::{HttpRequest, HttpResponse};
use crate::store::{history, requests, AppState};
use tauri::State;

/// Liveness check used to verify the IPC bridge end-to-end.
#[tauri::command]
pub fn ping() -> String {
    "pong".into()
}

/// Resolve `{{var}}`s for the given workspace/collection, send, and record
/// the attempt to history regardless of outcome.
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

    let label = name.unwrap_or_else(|| format!("{} {}", req.method, req.url));
    let result = crate::engine::http::send(req.clone()).await;

    // Secrets are redacted out of the copy written to history — see
    // `vars::redact_request` — so the live `req` above keeps real values
    // (needed for the send) while the persisted row never does.
    let history_req = crate::vars::redact_request(&req, &resolved.secrets);
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
