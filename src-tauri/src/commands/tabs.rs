use crate::error::AppResult;
use crate::model::http::HttpRequest;
use crate::model::Tab;
use crate::store::{tabs, AppState};
use tauri::State;

#[tauri::command]
pub fn list_tabs(state: State<AppState>, workspace_id: String) -> AppResult<Vec<Tab>> {
    let conn = state.db.lock().unwrap();
    tabs::list(&conn, &workspace_id)
}

#[tauri::command]
pub fn create_tab(
    state: State<AppState>,
    workspace_id: String,
    request_id: Option<String>,
    title: String,
    draft: HttpRequest,
) -> AppResult<Tab> {
    let mut conn = state.db.lock().unwrap();
    tabs::create(&mut conn, &workspace_id, request_id.as_deref(), &title, &draft)
}

#[tauri::command]
pub fn update_tab_draft(state: State<AppState>, id: String, title: String, draft: HttpRequest) -> AppResult<Tab> {
    let conn = state.db.lock().unwrap();
    tabs::update_draft(&conn, &id, &title, &draft)
}

#[tauri::command]
pub fn set_tab_request_id(state: State<AppState>, id: String, request_id: String) -> AppResult<Tab> {
    let conn = state.db.lock().unwrap();
    tabs::set_request_id(&conn, &id, &request_id)
}

#[tauri::command]
pub fn set_active_tab(state: State<AppState>, workspace_id: String, id: String) -> AppResult<()> {
    let mut conn = state.db.lock().unwrap();
    tabs::set_active(&mut conn, &workspace_id, &id)
}

#[tauri::command]
pub fn reorder_tabs(state: State<AppState>, ids: Vec<String>) -> AppResult<()> {
    let conn = state.db.lock().unwrap();
    tabs::reorder(&conn, &ids)
}

#[tauri::command]
pub fn close_tab(state: State<AppState>, workspace_id: String, id: String) -> AppResult<()> {
    let mut conn = state.db.lock().unwrap();
    tabs::close(&mut conn, &workspace_id, &id)
}

#[tauri::command]
pub fn close_other_tabs(state: State<AppState>, workspace_id: String, keep_id: String) -> AppResult<()> {
    let conn = state.db.lock().unwrap();
    tabs::close_others(&conn, &workspace_id, &keep_id)
}

#[tauri::command]
pub fn close_all_tabs(state: State<AppState>, workspace_id: String) -> AppResult<()> {
    let conn = state.db.lock().unwrap();
    tabs::close_all(&conn, &workspace_id)
}
