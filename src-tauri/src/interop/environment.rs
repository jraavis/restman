//! Environment import/export. Postman's "Environment" JSON is the de-facto
//! interchange format for variable bundles across REST clients:
//!
//! ```json
//! {
//!   "name": "Production",
//!   "values": [
//!     { "key": "baseUrl", "value": "https://api.prod", "enabled": true, "type": "default" },
//!     { "key": "token",   "value": "*****",          "enabled": true, "type": "secret" }
//!   ]
//! }
//! ```
//!
//! Insomnia uses the same shape (its "Environment Export" emits a `data`
//! array of these). Re-importing a file this app exported must not fabricate a
//! secret from `SECRET_MASK` — see `strip_unrecoverable_secret` and the
//! interop module doc for the rationale (a freshly-created environment has
//! no keychain entry to recover the mask from).
//!
//! Unlike collection import, environments don't carry a tree of nested
//! folders — they're a flat key/value list scoped to a single environment
//! row. So the preview/apply flow is simpler: `{ name, variables }` in,
//! `{ name, variables }` out, with conflicts handled by overwriting same-key
//! variables (the common case) rather than skip/merge nesting.

use crate::error::{AppError, AppResult};
use crate::model::{VarScope, VarType, VariableInput};
use crate::store;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

const POSTMAN_MASK: &str = "*****";

/// One importable/exportable variable, format-agnostic.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedVariable {
    pub key: String,
    pub value: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub is_secret: bool,
}

fn default_true() -> bool {
    true
}

/// Preview (no DB writes): `{ name, variables }`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentPreview {
    pub name: String,
    pub variables: Vec<ImportedVariable>,
    pub warnings: Vec<String>,
}

/// Per-variable apply report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentImportReport {
    pub created_variables: usize,
    pub overwritten: usize,
    pub warnings: Vec<String>,
}

/// Parse a Postman/Insomnia environment JSON blob.
pub fn parse(content: &str) -> AppResult<EnvironmentPreview> {
    let mut v: serde_json::Value = serde_json::from_str(content)
        .map_err(|e| AppError::Other(format!("invalid environment JSON: {e}")))?;
    let mut warnings = Vec::new();
    let name = v.get("name").and_then(serde_json::Value::as_str).unwrap_or("Imported environment").to_string();

    let values = match v.get_mut("values").and_then(serde_json::Value::as_array_mut) {
        Some(a) => std::mem::take(a),
        None => {
            return Err(AppError::Other("not a Postman environment: missing \"values\" array".into()));
        }
    };

    let mut variables = Vec::new();
    let mut seen_keys = std::collections::HashSet::new();
    for entry in &values {
        let key = entry.get("key").and_then(serde_json::Value::as_str).unwrap_or_default().to_string();
        if key.is_empty() {
            warnings.push("environment variable with an empty key — skipped".into());
            continue;
        }
        if !seen_keys.insert(key.clone()) {
            warnings.push(format!("duplicate variable key \"{key}\" — only the first occurrence is kept"));
            continue;
        }
        let value = entry.get("value").and_then(serde_json::Value::as_str).unwrap_or_default().to_string();
        let enabled = !entry.get("disabled").and_then(serde_json::Value::as_bool).unwrap_or(false);
        let ty = entry.get("type").and_then(serde_json::Value::as_str).unwrap_or("default");
        let is_secret = ty == "secret";
        // Re-importing this app's own export carries the mask instead of a
        // real secret. Flag it here; `apply_environment_import` zeroes it so
        // the user gets an honest "secret not set" state rather than a
        // misleading masked-but-unreadable value.
        let mut value = value;
        if is_secret && value == POSTMAN_MASK {
            value = String::new();
            warnings.push(format!("variable \"{key}\" was already masked in the imported file and could not be recovered — re-enter it"));
        }
        variables.push(ImportedVariable { key, value, enabled, is_secret });
    }

    Ok(EnvironmentPreview { name, variables, warnings })
}

/// Commit a previewed environment under `workspace_id`, optionally narrowed to
/// `collection_id`. Same-key variables are overwritten; the environment
/// itself is created fresh each call (no name conflict at the environment
/// level — re-importing the same env twice yields two environments, mirroring
/// how collection import would behave without a parent collision).
pub fn apply_environment_import(
    conn: &Connection,
    workspace_id: &str,
    collection_id: Option<&str>,
    preview: &EnvironmentPreview,
    overwrite_existing: bool,
) -> AppResult<EnvironmentImportReport> {
    let env = store::environments::create(conn, workspace_id, collection_id, &preview.name, None)?;
    let mut report = EnvironmentImportReport { created_variables: 0, overwritten: 0, warnings: preview.warnings.clone() };
    let scope = VarScope::Environment(env.id);

    for v in &preview.variables {
        let input = VariableInput {
            key: v.key.clone(),
            value: v.value.clone(),
            var_type: VarType::String,
            is_secret: v.is_secret,
            enabled: v.enabled,
        };
        // Overwrite-by-key (when requested) reuses an existing variable of the
        // same key within the *new* environment — but a freshly-created
        // environment has no prior variables, so there's nothing to overwrite
        // here on a first import. The flag exists for the re-import / sync
        // case where the caller wants to merge into an existing environment,
        // which this function doesn't take (it always mints a new env). For now
        // this branch is effectively a no-op but kept for parity with the
        // collection import's ConflictMode::Overwrite semantics.
        if overwrite_existing {
            let existing = store::variables::list(conn, &scope)?
                .into_iter()
                .find(|x| x.key == v.key);
            if let Some(x) = existing {
                store::variables::update(conn, &x.id, &input)?;
                report.overwritten += 1;
                continue;
            }
        }
        store::variables::create(conn, &scope, &input)?;
        report.created_variables += 1;
    }

    Ok(report)
}

/// Read an environment (and its variables) out as an `EnvironmentPreview`-
/// shaped JSON string. Secret values are masked (`POSTMAN_MASK`) — see module
/// doc: the real secret lives in the keychain, never in the DB column, so
/// export must emit a placeholder, not the cleartext.
pub fn export_environment(conn: &Connection, environment_id: &str) -> AppResult<String> {
    let env = store::environments::get(conn, environment_id)?;
    let vars = store::variables::list(conn, &VarScope::Environment(environment_id.to_string()))?;
    let values: Vec<serde_json::Value> = vars
        .into_iter()
        .map(|v| {
            let value = if v.is_secret { POSTMAN_MASK.to_string() } else { v.value };
            let ty = if v.is_secret { "secret" } else { "default" };
            serde_json::json!({
                "key": v.key,
                "value": value,
                "enabled": v.enabled,
                "type": ty,
            })
        })
        .collect();
    let doc = serde_json::json!({
        "name": env.name,
        "_postman_variable_scope": "environment",
        "values": values,
    });
    Ok(serde_json::to_string_pretty(&doc)?)
}

// ---------------------------------------------------------------------------
// IPC command wrappers live in `commands::interop` (re-exported flat through
// the `commands::*` namespace `lib.rs` builds the handler list from). This
// module stays pure: no `AppState`, no `tauri::State`, so the unit tests can
// exercise it against an in-memory SQLite DB without a Tauri runtime.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const ENV_FIXTURE: &str = r#"{
        "name": "Production",
        "values": [
            {"key": "baseUrl", "value": "https://api.prod", "enabled": true, "type": "default"},
            {"key": "token", "value": "tok_abc123", "enabled": true, "type": "secret"},
            {"key": "debug", "value": "1", "disabled": true, "type": "default"}
        ]
    }"#;

    #[test]
    fn parses_postman_environment_with_expected_shape() {
        let preview = parse(ENV_FIXTURE).unwrap();
        assert_eq!(preview.name, "Production");
        assert_eq!(preview.variables.len(), 3);
        assert_eq!(preview.variables[0].key, "baseUrl");
        assert!(!preview.variables[0].is_secret);
        assert!(preview.variables[0].enabled);
        assert!(preview.variables[1].is_secret);
        assert!(preview.variables[2] == ImportedVariable { key: "debug".into(), value: "1".into(), enabled: false, is_secret: false });
        assert!(preview.warnings.is_empty(), "{:?}", preview.warnings);
    }

    #[test]
    fn duplicate_keys_are_warned_and_deduped() {
        let json = r#"{
            "name": "X",
            "values": [
                {"key": "a", "value": "1"},
                {"key": "a", "value": "2"}
            ]
        }"#;
        let preview = parse(json).unwrap();
        assert_eq!(preview.variables.len(), 1);
        assert_eq!(preview.variables[0].value, "1");
        assert!(preview.warnings.iter().any(|w| w.contains("duplicate")), "{:?}", preview.warnings);
    }

    #[test]
    fn masked_secret_is_cleared_and_warned_on_reimport() {
        let json = r#"{
            "name": "X",
            "values": [{"key": "token", "value": "*****", "type": "secret"}]
        }"#;
        let preview = parse(json).unwrap();
        assert_eq!(preview.variables[0].value, "");
        assert!(preview.warnings.iter().any(|w| w.contains("re-enter")), "{:?}", preview.warnings);
    }

    #[test]
    fn apply_creates_environment_and_variables_with_secret_in_keychain() {
        let mut conn = crate::store::db::open_in_memory().unwrap();
        let ws = crate::store::workspaces::ensure_default(&mut conn).unwrap();

        let preview = parse(ENV_FIXTURE).unwrap();
        let report = apply_environment_import(&conn, &ws.id, None, &preview, false).unwrap();
        assert_eq!(report.created_variables, 3);

        let envs = store::environments::list(&conn, &ws.id).unwrap();
        assert_eq!(envs.len(), 1);
        assert_eq!(envs[0].name, "Production");
        let vars = store::variables::list(&conn, &VarScope::Environment(envs[0].id.clone())).unwrap();
        assert_eq!(vars.len(), 3);
        let token = vars.iter().find(|v| v.key == "token").unwrap();
        assert!(token.is_secret);
        assert_eq!(token.value, "", "secret value must not be stored in the DB column");
        let real = crate::secrets::get(&store::variables::keychain_key(&token.id)).unwrap().unwrap();
        assert_eq!(real, "tok_abc123");
    }

    #[test]
    fn export_masks_secret_values() {
        let mut conn = crate::store::db::open_in_memory().unwrap();
        let ws = crate::store::workspaces::ensure_default(&mut conn).unwrap();
        let preview = parse(ENV_FIXTURE).unwrap();
        let _report = apply_environment_import(&conn, &ws.id, None, &preview, false).unwrap();

        let envs = store::environments::list(&conn, &ws.id).unwrap();
        let json = export_environment(&conn, &envs[0].id).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["name"], "Production");
        let token = v["values"].as_array().unwrap().iter().find(|e| e["key"] == "token").unwrap();
        assert_eq!(token["value"], POSTMAN_MASK);
        assert_eq!(token["type"], "secret");

        // Round-trip the export back through parse — the masked secret should
        // come out as an empty value with a warning (fresh env, can't recover).
        let reparsed = parse(&json).unwrap();
        let tok = reparsed.variables.iter().find(|x| x.key == "token").unwrap();
        assert_eq!(tok.value, "");
        assert!(reparsed.warnings.iter().any(|w| w.contains("re-enter")), "{:?}", reparsed.warnings);
    }

    #[test]
    fn rejects_non_environment_json() {
        let err = parse(r#"{"foo": "bar"}"#).unwrap_err();
        assert!(err.to_string().contains("values"));
    }
}
