use crate::error::AppResult;
use crate::model::{AuthConfig, Collection};
use crate::store::{collections, AppState};
use tauri::State;

#[tauri::command]
pub fn list_collections(state: State<AppState>, workspace_id: String) -> AppResult<Vec<Collection>> {
    let conn = state.db.lock().unwrap();
    collections::list(&conn, &workspace_id)
}

#[tauri::command]
pub fn create_collection(
    state: State<AppState>,
    workspace_id: String,
    parent_id: Option<String>,
    name: String,
    description: Option<String>,
) -> AppResult<Collection> {
    let conn = state.db.lock().unwrap();
    collections::create(&conn, &workspace_id, parent_id.as_deref(), &name, description.as_deref())
}

#[tauri::command]
pub fn update_collection(
    state: State<AppState>,
    id: String,
    name: String,
    description: Option<String>,
) -> AppResult<Collection> {
    let conn = state.db.lock().unwrap();
    collections::update(&conn, &id, &name, description.as_deref())
}

#[tauri::command]
pub fn update_collection_auth(state: State<AppState>, id: String, auth: AuthConfig) -> AppResult<Collection> {
    let conn = state.db.lock().unwrap();
    collections::update_auth(&conn, &id, auth)
}

#[tauri::command]
pub fn delete_collection(state: State<AppState>, id: String) -> AppResult<()> {
    let conn = state.db.lock().unwrap();
    collections::delete(&conn, &id)
}

#[tauri::command]
pub fn move_collection(state: State<AppState>, id: String, new_parent_id: Option<String>) -> AppResult<Collection> {
    let conn = state.db.lock().unwrap();
    collections::move_to(&conn, &id, new_parent_id.as_deref())
}

#[tauri::command]
pub fn reorder_collections(state: State<AppState>, ids: Vec<String>) -> AppResult<()> {
    let conn = state.db.lock().unwrap();
    collections::reorder(&conn, &ids)
}

#[tauri::command]
pub fn duplicate_collection(state: State<AppState>, id: String, new_name: Option<String>) -> AppResult<Collection> {
    let conn = state.db.lock().unwrap();
    collections::duplicate(&conn, &id, new_name.as_deref())
}
