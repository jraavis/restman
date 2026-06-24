use crate::error::AppResult;
use crate::interop::{self, environment, ConflictMode, ExportFormat, ImportFormat, ImportPreview, ImportReport, ImportedNode, EnvironmentImportReport, EnvironmentPreview};
use crate::store::AppState;
use tauri::State;

/// Parse raw file content into a preview tree. No DB access — the frontend
/// renders the result and lets the user confirm before `apply_collection_import`.
#[tauri::command]
pub fn preview_import(format: ImportFormat, content: String) -> AppResult<ImportPreview> {
    interop::parse(format, &content)
}

/// Commit a previously-previewed tree under `parent_id` (`None` = workspace
/// top level).
#[tauri::command]
pub fn apply_collection_import(
    state: State<AppState>,
    workspace_id: String,
    parent_id: Option<String>,
    root: ImportedNode,
    mode: ConflictMode,
) -> AppResult<ImportReport> {
    let conn = state.db.lock().unwrap();
    interop::apply_import(&conn, &workspace_id, parent_id.as_deref(), &root, mode)
}

/// Export a collection (and everything nested under it) to `format`'s text
/// representation.
#[tauri::command]
pub fn export_collection(state: State<AppState>, collection_id: String, format: ExportFormat) -> AppResult<String> {
    let conn = state.db.lock().unwrap();
    let node = interop::collect(&conn, &collection_id)?;
    interop::export(format, &node)
}

/// Environment import/export — the command handlers live in
/// `interop::environment` (where the pure `parse`/`apply`/`export` functions
/// are), but `lib.rs` builds its `invoke_handler!` list off the flat
/// `commands::*` re-exports, so re-surface them here.
#[tauri::command]
pub fn preview_environment_import(content: String) -> AppResult<EnvironmentPreview> {
    environment::parse(&content)
}

#[tauri::command]
pub fn apply_environment_import(
    state: State<AppState>,
    workspace_id: String,
    collection_id: Option<String>,
    preview: EnvironmentPreview,
    overwrite_existing: bool,
) -> AppResult<EnvironmentImportReport> {
    let conn = state.db.lock().unwrap();
    environment::apply_environment_import(&conn, &workspace_id, collection_id.as_deref(), &preview, overwrite_existing)
}

#[tauri::command]
pub fn export_environment(state: State<AppState>, environment_id: String) -> AppResult<String> {
    let conn = state.db.lock().unwrap();
    environment::export_environment(&conn, &environment_id)
}
