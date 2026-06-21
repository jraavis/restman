use crate::error::AppResult;
use crate::model::{SavedRequest, SavedRequestInput};
use crate::store::requests::SearchHit;
use crate::store::{requests, AppState};
use tauri::State;

#[tauri::command]
pub fn list_requests(state: State<AppState>, collection_id: String) -> AppResult<Vec<SavedRequest>> {
    let conn = state.db.lock().unwrap();
    requests::list_by_collection(&conn, &collection_id)
}

#[tauri::command]
pub fn get_request(state: State<AppState>, id: String) -> AppResult<SavedRequest> {
    let conn = state.db.lock().unwrap();
    requests::get(&conn, &id)
}

#[tauri::command]
pub fn create_request(state: State<AppState>, collection_id: String, input: SavedRequestInput) -> AppResult<SavedRequest> {
    let conn = state.db.lock().unwrap();
    requests::create(&conn, &collection_id, &input)
}

#[tauri::command]
pub fn update_request(state: State<AppState>, id: String, input: SavedRequestInput) -> AppResult<SavedRequest> {
    let conn = state.db.lock().unwrap();
    requests::update(&conn, &id, &input)
}

#[tauri::command]
pub fn delete_request(state: State<AppState>, id: String) -> AppResult<()> {
    let conn = state.db.lock().unwrap();
    requests::delete(&conn, &id)
}

#[tauri::command]
pub fn move_request(state: State<AppState>, id: String, collection_id: String) -> AppResult<SavedRequest> {
    let conn = state.db.lock().unwrap();
    requests::move_to(&conn, &id, &collection_id)
}

#[tauri::command]
pub fn reorder_requests(state: State<AppState>, ids: Vec<String>) -> AppResult<()> {
    let conn = state.db.lock().unwrap();
    requests::reorder(&conn, &ids)
}

#[tauri::command]
pub fn duplicate_request(state: State<AppState>, id: String, new_name: Option<String>) -> AppResult<SavedRequest> {
    let conn = state.db.lock().unwrap();
    requests::duplicate(&conn, &id, new_name.as_deref())
}

#[tauri::command]
pub fn set_request_tags(state: State<AppState>, request_id: String, tag_ids: Vec<String>) -> AppResult<()> {
    let conn = state.db.lock().unwrap();
    requests::set_tags(&conn, &request_id, &tag_ids)
}

#[tauri::command]
pub fn search_requests(
    state: State<AppState>,
    workspace_id: String,
    query: String,
    method: Option<String>,
) -> AppResult<Vec<SearchHit>> {
    let conn = state.db.lock().unwrap();
    requests::search(&conn, &workspace_id, &query, method.as_deref())
}
