//! Mock server CRUD + start/stop. Thin wrappers over `store::mock_servers`
//! (persistence) and `engine::mock` (the live socket), mirroring every other
//! domain's command-module shape in this codebase.

use crate::engine::mock;
use crate::error::{AppError, AppResult};
use crate::model::{MockRule, MockRuleInput, MockServer, MockServerInput};
use crate::store::{self, AppState};
use tauri::State;

#[tauri::command]
pub fn list_mock_servers(state: State<'_, AppState>, workspace_id: String) -> AppResult<Vec<MockServer>> {
    let conn = state.db.lock().unwrap();
    store::mock_servers::list_servers(&conn, &workspace_id)
}

#[tauri::command]
pub fn create_mock_server(
    state: State<'_, AppState>,
    workspace_id: String,
    input: MockServerInput,
) -> AppResult<MockServer> {
    let conn = state.db.lock().unwrap();
    store::mock_servers::create_server(&conn, &workspace_id, &input)
}

#[tauri::command]
pub fn create_mock_server_from_collection(
    state: State<'_, AppState>,
    workspace_id: String,
    collection_id: String,
    name: String,
    port: u16,
) -> AppResult<MockServer> {
    let conn = state.db.lock().unwrap();
    store::mock_servers::create_from_collection(&conn, &workspace_id, &collection_id, &name, port)
}

#[tauri::command]
pub fn update_mock_server(state: State<'_, AppState>, id: String, input: MockServerInput) -> AppResult<MockServer> {
    let conn = state.db.lock().unwrap();
    store::mock_servers::update_server(&conn, &id, &input)
}

/// Deletes the server config. If it's currently running, stops it first —
/// a deleted config left running would be an orphaned, unstoppable-from-the-
/// UI socket the user can no longer see or reach.
#[tauri::command]
pub fn delete_mock_server(state: State<'_, AppState>, id: String) -> AppResult<()> {
    {
        let mut running = state.mock_servers.lock().unwrap();
        if let Some(server) = running.remove(&id) {
            server.abort();
        }
    }
    let conn = state.db.lock().unwrap();
    store::mock_servers::delete_server(&conn, &id)
}

#[tauri::command]
pub fn list_mock_rules(state: State<'_, AppState>, mock_server_id: String) -> AppResult<Vec<MockRule>> {
    let conn = state.db.lock().unwrap();
    store::mock_servers::list_rules(&conn, &mock_server_id)
}

#[tauri::command]
pub fn create_mock_rule(
    state: State<'_, AppState>,
    mock_server_id: String,
    input: MockRuleInput,
) -> AppResult<MockRule> {
    let conn = state.db.lock().unwrap();
    store::mock_servers::create_rule(&conn, &mock_server_id, &input)
}

#[tauri::command]
pub fn update_mock_rule(state: State<'_, AppState>, id: String, input: MockRuleInput) -> AppResult<MockRule> {
    let conn = state.db.lock().unwrap();
    store::mock_servers::update_rule(&conn, &id, &input)
}

#[tauri::command]
pub fn delete_mock_rule(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let conn = state.db.lock().unwrap();
    store::mock_servers::delete_rule(&conn, &id)
}

/// Starts serving `id`'s configured rules on its configured port. Errors if
/// it's already running (call `stop_mock_server` first to restart with
/// edited rules) or if the port can't be bound (already in use, etc.) — a
/// bind failure is surfaced to the user, never silently swallowed into
/// "started" state.
#[tauri::command]
pub async fn start_mock_server(state: State<'_, AppState>, id: String) -> AppResult<u16> {
    {
        let running = state.mock_servers.lock().unwrap();
        if running.contains_key(&id) {
            return Err(AppError::Other(format!("mock server {id} is already running")));
        }
    }
    let (server, rules) = {
        let conn = state.db.lock().unwrap();
        let server = store::mock_servers::get_server(&conn, &id)?;
        let rules = store::mock_servers::list_rules(&conn, &id)?;
        (server, rules)
    };

    let router = mock::build_router(rules);
    let running_server = mock::serve(router, server.port)
        .await
        .map_err(|e| AppError::Other(format!("failed to start mock server on port {}: {e}", server.port)))?;
    let bound_port = running_server.addr.port();

    state.mock_servers.lock().unwrap().insert(id, running_server);
    Ok(bound_port)
}

#[tauri::command]
pub fn stop_mock_server(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let mut running = state.mock_servers.lock().unwrap();
    match running.remove(&id) {
        Some(server) => {
            server.abort();
            Ok(())
        }
        None => Err(AppError::NotFound(format!("mock server {id} is not running"))),
    }
}

/// Every currently-running mock server's id — the frontend cross-references
/// this against its own `list_mock_servers` result rather than this command
/// taking a `workspace_id` itself, since `AppState.mock_servers` isn't
/// workspace-partitioned (this app doesn't run enough concurrent mock
/// servers for that to matter).
#[tauri::command]
pub fn list_running_mock_server_ids(state: State<'_, AppState>) -> Vec<String> {
    state.mock_servers.lock().unwrap().keys().cloned().collect()
}

/// Serializes a mock server's config (name/port/rules, including every
/// matcher field) to a JSON string for saving to disk — same shareable-
/// backup convention as `export_environment`. No secrets live in a mock
/// rule's fields, so unlike environment export this needs no masking.
#[tauri::command]
pub fn export_mock_server(state: State<'_, AppState>, id: String) -> AppResult<String> {
    let conn = state.db.lock().unwrap();
    store::mock_servers::export_server(&conn, &id)
}

/// Creates a new mock server (and its rules) from a previously exported
/// JSON string, into `workspace_id`.
#[tauri::command]
pub fn import_mock_server(state: State<'_, AppState>, workspace_id: String, content: String) -> AppResult<MockServer> {
    let conn = state.db.lock().unwrap();
    store::mock_servers::import_server(&conn, &workspace_id, &content)
}
