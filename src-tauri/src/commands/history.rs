use crate::error::AppResult;
use crate::model::http::HttpResponse;
use crate::model::{HistoryEntry, HistoryFilter};
use crate::store::{history, AppState};
use std::sync::Arc;
use tauri::State;

#[tauri::command]
pub fn list_history(state: State<AppState>, workspace_id: String, filter: HistoryFilter) -> AppResult<Vec<HistoryEntry>> {
    let conn = state.db.lock().unwrap();
    history::list(&conn, &workspace_id, &filter)
}

#[tauri::command]
pub fn delete_history_entry(state: State<AppState>, id: String) -> AppResult<()> {
    let conn = state.db.lock().unwrap();
    history::delete(&conn, &id)
}

#[tauri::command]
pub fn clear_history(state: State<AppState>, workspace_id: String) -> AppResult<()> {
    let conn = state.db.lock().unwrap();
    history::clear(&conn, &workspace_id)
}

#[tauri::command]
pub fn get_history_retention(state: State<AppState>) -> AppResult<i64> {
    let conn = state.db.lock().unwrap();
    Ok(history::get_retention(&conn))
}

#[tauri::command]
pub fn set_history_retention(state: State<AppState>, count: i64) -> AppResult<()> {
    let conn = state.db.lock().unwrap();
    history::set_retention(&conn, count)
}

/// Re-send a history entry's captured request exactly as it was sent, and
/// record the new attempt as its own history entry.
#[tauri::command]
pub async fn replay_history_entry(state: State<'_, AppState>, id: String) -> AppResult<HttpResponse> {
    let entry = {
        let conn = state.db.lock().unwrap();
        history::get(&conn, &id)?
    };
    let result = crate::engine::http::send(entry.request.clone(), Some(Arc::clone(&state.cookie_jar))).await;
    {
        let conn = state.db.lock().unwrap();
        match &result {
            Ok(resp) => {
                let _ = history::insert(
                    &conn,
                    &entry.workspace_id,
                    entry.request_id.as_deref(),
                    &entry.name,
                    &entry.request,
                    Some(resp),
                    None,
                );
            }
            Err(e) => {
                let _ = history::insert(
                    &conn,
                    &entry.workspace_id,
                    entry.request_id.as_deref(),
                    &entry.name,
                    &entry.request,
                    None,
                    Some(&e.to_string()),
                );
            }
        }
    }
    result
}
