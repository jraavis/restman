use crate::commands::plugins::require_kind;
use crate::error::{AppError, AppResult};
use crate::interop::{self, environment, ConflictMode, ExportFormat, ImportFormat, ImportPreview, ImportReport, ImportedNode, EnvironmentImportReport, EnvironmentPreview};
use crate::model::PluginKind;
use crate::store::{self, AppState};
use tauri::State;

/// Parse raw file content into a preview tree. `format`/`plugin_id` are
/// mutually exclusive — exactly one must be `Some`, same convention as
/// `commands::codegen::generate_code`'s `language`/`plugin_id` pair. The
/// native-format path needs no DB access (unchanged); the plugin path needs
/// `state` to look up the plugin's source by id.
#[tauri::command]
pub fn preview_import(
    state: State<AppState>,
    format: Option<ImportFormat>,
    plugin_id: Option<String>,
    content: String,
) -> AppResult<ImportPreview> {
    match (format, plugin_id) {
        (Some(f), None) => interop::parse(f, &content),
        (None, Some(id)) => {
            let plugin = {
                let conn = state.db.lock().unwrap();
                store::plugins::get(&conn, &id)?
            };
            require_kind(&plugin, PluginKind::Import)?;
            interop::plugin::parse(&plugin.source, &content)
        }
        _ => Err(AppError::Other("preview_import: specify exactly one of format or plugin_id".into())),
    }
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
/// representation, or to a plugin's format — same mutual-exclusivity
/// convention as `preview_import`.
#[tauri::command]
pub fn export_collection(
    state: State<AppState>,
    collection_id: String,
    format: Option<ExportFormat>,
    plugin_id: Option<String>,
) -> AppResult<String> {
    let conn = state.db.lock().unwrap();
    let node = interop::collect(&conn, &collection_id)?;
    match (format, plugin_id) {
        (Some(f), None) => interop::export(f, &node),
        (None, Some(id)) => {
            let plugin = store::plugins::get(&conn, &id)?;
            require_kind(&plugin, PluginKind::Export)?;
            interop::plugin::export(&plugin.source, &node)
        }
        _ => Err(AppError::Other("export_collection: specify exactly one of format or plugin_id".into())),
    }
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
