use crate::error::AppResult;
use crate::model::Tag;
use crate::store::{tags, AppState};
use tauri::State;

#[tauri::command]
pub fn list_tags(state: State<AppState>, workspace_id: String) -> AppResult<Vec<Tag>> {
    let conn = state.db.lock().unwrap();
    tags::list(&conn, &workspace_id)
}

#[tauri::command]
pub fn create_tag(state: State<AppState>, workspace_id: String, name: String, color: String) -> AppResult<Tag> {
    let conn = state.db.lock().unwrap();
    tags::create(&conn, &workspace_id, &name, &color)
}

#[tauri::command]
pub fn update_tag(state: State<AppState>, id: String, name: String, color: String) -> AppResult<()> {
    let conn = state.db.lock().unwrap();
    tags::update(&conn, &id, &name, &color)
}

#[tauri::command]
pub fn delete_tag(state: State<AppState>, id: String) -> AppResult<()> {
    let conn = state.db.lock().unwrap();
    tags::delete(&conn, &id)
}
