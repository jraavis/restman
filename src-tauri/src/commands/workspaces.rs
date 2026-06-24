use crate::error::AppResult;
use crate::model::{Workspace, WorkspaceSettings};
use crate::store::{workspaces, workspace_settings, AppState};
use tauri::State;

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
pub fn update_workspace(state: State<AppState>, id: String, name: String) -> AppResult<Workspace> {
    let conn = state.db.lock().unwrap();
    workspaces::update(&conn, &id, &name)
}

#[tauri::command]
pub fn delete_workspace(state: State<AppState>, id: String) -> AppResult<()> {
    let mut conn = state.db.lock().unwrap();
    // Cascade the keychain cleanup for any pasted client-cert slots the
    // workspace owned (store layer FK-cascades the row, but the OS
    // credential store has no cascade).
    workspace_settings::delete_cert_secrets(&id).ok();
    workspaces::delete(&mut conn, &id)
}

#[tauri::command]
pub fn set_active_workspace(state: State<AppState>, id: String) -> AppResult<()> {
    let mut conn = state.db.lock().unwrap();
    workspaces::set_active(&mut conn, &id)
}

/// Per-workspace transport settings (proxy / default headers / mTLS client
/// cert). The returned settings carry masked/empty PEM fields for display —
/// send-time hydration reads the real bytes from the keychain.
#[tauri::command]
pub fn get_workspace_settings(state: State<AppState>, workspace_id: String) -> AppResult<WorkspaceSettings> {
    let conn = state.db.lock().unwrap();
    workspace_settings::get(&conn, &workspace_id)
}

/// Persist transport settings. Plaintext PEM bytes (Paste mode) route to the
/// keychain before the (masked) row is written — see
/// `store::workspace_settings::set`.
#[tauri::command]
pub fn set_workspace_settings(
    state: State<AppState>,
    settings: WorkspaceSettings,
) -> AppResult<WorkspaceSettings> {
    let conn = state.db.lock().unwrap();
    workspace_settings::set(&conn, &settings)
}
