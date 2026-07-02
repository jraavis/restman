//! Thin IPC wrappers over `crate::sync`. Both commands read
//! `sync_folder_path`/`sync_format` from the workspace's settings row rather
//! than taking them as arguments — the frontend calls `sync_export` with
//! nothing but a workspace id for both the manual "Sync now" button and the
//! `SyncMode::Live` auto-trigger after a mutation, so there's exactly one
//! source of truth for "where does this workspace sync to."

use crate::error::{AppError, AppResult};
use crate::interop::ConflictMode;
use crate::model::{SyncFormat, SyncMode};
use crate::store::{workspace_settings, AppState};
use crate::sync::{self, SyncExportReport, SyncImportReport};
use rusqlite::Connection;
use std::path::PathBuf;
use tauri::State;

fn require_sync_folder(conn: &Connection, workspace_id: &str) -> AppResult<(PathBuf, SyncFormat)> {
    let settings = workspace_settings::get(conn, workspace_id)?;
    match (settings.sync_mode, settings.sync_folder_path) {
        (SyncMode::Off, _) | (_, None) => {
            Err(AppError::Other("sync is not configured for this workspace — set a sync folder in workspace settings first".into()))
        }
        (_, Some(path)) => Ok((PathBuf::from(path), settings.sync_format)),
    }
}

/// DB -> folder. Safe to call whenever `SyncMode` is `Manual` or `Live`
/// (`require_sync_folder` above rejects `Off`) — the frontend uses this both
/// for an explicit "Sync now" button and as the automatic post-mutation
/// trigger in `Live` mode.
#[tauri::command]
pub fn sync_export(state: State<AppState>, workspace_id: String) -> AppResult<SyncExportReport> {
    let conn = state.db.lock().unwrap();
    let (folder, format) = require_sync_folder(&conn, &workspace_id)?;
    sync::export_to_folder(&conn, &workspace_id, &folder, format)
}

/// Folder -> DB. Always an explicit user action (never auto-triggered) —
/// see `crate::sync` module doc for why.
#[tauri::command]
pub fn sync_import(state: State<AppState>, workspace_id: String, mode: ConflictMode) -> AppResult<SyncImportReport> {
    let conn = state.db.lock().unwrap();
    let (folder, _format) = require_sync_folder(&conn, &workspace_id)?;
    sync::import_from_folder(&conn, &workspace_id, &folder, mode)
}
