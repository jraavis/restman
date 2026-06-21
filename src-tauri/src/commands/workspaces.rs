use crate::error::AppResult;
use crate::model::Workspace;
use crate::store::{workspaces, AppState};
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
    workspaces::delete(&mut conn, &id)
}

#[tauri::command]
pub fn set_active_workspace(state: State<AppState>, id: String) -> AppResult<()> {
    let mut conn = state.db.lock().unwrap();
    workspaces::set_active(&mut conn, &id)
}
