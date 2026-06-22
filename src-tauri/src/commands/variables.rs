use crate::error::AppResult;
use crate::model::{VarScope, Variable, VariableInput, SECRET_MASK};
use crate::secrets::{self, SecretBackendStatus};
use crate::store::{variables, AppState};
use tauri::State;

// Secret values never cross the IPC boundary in plaintext — interpolation
// happens entirely in Rust (`crate::vars::resolve` reads the DB directly),
// so the frontend only ever needs to know a secret is set, not what it is.

#[tauri::command]
pub fn list_variables(state: State<AppState>, scope: VarScope) -> AppResult<Vec<Variable>> {
    let conn = state.db.lock().unwrap();
    Ok(variables::list(&conn, &scope)?.into_iter().map(Variable::mask_secret).collect())
}

#[tauri::command]
pub fn create_variable(state: State<AppState>, scope: VarScope, input: VariableInput) -> AppResult<Variable> {
    let conn = state.db.lock().unwrap();
    Ok(variables::create(&conn, &scope, &input)?.mask_secret())
}

#[tauri::command]
pub fn update_variable(state: State<AppState>, id: String, mut input: VariableInput) -> AppResult<Variable> {
    let conn = state.db.lock().unwrap();
    // The editor round-trips the mask when the user didn't touch a secret
    // field; that must not overwrite the real stored value with literal dots.
    // The real value lives in the keychain, not the (always-empty) DB column,
    // so it has to be recovered from there, not from `variables::get`.
    if input.is_secret && input.value == SECRET_MASK {
        input.value = secrets::get(&format!("var:{id}"))?.unwrap_or_default();
    }
    Ok(variables::update(&conn, &id, &input)?.mask_secret())
}

#[tauri::command]
pub fn delete_variable(state: State<AppState>, id: String) -> AppResult<()> {
    let conn = state.db.lock().unwrap();
    variables::delete(&conn, &id)
}

#[tauri::command]
pub fn get_secret_backend_status() -> SecretBackendStatus {
    secrets::backend_status()
}
