//! File-based `.restman/` folder sync (Phase 8), scoped to what this app can
//! generically redact: collections (with auth already mask-on-write) and
//! environments (secret variables already mask-on-write). **History is
//! deliberately excluded** — a response body can contain arbitrary
//! server-echoed secrets (session cookies, API keys returned in a JSON
//! payload) that nothing in this codebase can generically detect and
//! redact, unlike collection/environment secrets, which are always confined
//! to known fields the mask-on-write contract already covers. History still
//! ships in full inside the (password-encrypted) ZIP backup — see
//! `crate::backup` — where "protected, not redacted" is the right trade-off
//! for a local-disaster-recovery artifact instead.
//!
//! One-directional by design: `export_to_folder` (DB -> files) is the only
//! thing ever triggered automatically (`SyncMode::Live`, driven from the
//! frontend after a mutation — see `commands::sync::sync_export`).
//! `import_from_folder` (files -> DB) is always an explicit user action,
//! never automatic — round-tripping external edits back into the DB without
//! a conflict-resolution engine (which this phase doesn't build) would risk
//! silently clobbering unrelated concurrent DB changes.
//!
//! Reuses the existing import/export IR instead of inventing a new format:
//! `interop::{ImportedNode, collect, apply_import}` for collections (already
//! `Serialize`/`Deserialize`, so this is literally that struct written to
//! disk) and `interop::environment::{export_environment, parse,
//! apply_environment_import}` for environments. The only new code here is
//! the folder layout, JSON<->YAML bridge, and file naming.

use crate::error::{AppError, AppResult};
use crate::interop::{self, ConflictMode, ImportedNode};
use crate::model::SyncFormat;
use rusqlite::Connection;
use saphyr::{LoadableYamlNode, Scalar, Yaml, YamlEmitter};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncExportReport {
    pub collections: usize,
    pub environments: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncImportReport {
    pub collections_imported: usize,
    pub environments_imported: usize,
    pub warnings: Vec<String>,
}

/// Filesystem-safe file stem: lowercase, non-alphanumerics collapsed to a
/// single `-`, trimmed of leading/trailing `-`. Falls back to `"untitled"`
/// so a pathological name (all-symbols) never yields an empty filename.
fn slugify(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut last_was_dash = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_was_dash = false;
        } else if !last_was_dash && !out.is_empty() {
            out.push('-');
            last_was_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() { "untitled".to_string() } else { out }
}

// ---------------------------------------------------------------------------
// JSON <-> YAML bridge. Mirrors `interop::openapi`'s `yaml_to_json` (parse
// direction); this module also needs the reverse (export), which OpenAPI
// import never did (its export always emits JSON). Kept local rather than
// shared: both copies are ~15 lines and the two modules' YAML needs
// (`interop::openapi` never writes YAML) don't overlap enough to be worth a
// shared abstraction over `saphyr`'s API.
// ---------------------------------------------------------------------------

fn yaml_to_json(y: &Yaml) -> Value {
    match y {
        Yaml::Value(Scalar::Null) => Value::Null,
        Yaml::Value(Scalar::Boolean(b)) => Value::Bool(*b),
        Yaml::Value(Scalar::Integer(i)) => Value::Number((*i).into()),
        Yaml::Value(Scalar::FloatingPoint(f)) => {
            serde_json::Number::from_f64(f.into_inner()).map(Value::Number).unwrap_or(Value::Null)
        }
        Yaml::Value(Scalar::String(s)) => Value::String(s.to_string()),
        Yaml::Sequence(seq) => Value::Array(seq.iter().map(yaml_to_json).collect()),
        Yaml::Mapping(map) => {
            let mut m = serde_json::Map::new();
            for (k, v) in map.iter() {
                if let Some(key) = k.as_str() {
                    m.insert(key.to_string(), yaml_to_json(v));
                }
            }
            Value::Object(m)
        }
        _ => Value::Null,
    }
}

fn json_to_yaml(v: &Value) -> Yaml<'static> {
    match v {
        Value::Null => Yaml::Value(Scalar::Null),
        Value::Bool(b) => Yaml::Value(Scalar::Boolean(*b)),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Yaml::Value(Scalar::Integer(i))
            } else {
                Yaml::Value(Scalar::FloatingPoint(n.as_f64().unwrap_or_default().into()))
            }
        }
        Value::String(s) => Yaml::Value(Scalar::String(s.clone().into())),
        Value::Array(a) => Yaml::Sequence(a.iter().map(json_to_yaml).collect()),
        Value::Object(o) => {
            let mut map = saphyr::Mapping::new();
            for (k, v) in o {
                map.insert(Yaml::Value(Scalar::String(k.clone().into())), json_to_yaml(v));
            }
            Yaml::Mapping(map)
        }
    }
}

fn to_yaml_string(v: &Value) -> AppResult<String> {
    let yaml = json_to_yaml(v);
    let mut out = String::new();
    YamlEmitter::new(&mut out).dump(&yaml).map_err(|e| AppError::Other(format!("YAML emit failed: {e}")))?;
    Ok(out)
}

fn from_yaml_str(content: &str) -> AppResult<Value> {
    let docs = Yaml::load_from_str(content).map_err(|e| AppError::Other(format!("invalid YAML: {e}")))?;
    let doc = docs.first().ok_or_else(|| AppError::Other("empty YAML document".into()))?;
    Ok(yaml_to_json(doc))
}

/// Serialize `value` to `format`'s text form.
fn serialize<T: Serialize>(value: &T, format: SyncFormat) -> AppResult<String> {
    let json = serde_json::to_value(value)?;
    match format {
        SyncFormat::Json => Ok(serde_json::to_string_pretty(&json)?),
        SyncFormat::Yaml => to_yaml_string(&json),
    }
}

/// Deserialize `content`, inferring JSON vs YAML from `ext` (case-
/// insensitive `json` / `yaml` / `yml`) rather than trusting the workspace's
/// current `sync_format` setting — a folder may carry files written under a
/// since-changed setting, or hand-edited ones, and both should still import.
fn deserialize<T: for<'de> Deserialize<'de>>(content: &str, ext: &str) -> AppResult<T> {
    let json = match ext.to_ascii_lowercase().as_str() {
        "yaml" | "yml" => from_yaml_str(content)?,
        _ => serde_json::from_str(content).map_err(|e| AppError::Other(format!("invalid JSON: {e}")))?,
    };
    serde_json::from_value(json).map_err(|e| AppError::Other(format!("unexpected shape: {e}")))
}

// ---------------------------------------------------------------------------
// Export (DB -> folder)
// ---------------------------------------------------------------------------

pub fn export_to_folder(
    conn: &Connection,
    workspace_id: &str,
    folder: &Path,
    format: SyncFormat,
) -> AppResult<SyncExportReport> {
    let mut report = SyncExportReport::default();
    let ext = format.extension();

    let collections_dir = folder.join("collections");
    std::fs::create_dir_all(&collections_dir)?;
    for top in crate::store::collections::list_children(conn, workspace_id, None)? {
        let node = interop::collect(conn, &top.id)?;
        let content = serialize(&node, format)?;
        std::fs::write(collections_dir.join(format!("{}.{ext}", slugify(&top.name))), content)?;
        report.collections += 1;
    }

    let environments_dir = folder.join("environments");
    std::fs::create_dir_all(&environments_dir)?;
    for env in crate::store::environments::list(conn, workspace_id)? {
        // `export_environment` already returns pretty-printed JSON text —
        // round-trip through `Value` when YAML is wanted rather than
        // duplicating its field-by-field construction here.
        let json_text = interop::environment::export_environment(conn, &env.id)?;
        let content = match format {
            SyncFormat::Json => json_text,
            SyncFormat::Yaml => {
                let v: Value = serde_json::from_str(&json_text)?;
                to_yaml_string(&v)?
            }
        };
        std::fs::write(environments_dir.join(format!("{}.{ext}", slugify(&env.name))), content)?;
        report.environments += 1;
    }

    let manifest = serde_json::json!({
        "restman_sync_version": 1,
        "workspace_id": workspace_id,
        "exported_at": crate::util::now_millis(),
    });
    std::fs::write(folder.join("manifest.json"), serde_json::to_string_pretty(&manifest)?)?;

    Ok(report)
}

// ---------------------------------------------------------------------------
// Import (folder -> DB), always explicit — see module doc.
// ---------------------------------------------------------------------------

fn read_dir_entries(dir: &Path) -> AppResult<Vec<(String, String)>> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_string();
        let content = std::fs::read_to_string(&path)?;
        out.push((ext, content));
    }
    Ok(out)
}

pub fn import_from_folder(
    conn: &Connection,
    workspace_id: &str,
    folder: &Path,
    mode: ConflictMode,
) -> AppResult<SyncImportReport> {
    let mut report = SyncImportReport::default();

    for (ext, content) in read_dir_entries(&folder.join("collections"))? {
        let node: ImportedNode = deserialize(&content, &ext)?;
        let sub_report = interop::apply_import(conn, workspace_id, None, &node, mode)?;
        report.collections_imported += 1;
        report.warnings.extend(sub_report.warnings);
    }

    for (ext, content) in read_dir_entries(&folder.join("environments"))? {
        // Environment files are always plain Postman-Environment-shaped
        // JSON on disk (`interop::environment::export_environment`'s native
        // output); YAML files just carry that same shape YAML-encoded, so
        // route through the JSON bridge first and reuse `parse` as-is
        // rather than duplicating its field extraction.
        let json_text = match ext.to_ascii_lowercase().as_str() {
            "yaml" | "yml" => {
                let v = from_yaml_str(&content)?;
                serde_json::to_string(&v)?
            }
            _ => content,
        };
        let preview = interop::environment::parse(&json_text)?;
        let sub_report = interop::environment::apply_environment_import(
            conn,
            workspace_id,
            None,
            &preview,
            mode != ConflictMode::Skip,
        )?;
        report.environments_imported += 1;
        report.warnings.extend(sub_report.warnings);
    }

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::auth::{AuthConfig, RequestAuth};

    fn mem() -> (Connection, String) {
        let mut conn = crate::store::db::open_in_memory().unwrap();
        let ws = crate::store::workspaces::ensure_default(&mut conn).unwrap();
        (conn, ws.id)
    }

    fn seed_collection(conn: &Connection, workspace_id: &str) -> String {
        let c = crate::store::collections::create(conn, workspace_id, None, "My Collection", None).unwrap();
        crate::store::collections::update_auth(conn, &c.id, AuthConfig::Bearer { token: "tok-real".into() }).unwrap();
        let input = crate::model::SavedRequestInput {
            name: "Get thing".into(),
            method: "GET".into(),
            url: "https://api.test/thing".into(),
            headers: vec![],
            query: vec![],
            body: Default::default(),
            options: Default::default(),
            auth: RequestAuth::Inherit,
            pre_request_script: String::new(),
            post_response_script: String::new(),
        };
        crate::store::requests::create(conn, &c.id, &input).unwrap();
        c.id
    }

    #[test]
    fn slugify_lowercases_and_collapses_symbols() {
        assert_eq!(slugify("My Cool Collection!"), "my-cool-collection");
        assert_eq!(slugify("  ***  "), "untitled");
        assert_eq!(slugify("api_v2/users"), "api-v2-users");
    }

    #[test]
    fn json_roundtrips_a_collection_tree_through_the_yaml_bridge() {
        let v = serde_json::json!({
            "name": "Root",
            "requests": [{"name": "R", "method": "GET", "url": "https://a.test"}],
            "children": [],
            "nested_null": null,
            "count": 3,
            "ratio": 1.5,
        });
        let yaml = to_yaml_string(&v).unwrap();
        let back = from_yaml_str(&yaml).unwrap();
        assert_eq!(back, v);
    }

    #[test]
    fn export_writes_one_file_per_top_level_collection_and_environment() {
        let (conn, ws) = mem();
        seed_collection(&conn, &ws);
        crate::store::environments::create(&conn, &ws, None, "Dev", None).unwrap();

        let dir = std::env::temp_dir().join(format!("restman_sync_test_{}", uuid::Uuid::new_v4()));
        let report = export_to_folder(&conn, &ws, &dir, SyncFormat::Json).unwrap();
        assert_eq!(report.collections, 1);
        assert_eq!(report.environments, 1);
        assert!(dir.join("collections/my-collection.json").exists());
        assert!(dir.join("environments/dev.json").exists());
        assert!(dir.join("manifest.json").exists());

        // Auth on disk must be masked, never the real bearer token.
        let text = std::fs::read_to_string(dir.join("collections/my-collection.json")).unwrap();
        assert!(!text.contains("tok-real"), "real secret leaked into synced file: {text}");
        assert!(text.contains(crate::model::variable::SECRET_MASK));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn export_then_import_round_trips_collection_and_environment_names() {
        let (conn, ws) = mem();
        seed_collection(&conn, &ws);
        crate::store::environments::create(&conn, &ws, None, "Dev", None).unwrap();

        let dir = std::env::temp_dir().join(format!("restman_sync_test_{}", uuid::Uuid::new_v4()));
        export_to_folder(&conn, &ws, &dir, SyncFormat::Yaml).unwrap();

        // Fresh workspace: importing must recreate both from the YAML files.
        let ws2 = crate::store::workspaces::create(&conn, "Target").unwrap();
        let report = import_from_folder(&conn, &ws2.id, &dir, ConflictMode::Skip).unwrap();
        assert_eq!(report.collections_imported, 1);
        assert_eq!(report.environments_imported, 1);

        let roots = crate::store::collections::list_children(&conn, &ws2.id, None).unwrap();
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].name, "My Collection");
        let envs = crate::store::environments::list(&conn, &ws2.id).unwrap();
        assert_eq!(envs.len(), 1);
        assert_eq!(envs[0].name, "Dev");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn import_from_missing_folder_is_a_no_op_not_an_error() {
        let (conn, ws) = mem();
        let dir = std::env::temp_dir().join(format!("restman_sync_test_missing_{}", uuid::Uuid::new_v4()));
        let report = import_from_folder(&conn, &ws, &dir, ConflictMode::Skip).unwrap();
        assert_eq!(report.collections_imported, 0);
        assert_eq!(report.environments_imported, 0);
    }
}
