use crate::error::AppResult;
use crate::model::Environment;
use crate::store::{environments, AppState};
use tauri::State;

#[tauri::command]
pub fn list_environments(state: State<AppState>, workspace_id: String) -> AppResult<Vec<Environment>> {
    let conn = state.db.lock().unwrap();
    environments::list(&conn, &workspace_id)
}

#[tauri::command]
pub fn create_environment(
    state: State<AppState>,
    workspace_id: String,
    collection_id: Option<String>,
    name: String,
    group_name: Option<String>,
) -> AppResult<Environment> {
    let conn = state.db.lock().unwrap();
    environments::create(&conn, &workspace_id, collection_id.as_deref(), &name, group_name.as_deref())
}

#[tauri::command]
pub fn update_environment(
    state: State<AppState>,
    id: String,
    name: String,
    group_name: Option<String>,
) -> AppResult<Environment> {
    let conn = state.db.lock().unwrap();
    environments::update(&conn, &id, &name, group_name.as_deref())
}

#[tauri::command]
pub fn delete_environment(state: State<AppState>, id: String) -> AppResult<()> {
    let conn = state.db.lock().unwrap();
    environments::delete(&conn, &id)
}

#[tauri::command]
pub fn set_active_environment(state: State<AppState>, workspace_id: String, id: Option<String>) -> AppResult<()> {
    let mut conn = state.db.lock().unwrap();
    environments::set_active(&mut conn, &workspace_id, id.as_deref())
}

#[tauri::command]
pub fn active_environment(state: State<AppState>, workspace_id: String) -> AppResult<Option<Environment>> {
    let conn = state.db.lock().unwrap();
    environments::active_for_workspace(&conn, &workspace_id)
}
