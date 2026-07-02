//! IPC wrappers over `crate::backup`. Bytes cross the boundary base64-
//! encoded, same convention as `commands::files::write_file_bytes` — the
//! frontend base64-decodes/-encodes on its side (`textToBase64`/
//! `base64ToBytes` in `src/lib/encoding.ts`) rather than this module adding
//! a second `AppState`-free path for raw bytes.

use crate::backup::{self, RestoreReport};
use crate::error::{AppError, AppResult};
use crate::store::AppState;
use base64::Engine;
use tauri::State;

fn decode_base64(s: &str) -> AppResult<Vec<u8>> {
    base64::engine::general_purpose::STANDARD
        .decode(s)
        .map_err(|e| AppError::Other(format!("invalid base64: {e}")))
}

/// Returns the ZIP archive base64-encoded — the frontend writes it to disk
/// via a native save dialog + `write_file_bytes`, same pattern every other
/// export in this app already uses.
#[tauri::command]
pub fn create_backup(state: State<AppState>, password: String) -> AppResult<String> {
    let conn = state.db.lock().unwrap();
    let bytes = backup::create_backup(&conn, &password)?;
    Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
}

/// `content_base64` is read client-side from an uploaded `.zip` file (same
/// browser-`FileReader` convention the text-based importers already use for
/// user-supplied files) rather than a filesystem path, so this command needs
/// no separate "read arbitrary path" capability.
#[tauri::command]
pub fn restore_backup(state: State<AppState>, content_base64: String, password: String) -> AppResult<RestoreReport> {
    let bytes = decode_base64(&content_base64)?;
    let mut conn = state.db.lock().unwrap();
    backup::restore_backup(&mut conn, &bytes, &password)
}
