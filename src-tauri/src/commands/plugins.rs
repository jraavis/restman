//! Plugin CRUD + "run before saving" preview commands. Thin wrappers over
//! `store::plugins` (persistence) and the per-domain plugin-dispatch glue in
//! `codegen::plugin`/`interop::plugin` (execution) — mirrors every other
//! domain's command-module shape in this codebase.

use crate::codegen::CodegenOptions;
use crate::error::{AppError, AppResult};
use crate::interop::{ImportPreview, ImportedNode};
use crate::model::http::HttpRequest;
use crate::model::{Plugin, PluginInput, PluginKind};
use crate::store::{self, AppState};
use crate::{codegen, interop};
use tauri::State;

#[tauri::command]
pub fn list_plugins(state: State<'_, AppState>, workspace_id: String, kind: Option<PluginKind>) -> AppResult<Vec<Plugin>> {
    let conn = state.db.lock().unwrap();
    store::plugins::list_by_workspace(&conn, &workspace_id, kind)
}

#[tauri::command]
pub fn create_plugin(state: State<'_, AppState>, workspace_id: String, input: PluginInput) -> AppResult<Plugin> {
    let conn = state.db.lock().unwrap();
    store::plugins::create(&conn, &workspace_id, &input)
}

#[tauri::command]
pub fn update_plugin(state: State<'_, AppState>, id: String, input: PluginInput) -> AppResult<Plugin> {
    let conn = state.db.lock().unwrap();
    store::plugins::update(&conn, &id, &input)
}

#[tauri::command]
pub fn delete_plugin(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let conn = state.db.lock().unwrap();
    store::plugins::delete(&conn, &id)
}

/// Serializes a plugin's config (name/kind/label/source/enabled) to a JSON
/// string for saving to disk — same shareable-backup convention as
/// `export_mock_server`. No secrets live in a plugin's JS source, so unlike
/// environment export this needs no masking.
#[tauri::command]
pub fn export_plugin(state: State<'_, AppState>, id: String) -> AppResult<String> {
    let conn = state.db.lock().unwrap();
    store::plugins::export(&conn, &id)
}

/// Creates a new plugin in `workspace_id` from a previously exported JSON
/// string.
#[tauri::command]
pub fn import_plugin(state: State<'_, AppState>, workspace_id: String, content: String) -> AppResult<Plugin> {
    let conn = state.db.lock().unwrap();
    store::plugins::import(&conn, &workspace_id, &content)
}

/// Run a codegen plugin's `source` against `req` without persisting it —
/// lets the plugin editor show live output while the user is still writing
/// the plugin.
#[tauri::command]
pub fn preview_plugin_codegen(source: String, req: HttpRequest, options: CodegenOptions) -> AppResult<String> {
    codegen::plugin::generate(&source, &req, &options)
}

/// Run an import plugin's `source` against raw `content` without
/// persisting it.
#[tauri::command]
pub fn preview_plugin_import(source: String, content: String) -> AppResult<ImportPreview> {
    interop::plugin::parse(&source, &content)
}

/// Run an export plugin's `source` against an in-memory `node` without
/// persisting it.
#[tauri::command]
pub fn preview_plugin_export(source: String, node: ImportedNode) -> AppResult<String> {
    interop::plugin::export(&source, &node)
}

pub(crate) fn require_kind(plugin: &Plugin, expected: PluginKind) -> AppResult<()> {
    if plugin.kind != expected {
        return Err(AppError::Other(format!(
            "plugin {} is a {:?} plugin, not a {:?} plugin",
            plugin.id, plugin.kind, expected
        )));
    }
    Ok(())
}
