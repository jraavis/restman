//! Tauri IPC command handlers. Thin wrappers over the store/engine layers.

use crate::error::AppResult;
use crate::model::http::{HttpRequest, HttpResponse};
use crate::model::Workspace;
use crate::store::{workspaces, AppState};
use tauri::State;

/// Liveness check used to verify the IPC bridge end-to-end.
#[tauri::command]
pub fn ping() -> String {
    "pong".into()
}

/// Send an HTTP request and return the response with timing.
#[tauri::command]
pub async fn send_request(req: HttpRequest) -> AppResult<HttpResponse> {
    crate::engine::http::send(req).await
}

#[tauri::command]
pub fn list_workspaces(state: State<AppState>) -> AppResult<Vec<Workspace>> {
    let conn = state.db.lock().unwrap();
    workspaces::list(&conn)
}

#[tauri::command]
pub fn active_workspace(state: State<AppState>) -> AppResult<Option<Workspace>> {
    let conn = state.db.lock().unwrap();
    workspaces::active(&conn)
}

#[tauri::command]
pub fn create_workspace(state: State<AppState>, name: String) -> AppResult<Workspace> {
    let conn = state.db.lock().unwrap();
    workspaces::create(&conn, &name)
}

#[tauri::command]
pub fn set_active_workspace(state: State<AppState>, id: String) -> AppResult<()> {
    let mut conn = state.db.lock().unwrap();
    workspaces::set_active(&mut conn, &id)
}
