//! Restman-native full export/import (`.restman.json`): whole selected
//! workspaces — collections (nested folders + requests incl. pre/post
//! scripts), environments, and workspace/global variables — in one JSON
//! document. Sits between the per-collection interop formats (lossy,
//! single-tree) and the encrypted full-app backup (exact-ID, all-or-nothing):
//! this format is selective, name-addressed, and shareable.
//!
//! Secrets: export is mask-on-write by default like every other export in
//! this app, but the caller can opt into `include_secrets`, which hydrates
//! real values from the keychain into the plaintext JSON (the UI warns).
//! Import routes real secret values back through the existing keychain-aware
//! create paths; a value still equal to `SECRET_MASK` is unrecoverable in a
//! fresh install and is cleared with a warning, same contract as
//! `strip_unrecoverable_masks`.
//!
//! IDs are never exported. Import matches workspaces/environments by name
//! and variables by key, minting new UUIDs through the store layer — the
//! same conflict semantics `apply_import` already uses for collections.

use crate::error::{AppError, AppResult};
use crate::interop::{self, ConflictMode, ImportedNode};
use crate::model::http::HeaderEntry;
use crate::model::variable::{SECRET_MASK, VarType};
use crate::model::{VarScope, VariableInput, WorkspaceSettings};
use crate::store;
use crate::util::now_millis;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

/// Bump when the schema changes shape incompatibly. Import refuses files
/// newer than this rather than silently dropping fields it can't parse.
pub const EXPORT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestmanExport {
    pub restman_export_version: u32,
    pub created_at: i64,
    pub app_version: String,
    pub includes_secrets: bool,
    #[serde(default)]
    pub workspaces: Vec<ExportedWorkspace>,
    #[serde(default)]
    pub global_variables: Vec<ExportedVariable>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportedWorkspace {
    pub name: String,
    #[serde(default)]
    pub settings: Option<ExportedWorkspaceSettings>,
    #[serde(default)]
    pub collections: Vec<ImportedNode>,
    #[serde(default)]
    pub environments: Vec<ExportedEnvironment>,
    #[serde(default)]
    pub workspace_variables: Vec<ExportedVariable>,
}

/// Only the portable subset of `WorkspaceSettings` — client certs live in
/// the keychain/filesystem and sync folder paths are machine-specific, so
/// neither survives a move to another machine meaningfully.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportedWorkspaceSettings {
    #[serde(default)]
    pub proxy_url: Option<String>,
    #[serde(default)]
    pub proxy_bypass: Option<String>,
    #[serde(default)]
    pub default_headers: Vec<HeaderEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportedEnvironment {
    pub name: String,
    #[serde(default)]
    pub group_name: Option<String>,
    #[serde(default)]
    pub variables: Vec<ExportedVariable>,
}

/// Like `environment::ImportedVariable` but carrying `var_type`, which the
/// Postman-shaped interchange format has no slot for and a native format
/// shouldn't lose.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportedVariable {
    pub key: String,
    pub value: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub is_secret: bool,
    #[serde(default = "default_var_type")]
    pub var_type: VarType,
}

fn default_true() -> bool {
    true
}

fn default_var_type() -> VarType {
    VarType::String
}

// ---------------------------------------------------------------------------
// Export
// ---------------------------------------------------------------------------

pub fn export_full(
    conn: &Connection,
    workspace_ids: &[String],
    include_secrets: bool,
    include_settings: bool,
) -> AppResult<String> {
    let mut workspaces = Vec::new();
    for ws in store::workspaces::list(conn)?.into_iter().filter(|w| workspace_ids.contains(&w.id)) {
        let collections = store::collections::list_children(conn, &ws.id, None)?
            .iter()
            .map(|c| interop::collect_with_secrets(conn, &c.id, include_secrets))
            .collect::<AppResult<Vec<_>>>()?;

        let environments = store::environments::list(conn, &ws.id)?
            .into_iter()
            .map(|e| {
                let variables =
                    export_variables(conn, &VarScope::Environment(e.id.clone()), include_secrets)?;
                Ok(ExportedEnvironment { name: e.name, group_name: e.group_name, variables })
            })
            .collect::<AppResult<Vec<_>>>()?;

        let workspace_variables =
            export_variables(conn, &VarScope::Workspace(ws.id.clone()), include_secrets)?;

        let settings = if include_settings {
            let s = store::workspace_settings::get(conn, &ws.id)?;
            Some(ExportedWorkspaceSettings {
                proxy_url: s.proxy_url,
                proxy_bypass: s.proxy_bypass,
                default_headers: s.default_headers,
            })
        } else {
            None
        };

        workspaces.push(ExportedWorkspace {
            name: ws.name,
            settings,
            collections,
            environments,
            workspace_variables,
        });
    }

    let doc = RestmanExport {
        restman_export_version: EXPORT_VERSION,
        created_at: now_millis(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        includes_secrets: include_secrets,
        workspaces,
        global_variables: export_variables(conn, &VarScope::Global, include_secrets)?,
    };
    Ok(serde_json::to_string_pretty(&doc)?)
}

/// Secret variables store `""` in the value column with the real value in
/// the keychain — so unlike auth (mask-on-write in `auth_json`), the masked
/// representation has to be minted here at export time.
fn export_variables(
    conn: &Connection,
    scope: &VarScope,
    include_secrets: bool,
) -> AppResult<Vec<ExportedVariable>> {
    store::variables::list(conn, scope)?
        .into_iter()
        .map(|v| {
            let value = if v.is_secret {
                if include_secrets {
                    crate::secrets::get(&store::variables::keychain_key(&v.id))?.unwrap_or_default()
                } else {
                    SECRET_MASK.to_string()
                }
            } else {
                v.value
            };
            Ok(ExportedVariable {
                key: v.key,
                value,
                enabled: v.enabled,
                is_secret: v.is_secret,
                var_type: v.var_type,
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Import: preview
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspacePreview {
    pub name: String,
    /// A same-name workspace already exists — import will merge into it.
    pub exists: bool,
    pub collections: usize,
    pub requests: usize,
    pub environments: usize,
    pub variables: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FullImportPreview {
    pub version: u32,
    pub app_version: String,
    pub includes_secrets: bool,
    pub workspaces: Vec<WorkspacePreview>,
    pub global_variables: usize,
    /// Secret values carried as `SECRET_MASK` — unrecoverable on import.
    pub masked_secrets: usize,
    pub warnings: Vec<String>,
}

/// Parse + version-check. Reads the version out of the raw JSON before the
/// typed parse so a newer file fails with "update the app", not a cryptic
/// serde error about a field this version doesn't know.
fn parse_export(content: &str) -> AppResult<RestmanExport> {
    let v: serde_json::Value = serde_json::from_str(content)
        .map_err(|e| AppError::Other(format!("invalid restman export JSON: {e}")))?;
    let version = v
        .get("restmanExportVersion")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| {
            AppError::Other("not a restman export: missing \"restmanExportVersion\"".into())
        })?;
    if version as u32 > EXPORT_VERSION {
        return Err(AppError::Other(format!(
            "this file uses restman export format v{version}, but this app supports up to v{EXPORT_VERSION} — update restman to import it"
        )));
    }
    serde_json::from_value(v).map_err(|e| AppError::Other(format!("invalid restman export: {e}")))
}

pub fn preview_full(conn: &Connection, content: &str) -> AppResult<FullImportPreview> {
    let doc = parse_export(content)?;
    let existing_names: Vec<String> =
        store::workspaces::list(conn)?.into_iter().map(|w| w.name).collect();

    let mut masked_secrets = count_masked_variables(&doc.global_variables);
    let workspaces = doc
        .workspaces
        .iter()
        .map(|ws| {
            let (folders, requests) = ws
                .collections
                .iter()
                .fold((0, 0), |(f, r), node| {
                    let (nf, nr) = count_nodes(node);
                    (f + nf, r + nr)
                });
            let variables = ws.workspace_variables.len()
                + ws.environments.iter().map(|e| e.variables.len()).sum::<usize>();
            masked_secrets += count_masked_variables(&ws.workspace_variables)
                + ws.environments.iter().map(|e| count_masked_variables(&e.variables)).sum::<usize>()
                + ws.collections.iter().map(count_masked_auth).sum::<usize>();
            WorkspacePreview {
                name: ws.name.clone(),
                exists: existing_names.iter().any(|n| n == &ws.name),
                collections: folders,
                requests,
                environments: ws.environments.len(),
                variables,
            }
        })
        .collect();

    let mut warnings = Vec::new();
    if masked_secrets > 0 {
        warnings.push(format!(
            "{masked_secrets} secret(s) in this file are masked and cannot be recovered — re-enter them after import"
        ));
    }

    Ok(FullImportPreview {
        version: doc.restman_export_version,
        app_version: doc.app_version,
        includes_secrets: doc.includes_secrets,
        workspaces,
        global_variables: doc.global_variables.len(),
        masked_secrets,
        warnings,
    })
}

fn count_nodes(node: &ImportedNode) -> (usize, usize) {
    node.children.iter().fold((1, node.requests.len()), |(f, r), child| {
        let (cf, cr) = count_nodes(child);
        (f + cf, r + cr)
    })
}

fn count_masked_variables(vars: &[ExportedVariable]) -> usize {
    vars.iter().filter(|v| v.is_secret && v.value == SECRET_MASK).count()
}

fn count_masked_auth(node: &ImportedNode) -> usize {
    let own = node
        .auth
        .secret_fields()
        .into_iter()
        .filter(|(_, v)| *v == SECRET_MASK)
        .count();
    let requests: usize = node
        .requests
        .iter()
        .map(|r| match &r.auth {
            crate::model::auth::RequestAuth::Inherit => 0,
            crate::model::auth::RequestAuth::Own(cfg) => {
                cfg.secret_fields().into_iter().filter(|(_, v)| *v == SECRET_MASK).count()
            }
        })
        .sum();
    own + requests + node.children.iter().map(count_masked_auth).sum::<usize>()
}

// ---------------------------------------------------------------------------
// Import: apply
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FullImportReport {
    pub workspaces_created: usize,
    pub created_collections: usize,
    pub created_requests: usize,
    pub skipped: usize,
    pub overwritten: usize,
    pub environments_created: usize,
    pub variables_created: usize,
    pub variables_overwritten: usize,
    pub variables_skipped: usize,
    pub warnings: Vec<String>,
}

pub fn apply_full(conn: &Connection, content: &str, mode: ConflictMode) -> AppResult<FullImportReport> {
    let doc = parse_export(content)?;
    let mut report = FullImportReport::default();

    for ws in &doc.workspaces {
        let existing = store::workspaces::list(conn)?.into_iter().find(|w| w.name == ws.name);
        let (ws_id, created) = match existing {
            Some(w) => (w.id, false),
            None => {
                let w = store::workspaces::create(conn, &ws.name)?;
                report.workspaces_created += 1;
                (w.id, true)
            }
        };

        // Portable settings only land on a freshly created workspace — an
        // existing workspace's proxy/header config shouldn't be clobbered by
        // an import that's primarily about collections.
        if created {
            if let Some(s) = &ws.settings {
                store::workspace_settings::set(
                    conn,
                    &WorkspaceSettings {
                        proxy_url: s.proxy_url.clone(),
                        proxy_bypass: s.proxy_bypass.clone(),
                        default_headers: s.default_headers.clone(),
                        ..WorkspaceSettings::empty(&ws_id)
                    },
                )?;
            }
        }

        for node in &ws.collections {
            let r = interop::apply_import(conn, &ws_id, None, node, mode, interop::ImportPlacement::AsSubfolder)?;
            report.created_collections += r.created_collections;
            report.created_requests += r.created_requests;
            report.skipped += r.skipped;
            report.overwritten += r.overwritten;
            report.warnings.extend(r.warnings);
        }

        for env in &ws.environments {
            apply_environment(conn, &ws_id, env, mode, &mut report)?;
        }

        apply_variables(conn, &VarScope::Workspace(ws_id.clone()), &ws.workspace_variables, mode, &mut report)?;
    }

    apply_variables(conn, &VarScope::Global, &doc.global_variables, mode, &mut report)?;
    Ok(report)
}

/// Environments match by name within the workspace: missing → created with
/// all its variables; existing → variables merged per `mode` (Skip leaves
/// the whole environment untouched).
fn apply_environment(
    conn: &Connection,
    workspace_id: &str,
    env: &ExportedEnvironment,
    mode: ConflictMode,
    report: &mut FullImportReport,
) -> AppResult<()> {
    let existing = store::environments::list(conn, workspace_id)?
        .into_iter()
        .find(|e| e.name == env.name);
    let env_id = match existing {
        Some(e) => {
            if mode == ConflictMode::Skip {
                report.variables_skipped += env.variables.len();
                return Ok(());
            }
            e.id
        }
        None => {
            let created = store::environments::create(
                conn,
                workspace_id,
                None,
                &env.name,
                env.group_name.as_deref(),
            )?;
            report.environments_created += 1;
            created.id
        }
    };
    apply_variables(conn, &VarScope::Environment(env_id), &env.variables, mode, report)
}

/// Variables match by key within their scope. A secret still carrying
/// `SECRET_MASK` can't be recovered: on create it lands empty with a
/// warning; on overwrite the existing (real) value is deliberately kept.
fn apply_variables(
    conn: &Connection,
    scope: &VarScope,
    vars: &[ExportedVariable],
    mode: ConflictMode,
    report: &mut FullImportReport,
) -> AppResult<()> {
    let existing = store::variables::list(conn, scope)?;
    for v in vars {
        let masked = v.is_secret && v.value == SECRET_MASK;
        let input = VariableInput {
            key: v.key.clone(),
            value: if masked { String::new() } else { v.value.clone() },
            var_type: v.var_type,
            is_secret: v.is_secret,
            enabled: v.enabled,
        };
        match existing.iter().find(|x| x.key == v.key) {
            None => {
                store::variables::create(conn, scope, &input)?;
                report.variables_created += 1;
                if masked {
                    report.warnings.push(format!(
                        "variable \"{}\": secret was already masked in the imported file and could not be recovered — re-enter it",
                        v.key
                    ));
                }
            }
            Some(x) => match mode {
                // Masked value on overwrite: the existing variable's real
                // secret beats an unrecoverable placeholder.
                ConflictMode::Overwrite if !masked => {
                    store::variables::update(conn, &x.id, &input)?;
                    report.variables_overwritten += 1;
                }
                _ => report.variables_skipped += 1,
            },
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interop::ImportedRequest;
    use crate::model::auth::{AuthConfig, RequestAuth};

    fn seeded_conn() -> (Connection, String) {
        let mut conn = crate::store::db::open_in_memory().unwrap();
        let ws = crate::store::workspaces::ensure_default(&mut conn).unwrap();
        (conn, ws.id)
    }

    /// Regression guard for the exact user-visible bug: a request body
    /// (every non-None mode) must survive export_full → apply_full into a
    /// fresh DB — the Body tab showed "None" after a restman import because
    /// the body was lost somewhere on this path.
    #[test]
    fn request_bodies_of_every_mode_survive_a_restman_round_trip() {
        use crate::model::http::{FormField, KeyValue, RequestBody};

        let (conn, ws_id) = seeded_conn();
        let bodies: Vec<(&str, RequestBody)> = vec![
            ("json", RequestBody::Json("{\"a\":1}".into())),
            ("raw", RequestBody::Raw { content: "hello".into(), language: Some("xml".into()) }),
            (
                "urlencoded",
                RequestBody::UrlEncoded(vec![KeyValue { key: "a".into(), value: "1".into(), enabled: true }]),
            ),
            (
                "formdata",
                RequestBody::FormData(vec![FormField {
                    key: "f".into(),
                    value: "/tmp/x".into(),
                    enabled: true,
                    is_file: true,
                    content_type: None,
                }]),
            ),
            ("binary", RequestBody::Binary { path: "/tmp/file.bin".into() }),
            (
                "graphql",
                RequestBody::Graphql {
                    query: "{ pets { id } }".into(),
                    variables: Some("{}".into()),
                    operation_name: Some("Pets".into()),
                },
            ),
        ];
        let tree = ImportedNode {
            name: "Bodies".into(),
            requests: bodies
                .iter()
                .map(|(name, body)| ImportedRequest {
                    name: (*name).into(),
                    method: "POST".into(),
                    url: "https://api.test".into(),
                    body: body.clone(),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        interop::apply_import(&conn, &ws_id, None, &tree, ConflictMode::Skip, interop::ImportPlacement::AsSubfolder).unwrap();

        let json = export_full(&conn, &[ws_id], false, false).unwrap();

        let mut fresh = crate::store::db::open_in_memory().unwrap();
        crate::store::workspaces::ensure_default(&mut fresh).unwrap();
        apply_full(&fresh, &json, ConflictMode::Skip).unwrap();

        let ws = store::workspaces::list(&fresh).unwrap().into_iter().next().unwrap();
        let roots = store::collections::list_children(&fresh, &ws.id, None).unwrap();
        let col = roots.iter().find(|c| c.name == "Bodies").unwrap();
        let reqs = store::requests::list_by_collection(&fresh, &col.id).unwrap();
        for (name, expected) in &bodies {
            let req = reqs.iter().find(|r| r.name == *name).unwrap_or_else(|| panic!("request {name} missing"));
            assert_eq!(&req.body, expected, "body mode {name} lost or mangled in round trip");
        }
    }

    /// A saved streaming request (kind + opaque `stream_config`) must survive
    /// export_full → apply_full into a fresh DB just like an HTTP request's
    /// body does above — this is the exact path saving a WS/SSE/gRPC request
    /// into a collection, then exporting/reimporting the workspace, exercises.
    #[test]
    fn streaming_kind_and_config_survive_a_restman_round_trip() {
        use crate::model::RequestKind;

        let (conn, ws_id) = seeded_conn();
        let config = serde_json::json!({ "url": "wss://example.com", "headers": [] });
        let tree = ImportedNode {
            name: "Streaming".into(),
            requests: vec![ImportedRequest {
                name: "live updates".into(),
                method: "WS".into(),
                url: String::new(),
                kind: RequestKind::Ws,
                stream_config: Some(config.clone()),
                ..Default::default()
            }],
            ..Default::default()
        };
        interop::apply_import(&conn, &ws_id, None, &tree, ConflictMode::Skip, interop::ImportPlacement::AsSubfolder).unwrap();

        let json = export_full(&conn, &[ws_id], false, false).unwrap();

        let mut fresh = crate::store::db::open_in_memory().unwrap();
        crate::store::workspaces::ensure_default(&mut fresh).unwrap();
        apply_full(&fresh, &json, ConflictMode::Skip).unwrap();

        let ws = store::workspaces::list(&fresh).unwrap().into_iter().next().unwrap();
        let roots = store::collections::list_children(&fresh, &ws.id, None).unwrap();
        let col = roots.iter().find(|c| c.name == "Streaming").unwrap();
        let reqs = store::requests::list_by_collection(&fresh, &col.id).unwrap();
        let req = reqs.iter().find(|r| r.name == "live updates").unwrap();
        assert_eq!(req.kind, RequestKind::Ws);
        assert_eq!(req.stream_config, Some(config));
    }

    /// Two workspaces with nested folders, scripts, secret auth, and secret
    /// variables at every scope.
    fn seed_rich(conn: &mut Connection, default_ws: &str) -> String {
        let tree = ImportedNode {
            name: "API".into(),
            description: Some("main".into()),
            auth: AuthConfig::Bearer { token: "col-secret".into(), prefix: crate::model::auth::default_bearer_prefix() },
            requests: vec![ImportedRequest {
                name: "Login".into(),
                method: "POST".into(),
                url: "https://api.test/login".into(),
                pre_request_script: "console.log('pre')".into(),
                post_response_script: "restman.test('ok', () => {})".into(),
                auth: RequestAuth::Own(AuthConfig::Basic {
                    username: "u".into(),
                    password: "req-secret".into(),
                }),
                ..Default::default()
            }],
            children: vec![ImportedNode {
                name: "Nested".into(),
                requests: vec![ImportedRequest {
                    name: "Deep".into(),
                    method: "GET".into(),
                    url: "https://api.test/deep".into(),
                    ..Default::default()
                }],
                ..Default::default()
            }],
        };
        interop::apply_import(conn, default_ws, None, &tree, ConflictMode::Skip, interop::ImportPlacement::AsSubfolder).unwrap();

        let env = store::environments::create(conn, default_ws, None, "Prod", Some("main")).unwrap();
        store::variables::create(
            conn,
            &VarScope::Environment(env.id.clone()),
            &VariableInput {
                key: "token".into(),
                value: "env-secret".into(),
                var_type: VarType::String,
                is_secret: true,
                enabled: true,
            },
        )
        .unwrap();
        store::variables::create(
            conn,
            &VarScope::Workspace(default_ws.to_string()),
            &VariableInput {
                key: "baseUrl".into(),
                value: "https://api.test".into(),
                var_type: VarType::String,
                is_secret: false,
                enabled: true,
            },
        )
        .unwrap();
        store::variables::create(
            conn,
            &VarScope::Global,
            &VariableInput {
                key: "globalSecret".into(),
                value: "g-secret".into(),
                var_type: VarType::String,
                is_secret: true,
                enabled: true,
            },
        )
        .unwrap();

        let second = store::workspaces::create(conn, "Second").unwrap();
        let tree2 = ImportedNode {
            name: "Other".into(),
            requests: vec![ImportedRequest {
                name: "Ping".into(),
                method: "GET".into(),
                url: "https://other.test".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        interop::apply_import(conn, &second.id, None, &tree2, ConflictMode::Skip, interop::ImportPlacement::AsSubfolder).unwrap();
        second.id
    }

    #[test]
    fn round_trip_with_secrets_restores_structure_and_real_secrets() {
        let (mut conn, ws_id) = seeded_conn();
        let second_id = seed_rich(&mut conn, &ws_id);

        let json = export_full(&conn, &[ws_id.clone(), second_id], true, true).unwrap();
        assert!(json.contains("col-secret"), "hydrated collection secret missing");
        assert!(json.contains("req-secret"), "hydrated request secret missing");
        assert!(json.contains("env-secret"), "hydrated variable secret missing");
        assert!(json.contains("g-secret"), "hydrated global secret missing");

        // Fresh DB = fresh machine. Keychain is process-global in tests, so
        // wipe nothing — apply mints new ids, so new keychain keys.
        let mut fresh = crate::store::db::open_in_memory().unwrap();
        crate::store::workspaces::ensure_default(&mut fresh).unwrap();
        let report = apply_full(&fresh, &json, ConflictMode::Skip).unwrap();

        // "Second" is new; the default workspace name collides and merges.
        assert_eq!(report.workspaces_created, 1, "{report:?}");
        assert_eq!(report.created_collections, 3); // API + Nested + Other
        assert_eq!(report.created_requests, 3);
        assert_eq!(report.environments_created, 1);
        assert_eq!(report.variables_created, 3);
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);

        // Collection auth secret really landed in the keychain.
        let default_ws = store::workspaces::list(&fresh)
            .unwrap()
            .into_iter()
            .find(|w| w.name != "Second")
            .unwrap();
        let roots = store::collections::list_children(&fresh, &default_ws.id, None).unwrap();
        let api = roots.iter().find(|c| c.name == "API").unwrap();
        let stored = store::collections::get(&fresh, &api.id).unwrap();
        assert!(stored.auth.is_masked());
        let owner = crate::auth::owner_key("collection", &api.id);
        let real = crate::auth::hydrate(&owner, stored.auth).unwrap();
        assert_eq!(real, AuthConfig::Bearer { token: "col-secret".into(), prefix: crate::model::auth::default_bearer_prefix() });

        // Scripts survived.
        let reqs = store::requests::list_by_collection(&fresh, &api.id).unwrap();
        let login = reqs.iter().find(|r| r.name == "Login").unwrap();
        assert_eq!(login.pre_request_script, "console.log('pre')");
        assert_eq!(login.post_response_script, "restman.test('ok', () => {})");

        // Secret variable real value in keychain, not DB column.
        let envs = store::environments::list(&fresh, &default_ws.id).unwrap();
        let prod = envs.iter().find(|e| e.name == "Prod").unwrap();
        assert_eq!(prod.group_name.as_deref(), Some("main"));
        let vars = store::variables::list(&fresh, &VarScope::Environment(prod.id.clone())).unwrap();
        let token = vars.iter().find(|v| v.key == "token").unwrap();
        assert_eq!(token.value, "");
        let real = crate::secrets::get(&store::variables::keychain_key(&token.id)).unwrap().unwrap();
        assert_eq!(real, "env-secret");
    }

    #[test]
    fn export_without_secrets_masks_everything() {
        let (mut conn, ws_id) = seeded_conn();
        let second_id = seed_rich(&mut conn, &ws_id);

        let json = export_full(&conn, &[ws_id, second_id], false, false).unwrap();
        for real in ["col-secret", "req-secret", "env-secret", "g-secret"] {
            assert!(!json.contains(real), "real secret {real:?} leaked into masked export");
        }
        assert!(json.contains(SECRET_MASK));
        let doc: RestmanExport = serde_json::from_str(&json).unwrap();
        assert!(!doc.includes_secrets);
        assert!(doc.workspaces.iter().all(|w| w.settings.is_none()));
    }

    #[test]
    fn selective_export_excludes_unselected_workspace_but_keeps_globals() {
        let (mut conn, ws_id) = seeded_conn();
        let _second_id = seed_rich(&mut conn, &ws_id);

        let json = export_full(&conn, &[ws_id], true, false).unwrap();
        let doc: RestmanExport = serde_json::from_str(&json).unwrap();
        assert_eq!(doc.workspaces.len(), 1);
        assert!(doc.workspaces[0].collections.iter().all(|c| c.name != "Other"));
        assert_eq!(doc.global_variables.len(), 1);
        assert_eq!(doc.global_variables[0].key, "globalSecret");
    }

    #[test]
    fn preview_reports_counts_collisions_and_masked_secrets() {
        let (mut conn, ws_id) = seeded_conn();
        let second_id = seed_rich(&mut conn, &ws_id);

        let json = export_full(&conn, &[ws_id, second_id], false, false).unwrap();
        let preview = preview_full(&conn, &json).unwrap();

        assert_eq!(preview.version, EXPORT_VERSION);
        assert_eq!(preview.workspaces.len(), 2);
        let default_ws = &preview.workspaces[0];
        assert!(default_ws.exists, "same-name workspace should flag as existing");
        assert_eq!(default_ws.collections, 2); // API + Nested
        assert_eq!(default_ws.requests, 2);
        assert_eq!(default_ws.environments, 1);
        assert_eq!(default_ws.variables, 2); // baseUrl + env token
        assert_eq!(preview.global_variables, 1);
        // col-secret + req-secret (bearer + basic password) + env token + global.
        assert_eq!(preview.masked_secrets, 4);
        assert!(!preview.warnings.is_empty());
    }

    #[test]
    fn reimport_conflict_modes_behave_like_collection_import() {
        let (mut conn, ws_id) = seeded_conn();
        let second_id = seed_rich(&mut conn, &ws_id);
        let json = export_full(&conn, &[ws_id.clone(), second_id], true, false).unwrap();

        // Skip: everything already exists by name → nothing created.
        let report = apply_full(&conn, &json, ConflictMode::Skip).unwrap();
        assert_eq!(report.workspaces_created, 0);
        assert_eq!(report.created_collections, 0);
        assert_eq!(report.created_requests, 0);
        assert_eq!(report.skipped, 3);
        assert_eq!(report.environments_created, 0);
        assert_eq!(report.variables_created, 0);
        assert!(report.variables_skipped >= 2, "{report:?}");

        // Overwrite: requests and variables replaced in place.
        let report = apply_full(&conn, &json, ConflictMode::Overwrite).unwrap();
        assert_eq!(report.overwritten, 3);
        assert_eq!(report.variables_overwritten, 3);

        // Merge: same-name requests duplicated under disambiguated names.
        let report = apply_full(&conn, &json, ConflictMode::Merge).unwrap();
        assert_eq!(report.created_requests, 3);
    }

    #[test]
    fn masked_secret_variable_reimport_keeps_existing_value_on_overwrite() {
        let (mut conn, ws_id) = seeded_conn();
        let second_id = seed_rich(&mut conn, &ws_id);
        let json = export_full(&conn, &[ws_id.clone(), second_id], false, false).unwrap();

        // Overwrite re-import of a masked export: the existing real secret
        // must survive, not be clobbered by an empty value.
        let report = apply_full(&conn, &json, ConflictMode::Overwrite).unwrap();
        assert!(report.warnings.iter().all(|w| !w.contains("globalSecret")), "{:?}", report.warnings);

        let globals = store::variables::list(&conn, &VarScope::Global).unwrap();
        let g = globals.iter().find(|v| v.key == "globalSecret").unwrap();
        let real = crate::secrets::get(&store::variables::keychain_key(&g.id)).unwrap().unwrap();
        assert_eq!(real, "g-secret", "existing secret clobbered by masked import");
    }

    #[test]
    fn masked_secret_variable_import_into_fresh_db_warns_and_lands_empty() {
        let (mut conn, ws_id) = seeded_conn();
        let second_id = seed_rich(&mut conn, &ws_id);
        let json = export_full(&conn, &[ws_id, second_id], false, false).unwrap();

        let mut fresh = crate::store::db::open_in_memory().unwrap();
        crate::store::workspaces::ensure_default(&mut fresh).unwrap();
        let report = apply_full(&fresh, &json, ConflictMode::Skip).unwrap();
        assert!(
            report.warnings.iter().any(|w| w.contains("globalSecret")),
            "{:?}",
            report.warnings
        );
    }

    #[test]
    fn newer_export_version_is_rejected() {
        let (conn, _ws_id) = seeded_conn();
        let json = r#"{"restmanExportVersion": 999, "createdAt": 0, "appVersion": "9.9.9", "includesSecrets": false, "workspaces": []}"#;
        let err = apply_full(&conn, json, ConflictMode::Skip).unwrap_err();
        assert!(err.to_string().contains("update restman"), "{err}");
        let err = preview_full(&conn, json).unwrap_err();
        assert!(err.to_string().contains("update restman"), "{err}");
    }

    #[test]
    fn non_restman_json_is_rejected() {
        let (conn, _ws_id) = seeded_conn();
        let err = preview_full(&conn, r#"{"info": "postman collection"}"#).unwrap_err();
        assert!(err.to_string().contains("restmanExportVersion"), "{err}");
    }

    #[test]
    fn portable_settings_apply_only_to_new_workspaces() {
        let (mut conn, ws_id) = seeded_conn();
        let _second = seed_rich(&mut conn, &ws_id);
        store::workspace_settings::set(
            &conn,
            &WorkspaceSettings {
                proxy_url: Some("http://proxy.corp:8080".into()),
                ..WorkspaceSettings::empty(&ws_id)
            },
        )
        .unwrap();
        let json = export_full(&conn, &[ws_id], true, true).unwrap();

        let mut fresh = crate::store::db::open_in_memory().unwrap();
        let fresh_default = crate::store::workspaces::ensure_default(&mut fresh).unwrap();
        // Rename the default workspace out of the way so the imported one is new.
        store::workspaces::update(&fresh, &fresh_default.id, "Something else").unwrap();

        apply_full(&fresh, &json, ConflictMode::Skip).unwrap();
        let imported = store::workspaces::list(&fresh)
            .unwrap()
            .into_iter()
            .find(|w| w.id != fresh_default.id)
            .unwrap();
        let settings = store::workspace_settings::get(&fresh, &imported.id).unwrap();
        assert_eq!(settings.proxy_url.as_deref(), Some("http://proxy.corp:8080"));

        // Re-import into the now-existing workspace with a different proxy
        // must NOT clobber.
        store::workspace_settings::set(
            &fresh,
            &WorkspaceSettings {
                proxy_url: Some("http://other:1".into()),
                ..WorkspaceSettings::empty(&imported.id)
            },
        )
        .unwrap();
        apply_full(&fresh, &json, ConflictMode::Skip).unwrap();
        let settings = store::workspace_settings::get(&fresh, &imported.id).unwrap();
        assert_eq!(settings.proxy_url.as_deref(), Some("http://other:1"));
    }
}
