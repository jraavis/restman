//! Import/export: a shared intermediate representation (IR) that every
//! format's importer/exporter maps to/from, plus the DB-facing apply/collect
//! pair that turns an IR tree into real collections/requests and back.
//!
//! Layout mirrors `model`/`store`: this module owns the IR shape and the
//! conflict-resolution policy; `commands::interop` is the thin Tauri-facing
//! wrapper. Per-format code lives in sibling modules (`postman`, …), each
//! exposing `parse(&str) -> AppResult<ImportPreview>` and
//! `export(&ImportedNode) -> AppResult<String>`.
//!
//! Secrets: `collect()` reads `auth_json` straight from the DB, which
//! `crate::auth::persist`/`persist_request_auth` already mask before
//! storage — so every exporter gets export-safe auth for free, with no
//! extra masking step here. `apply_import()` is the mirror image: any auth
//! carried by an imported tree is routed through `crate::auth::persist`/
//! `persist_request_auth` before it touches `auth_json`, so a freshly
//! imported Bearer token (say) lands in the keychain, never in plaintext
//! in the DB.

pub mod bruno;
pub mod curl;
pub mod environment;
pub mod har;
pub mod http_file;
pub mod insomnia;
pub mod openapi;
pub mod plugin;
pub mod postman;
pub mod restman;

use crate::error::AppResult;
use crate::model::auth::{AuthConfig, RequestAuth};
use crate::model::http::{HeaderEntry, KeyValue, RequestBody, RequestOptions};
use crate::model::{Collection, SavedRequestInput};
use crate::store;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

// Re-export the environment IO types at the module root so
// `commands::interop` can pull them with one `use crate::interop::{...}`
// alongside the collection-import types.
pub use environment::{EnvironmentImportReport, EnvironmentPreview};

/// One importable/exportable request, format-agnostic.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedRequest {
    pub name: String,
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub headers: Vec<HeaderEntry>,
    #[serde(default)]
    pub query: Vec<KeyValue>,
    #[serde(default)]
    pub body: RequestBody,
    #[serde(default)]
    pub options: RequestOptions,
    #[serde(default)]
    pub auth: RequestAuth,
    #[serde(default)]
    pub pre_request_script: String,
    #[serde(default)]
    pub post_response_script: String,
    /// Only restman's own export ever produces a non-`Http` kind — every
    /// other format importer (Postman/Insomnia/Bruno/curl/OpenAPI/HAR/
    /// http-file) only knows HTTP, so they all leave this at the derived
    /// `Default` via `..Default::default()`.
    #[serde(default)]
    pub kind: crate::model::RequestKind,
    #[serde(default)]
    pub stream_config: Option<serde_json::Value>,
}

/// One importable/exportable collection (or folder — same shape, folders are
/// just nodes with no auth-of-their-own significance beyond `AuthConfig`).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedNode {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub auth: AuthConfig,
    #[serde(default)]
    pub requests: Vec<ImportedRequest>,
    #[serde(default)]
    pub children: Vec<ImportedNode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportFormat {
    Postman,
    Curl,
    OpenApi,
    Har,
    Insomnia,
    Bruno,
    HttpFile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    Postman,
    Curl,
    OpenApi,
    Har,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ImportStats {
    pub folders: usize,
    pub requests: usize,
    pub warnings: usize,
}

/// What a parser hands back before anything touches the DB — the frontend
/// renders this as a preview tree the user can inspect before committing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreview {
    pub root: ImportedNode,
    pub warnings: Vec<String>,
    pub stats: ImportStats,
}

impl ImportPreview {
    fn new(root: ImportedNode, warnings: Vec<String>) -> Self {
        let (folders, requests) = count(&root);
        // `root` itself is the collection the user is importing into/as —
        // not counted as a "folder" (it has no sibling at its own level to
        // collide with until `apply_import` places it under a parent).
        let stats = ImportStats { folders: folders.saturating_sub(1), requests, warnings: warnings.len() };
        Self { root, warnings, stats }
    }
}

fn count(node: &ImportedNode) -> (usize, usize) {
    node.children.iter().fold((1, node.requests.len()), |(f, r), child| {
        let (cf, cr) = count(child);
        (f + cf, r + cr)
    })
}

/// Imported auth may already carry `SECRET_MASK` — e.g. re-importing a
/// collection this app itself exported. `apply_node` always mints a brand
/// new owner id for a freshly created collection/request, so there is no
/// prior keychain entry for `crate::auth::persist`'s "already `SECRET_MASK`,
/// keychain already holds it" branch to find; left alone, the secret would
/// silently resolve to `""` the first time it's hydrated, with no warning.
/// Clearing masked fields to `""` here instead makes the gap honest (the
/// config becomes "no secret set", not "secret present but unreadable") so
/// the caller can warn instead of failing silently.
fn strip_unrecoverable_masks(config: AuthConfig) -> (AuthConfig, bool) {
    let masked: Vec<&'static str> = config
        .secret_fields()
        .into_iter()
        .filter(|(_, v)| *v == crate::model::variable::SECRET_MASK)
        .map(|(slot, _)| slot)
        .collect();
    let any_masked = !masked.is_empty();
    let mut config = config;
    for slot in masked {
        config = config.with_secret_field(slot, String::new());
    }
    (config, any_masked)
}

fn strip_unrecoverable_request_auth_masks(auth: RequestAuth) -> (RequestAuth, bool) {
    match auth {
        RequestAuth::Inherit => (RequestAuth::Inherit, false),
        RequestAuth::Own(cfg) => {
            let (cfg, any_masked) = strip_unrecoverable_masks(cfg);
            (RequestAuth::Own(cfg), any_masked)
        }
    }
}

/// Where to place an import when the user targets an existing folder via
/// "Import here…". Ignored when `parent_id` is `None` (workspace-root import
/// always creates a top-level collection).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportPlacement {
    /// Merge `root.requests` into the target folder; recurse `root.children`
    /// as subfolders under it.
    IntoFolder,
    /// Create/find a child collection named `root.name` under the target.
    #[default]
    AsSubfolder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictMode {
    /// Reuse an existing same-name collection/folder; an existing same-name
    /// request is left untouched and the imported copy is dropped.
    Skip,
    /// Reuse an existing same-name collection/folder; an existing same-name
    /// request has its fields replaced by the imported copy.
    Overwrite,
    /// Reuse an existing same-name collection/folder; an existing same-name
    /// request is kept *and* the imported copy is added alongside it under
    /// a disambiguated name.
    Merge,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub created_collections: usize,
    pub created_requests: usize,
    pub skipped: usize,
    pub overwritten: usize,
    pub warnings: Vec<String>,
}

/// Parse raw file content for `format` into a preview tree. No DB access —
/// pure text-in, IR-out, so the frontend can show a preview before anything
/// is committed.
pub fn parse(format: ImportFormat, content: &str) -> AppResult<ImportPreview> {
    match format {
        ImportFormat::Postman => postman::parse(content),
        ImportFormat::Curl => curl::parse(content),
        ImportFormat::OpenApi => openapi::parse(content),
        ImportFormat::Har => har::parse(content),
        ImportFormat::Insomnia => insomnia::parse(content),
        ImportFormat::Bruno => bruno::parse(content),
        ImportFormat::HttpFile => http_file::parse(content),
    }
}

/// Serialize `node` to `format`'s text representation.
///
/// These four formats (Postman/cURL/OpenAPI/HAR) only understand HTTP — a
/// streaming-kind request's HTTP-shaped fields are unused placeholders (see
/// `model::RequestKind`), so exporting one as-is would emit a broken item
/// (e.g. `curl -X WS ''`, its real URL trapped in `stream_config`). Only
/// restman's own export (`restman::export_full`, via `collect_node`) round-
/// trips streaming requests; every other format silently drops them here
/// rather than emit garbage.
pub fn export(format: ExportFormat, node: &ImportedNode) -> AppResult<String> {
    let node = strip_non_http_requests(node);
    match format {
        ExportFormat::Postman => postman::export(&node),
        ExportFormat::Curl => curl::export(&node),
        ExportFormat::OpenApi => openapi::export(&node),
        ExportFormat::Har => har::export(&node),
    }
}

fn strip_non_http_requests(node: &ImportedNode) -> ImportedNode {
    ImportedNode {
        name: node.name.clone(),
        description: node.description.clone(),
        auth: node.auth.clone(),
        requests: node.requests.iter().filter(|r| r.kind == crate::model::RequestKind::Http).cloned().collect(),
        children: node.children.iter().map(strip_non_http_requests).collect(),
    }
}

/// Materialize an `ImportedNode` tree under `parent_id` (`None` = workspace
/// top level), applying `mode` at every name collision. When `parent_id` is
/// set and `placement` is `IntoFolder`, `root.requests` land directly in the
/// target folder and `root.children` become subfolders under it; otherwise
/// `root` is placed as one collection under `parent_id` (or at workspace
/// root when `parent_id` is `None`).
pub fn apply_import(
    conn: &Connection,
    workspace_id: &str,
    parent_id: Option<&str>,
    root: &ImportedNode,
    mode: ConflictMode,
    placement: ImportPlacement,
) -> AppResult<ImportReport> {
    let mut report = ImportReport::default();
    if parent_id.is_some() && placement == ImportPlacement::IntoFolder {
        let target = store::collections::get(conn, parent_id.unwrap())?;
        apply_collection_auth(conn, &target.name, &target.id, root.auth.clone(), &mut report)?;
        apply_requests(conn, &target.id, &root.requests, mode, &mut report)?;
        for child in &root.children {
            apply_node(conn, workspace_id, Some(target.id.as_str()), child, mode, &mut report)?;
        }
    } else {
        apply_node(conn, workspace_id, parent_id, root, mode, &mut report)?;
    }
    Ok(report)
}

fn apply_node(
    conn: &Connection,
    workspace_id: &str,
    parent_id: Option<&str>,
    node: &ImportedNode,
    mode: ConflictMode,
    report: &mut ImportReport,
) -> AppResult<()> {
    let existing = store::collections::list_children(conn, workspace_id, parent_id)?
        .into_iter()
        .find(|c| c.name == node.name);

    let collection: Collection = match existing {
        Some(c) => c,
        None => {
            let created = store::collections::create(
                conn,
                workspace_id,
                parent_id,
                &node.name,
                node.description.as_deref(),
            )?;
            report.created_collections += 1;
            apply_collection_auth(conn, &node.name, &created.id, node.auth.clone(), report)?;
            created
        }
    };

    apply_requests(conn, &collection.id, &node.requests, mode, report)?;

    for child in &node.children {
        apply_node(conn, workspace_id, Some(collection.id.as_str()), child, mode, report)?;
    }
    Ok(())
}

fn apply_collection_auth(
    conn: &Connection,
    label: &str,
    collection_id: &str,
    auth: AuthConfig,
    report: &mut ImportReport,
) -> AppResult<()> {
    if auth == AuthConfig::None {
        return Ok(());
    }
    let (auth, masked) = strip_unrecoverable_masks(auth);
    if masked {
        report.warnings.push(format!(
            "\"{label}\": secret was already masked in the imported file and could not be recovered — re-enter it"
        ));
    }
    store::collections::update_auth(conn, collection_id, auth)?;
    Ok(())
}

fn apply_requests(
    conn: &Connection,
    collection_id: &str,
    requests: &[ImportedRequest],
    mode: ConflictMode,
    report: &mut ImportReport,
) -> AppResult<()> {
    let existing_requests = store::requests::list_by_collection(conn, collection_id)?;
    for req in requests {
        let collision = existing_requests.iter().find(|r| r.name == req.name);
        let (auth, masked) = strip_unrecoverable_request_auth_masks(req.auth.clone());
        let input = SavedRequestInput {
            name: req.name.clone(),
            method: req.method.clone(),
            url: req.url.clone(),
            headers: req.headers.clone(),
            query: req.query.clone(),
            body: req.body.clone(),
            options: req.options.clone(),
            auth,
            pre_request_script: req.pre_request_script.clone(),
            post_response_script: req.post_response_script.clone(),
            kind: req.kind,
            stream_config: req.stream_config.clone(),
        };
        let mut persisted = false;
        match collision {
            None => {
                store::requests::create(conn, collection_id, &input)?;
                report.created_requests += 1;
                persisted = true;
            }
            Some(existing) => match mode {
                ConflictMode::Skip => {
                    report.skipped += 1;
                }
                ConflictMode::Overwrite => {
                    store::requests::update(conn, &existing.id, &input)?;
                    // Any open tab linked to this request still carries the
                    // pre-import draft; left alone, clicking the request
                    // activates that tab and shows the stale content (e.g. a
                    // body the import just replaced). The import is the
                    // source of truth here, so push it into the tab drafts.
                    let draft = crate::model::http::HttpRequest {
                        method: input.method.clone(),
                        url: input.url.clone(),
                        headers: input.headers.clone(),
                        query: input.query.clone(),
                        body: input.body.clone(),
                        options: input.options.clone(),
                        auth: Default::default(),
                    };
                    store::tabs::refresh_drafts_for_request(conn, &existing.id, &draft)?;
                    report.overwritten += 1;
                    persisted = true;
                }
                ConflictMode::Merge => {
                    let mut disambiguated = input;
                    disambiguated.name =
                        unique_request_name(&existing_requests, &disambiguated.name);
                    store::requests::create(conn, collection_id, &disambiguated)?;
                    report.created_requests += 1;
                    persisted = true;
                }
            },
        }
        if persisted && masked {
            report.warnings.push(format!(
                "\"{}\": secret was already masked in the imported file and could not be recovered — re-enter it",
                req.name
            ));
        }
    }
    Ok(())
}

fn unique_request_name(existing: &[crate::model::SavedRequest], base: &str) -> String {
    let taken: std::collections::HashSet<&str> = existing.iter().map(|r| r.name.as_str()).collect();
    if !taken.contains(base) {
        return base.to_string();
    }
    let mut n = 2;
    loop {
        let candidate = format!("{base} ({n})");
        if !taken.contains(candidate.as_str()) {
            return candidate;
        }
        n += 1;
    }
}

/// Read a collection (and everything nested under it) back out as an
/// `ImportedNode` tree, for export. Auth is read straight from `auth_json`,
/// which is already mask-on-write — see module doc.
pub fn collect(conn: &Connection, collection_id: &str) -> AppResult<ImportedNode> {
    collect_with_secrets(conn, collection_id, false)
}

/// Read a single saved request back out as a one-request `ImportedNode` tree,
/// for per-request export.
pub fn collect_request(conn: &Connection, request_id: &str) -> AppResult<ImportedNode> {
    let req = store::requests::get(conn, request_id)?;
    Ok(ImportedNode {
        name: req.name.clone(),
        description: None,
        auth: AuthConfig::None,
        requests: vec![ImportedRequest {
            name: req.name,
            method: req.method,
            url: req.url,
            headers: req.headers,
            query: req.query,
            body: req.body,
            options: req.options,
            auth: req.auth,
            pre_request_script: req.pre_request_script,
            post_response_script: req.post_response_script,
            kind: req.kind,
            stream_config: req.stream_config,
        }],
        children: vec![],
    })
}

/// `collect`, with an opt-in knob to hydrate real auth secrets from the
/// keychain instead of carrying the stored `SECRET_MASK`. Only the
/// restman-native full export uses `hydrate_secrets = true` (behind an
/// explicit user opt-in) — every interchange-format export stays masked.
pub fn collect_with_secrets(
    conn: &Connection,
    collection_id: &str,
    hydrate_secrets: bool,
) -> AppResult<ImportedNode> {
    let collection = store::collections::get(conn, collection_id)?;
    collect_node(conn, &collection, hydrate_secrets)
}

fn collect_node(conn: &Connection, collection: &Collection, hydrate_secrets: bool) -> AppResult<ImportedNode> {
    let requests = store::requests::list_by_collection(conn, &collection.id)?
        .into_iter()
        .map(|r| {
            let auth = if hydrate_secrets {
                crate::auth::hydrate_request_auth(&crate::auth::owner_key("request", &r.id), r.auth)?
            } else {
                r.auth
            };
            Ok(ImportedRequest {
                name: r.name,
                method: r.method,
                url: r.url,
                headers: r.headers,
                query: r.query,
                body: r.body,
                options: r.options,
                auth,
                pre_request_script: r.pre_request_script,
                post_response_script: r.post_response_script,
                kind: r.kind,
                stream_config: r.stream_config,
            })
        })
        .collect::<AppResult<Vec<_>>>()?;

    let children = store::collections::list_children(conn, &collection.workspace_id, Some(&collection.id))?
        .iter()
        .map(|c| collect_node(conn, c, hydrate_secrets))
        .collect::<AppResult<Vec<_>>>()?;

    let auth = if hydrate_secrets {
        crate::auth::hydrate(&crate::auth::owner_key("collection", &collection.id), collection.auth.clone())?
    } else {
        collection.auth.clone()
    };

    Ok(ImportedNode {
        name: collection.name.clone(),
        description: collection.description.clone(),
        auth,
        requests,
        children,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::auth::AuthConfig;

    fn leaf_request(name: &str) -> ImportedRequest {
        ImportedRequest { name: name.into(), method: "GET".into(), url: "https://a.test".into(), ..Default::default() }
    }

    fn sample_tree() -> ImportedNode {
        ImportedNode {
            name: "Root".into(),
            description: Some("desc".into()),
            auth: AuthConfig::Bearer { token: "tok".into(), prefix: crate::model::auth::default_bearer_prefix() },
            requests: vec![leaf_request("Get thing")],
            children: vec![ImportedNode {
                name: "Sub".into(),
                requests: vec![leaf_request("Nested req")],
                ..Default::default()
            }],
        }
    }

    #[test]
    fn apply_then_collect_round_trips_tree_shape() {
        let mut conn = crate::store::db::open_in_memory().unwrap();
        let ws = crate::store::workspaces::ensure_default(&mut conn).unwrap();

        let report = apply_import(&conn, &ws.id, None, &sample_tree(), ConflictMode::Skip, ImportPlacement::AsSubfolder).unwrap();
        assert_eq!(report.created_collections, 2); // Root + Sub
        assert_eq!(report.created_requests, 2);

        let roots = store::collections::list_children(&conn, &ws.id, None).unwrap();
        assert_eq!(roots.len(), 1);
        let collected = collect(&conn, &roots[0].id).unwrap();

        // Auth round-trips masked (bearer token was persisted -> keychain,
        // auth_json holds the mask) — collect() must not try to hydrate it.
        assert_eq!(
            collected,
            ImportedNode {
                name: "Root".into(),
                description: Some("desc".into()),
                auth: AuthConfig::Bearer { token: crate::model::variable::SECRET_MASK.into(), prefix: crate::model::auth::default_bearer_prefix() },
                requests: vec![leaf_request("Get thing")],
                children: vec![ImportedNode {
                    name: "Sub".into(),
                    requests: vec![leaf_request("Nested req")],
                    ..Default::default()
                }],
            }
        );
    }

    #[test]
    fn reimport_skip_mode_leaves_existing_request_untouched() {
        let mut conn = crate::store::db::open_in_memory().unwrap();
        let ws = crate::store::workspaces::ensure_default(&mut conn).unwrap();

        apply_import(&conn, &ws.id, None, &sample_tree(), ConflictMode::Skip, ImportPlacement::AsSubfolder).unwrap();
        let mut second = sample_tree();
        second.requests[0].url = "https://changed.test".into();
        let report = apply_import(&conn, &ws.id, None, &second, ConflictMode::Skip, ImportPlacement::AsSubfolder).unwrap();

        // Re-importing the same tree finds the Root/Sub collections and both
        // requests already present by name; nothing new is created.
        assert_eq!(report.created_collections, 0);
        assert_eq!(report.created_requests, 0);
        assert_eq!(report.skipped, 2);

        let roots = store::collections::list_children(&conn, &ws.id, None).unwrap();
        let reqs = store::requests::list_by_collection(&conn, &roots[0].id).unwrap();
        assert_eq!(reqs[0].url, "https://a.test"); // untouched, not "changed.test"
    }

    #[test]
    fn reimport_overwrite_mode_replaces_existing_request() {
        let mut conn = crate::store::db::open_in_memory().unwrap();
        let ws = crate::store::workspaces::ensure_default(&mut conn).unwrap();

        apply_import(&conn, &ws.id, None, &sample_tree(), ConflictMode::Skip, ImportPlacement::AsSubfolder).unwrap();
        let mut second = sample_tree();
        second.requests[0].url = "https://changed.test".into();
        let report = apply_import(&conn, &ws.id, None, &second, ConflictMode::Overwrite, ImportPlacement::AsSubfolder).unwrap();

        assert_eq!(report.overwritten, 2);
        let roots = store::collections::list_children(&conn, &ws.id, None).unwrap();
        let reqs = store::requests::list_by_collection(&conn, &roots[0].id).unwrap();
        assert!(reqs.iter().any(|r| r.url == "https://changed.test"));
    }

    /// A tab already open on a request that an Overwrite import just
    /// replaced must not keep showing the pre-import draft — the classic
    /// symptom was the Body tab flipping to "None" (the stale draft) even
    /// though the imported request has a body.
    #[test]
    fn reimport_overwrite_mode_refreshes_stale_tab_drafts() {
        use crate::model::http::{HttpRequest, RequestBody};

        let mut conn = crate::store::db::open_in_memory().unwrap();
        let ws = crate::store::workspaces::ensure_default(&mut conn).unwrap();

        apply_import(&conn, &ws.id, None, &sample_tree(), ConflictMode::Skip, ImportPlacement::AsSubfolder).unwrap();
        let roots = store::collections::list_children(&conn, &ws.id, None).unwrap();
        let req = store::requests::list_by_collection(&conn, &roots[0].id)
            .unwrap()
            .into_iter()
            .find(|r| r.name == "Get thing")
            .unwrap();

        // Open a tab on it with the current (bodyless) draft.
        let stale_draft = HttpRequest { method: req.method.clone(), url: req.url.clone(), ..Default::default() };
        let tab = store::tabs::create(&mut conn, &ws.id, Some(&req.id), &req.name, &stale_draft).unwrap();

        // Re-import with a body added, Overwrite mode.
        let mut second = sample_tree();
        second.requests[0].body = RequestBody::Json("{\"a\":1}".into());
        apply_import(&conn, &ws.id, None, &second, ConflictMode::Overwrite, ImportPlacement::AsSubfolder).unwrap();

        let refreshed = store::tabs::get(&conn, &tab.id).unwrap();
        assert_eq!(refreshed.draft.body, RequestBody::Json("{\"a\":1}".into()), "tab draft still stale after overwrite import");
    }

    #[test]
    fn reimport_merge_mode_adds_disambiguated_sibling() {
        let mut conn = crate::store::db::open_in_memory().unwrap();
        let ws = crate::store::workspaces::ensure_default(&mut conn).unwrap();

        apply_import(&conn, &ws.id, None, &sample_tree(), ConflictMode::Skip, ImportPlacement::AsSubfolder).unwrap();
        let report = apply_import(&conn, &ws.id, None, &sample_tree(), ConflictMode::Merge, ImportPlacement::AsSubfolder).unwrap();

        assert_eq!(report.created_requests, 2); // "Get thing (2)", "Nested req (2)"
        let roots = store::collections::list_children(&conn, &ws.id, None).unwrap();
        let reqs = store::requests::list_by_collection(&conn, &roots[0].id).unwrap();
        assert!(reqs.iter().any(|r| r.name == "Get thing (2)"));
    }

    #[test]
    fn imported_secret_is_persisted_to_keychain_not_left_plaintext() {
        let mut conn = crate::store::db::open_in_memory().unwrap();
        let ws = crate::store::workspaces::ensure_default(&mut conn).unwrap();

        apply_import(&conn, &ws.id, None, &sample_tree(), ConflictMode::Skip, ImportPlacement::AsSubfolder).unwrap();
        let roots = store::collections::list_children(&conn, &ws.id, None).unwrap();
        let stored = store::collections::get(&conn, &roots[0].id).unwrap();
        assert!(stored.auth.is_masked(), "{:?} must be masked at rest", stored.auth);

        let owner = crate::auth::owner_key("collection", &roots[0].id);
        let real = crate::auth::hydrate(&owner, stored.auth).unwrap();
        assert_eq!(real, AuthConfig::Bearer { token: "tok".into(), prefix: crate::model::auth::default_bearer_prefix() });
    }

    /// Postman/cURL/OpenAPI/HAR only understand HTTP — exporting a collection
    /// that also holds a saved WS/SSE/gRPC request must skip that request
    /// rather than emit a broken item (e.g. `curl -X WS ''`, its real URL
    /// trapped in `stream_config`). Regression guard for the export() dispatch
    /// filter, using cURL since its output is trivial to assert against.
    #[test]
    fn export_to_curl_skips_streaming_kind_requests() {
        use crate::model::RequestKind;

        let http_req = ImportedRequest {
            name: "Get thing".into(),
            method: "GET".into(),
            url: "https://api.test/thing".into(),
            ..Default::default()
        };
        let ws_req = ImportedRequest {
            name: "Live updates".into(),
            method: "WS".into(),
            url: String::new(),
            kind: RequestKind::Ws,
            stream_config: Some(serde_json::json!({ "url": "wss://example.com", "headers": [] })),
            ..Default::default()
        };
        let node = ImportedNode {
            name: "Mixed".into(),
            requests: vec![http_req, ws_req],
            ..Default::default()
        };

        let out = export(ExportFormat::Curl, &node).unwrap();
        assert!(out.contains("https://api.test/thing"), "HTTP request must still export:\n{out}");
        assert!(!out.contains("WS"), "streaming request must be skipped, not exported broken:\n{out}");
        assert!(!out.contains("wss://example.com"), "stream_config must never leak into a format export:\n{out}");
    }

    /// Re-importing a file whose auth is already `SECRET_MASK` (e.g. one
    /// this app itself exported) must not silently fabricate a token: the
    /// freshly-created collection has never had a keychain entry, so the
    /// secret cannot be recovered — it should come out empty, not masked-
    /// but-missing, and the report must say so.
    #[test]
    fn reimporting_already_masked_collection_auth_clears_secret_and_warns() {
        let mut conn = crate::store::db::open_in_memory().unwrap();
        let ws = crate::store::workspaces::ensure_default(&mut conn).unwrap();

        let mut tree = sample_tree();
        tree.auth = AuthConfig::Bearer { token: crate::model::variable::SECRET_MASK.into(), prefix: crate::model::auth::default_bearer_prefix() };

        let report = apply_import(&conn, &ws.id, None, &tree, ConflictMode::Skip, ImportPlacement::AsSubfolder).unwrap();
        assert_eq!(report.warnings.len(), 1, "{:?}", report.warnings);
        assert!(report.warnings[0].contains("Root"), "{:?}", report.warnings);

        let roots = store::collections::list_children(&conn, &ws.id, None).unwrap();
        let stored = store::collections::get(&conn, &roots[0].id).unwrap();
        assert_eq!(stored.auth, AuthConfig::Bearer { token: String::new(), prefix: crate::model::auth::default_bearer_prefix() });

        let owner = crate::auth::owner_key("collection", &roots[0].id);
        let real = crate::auth::hydrate(&owner, stored.auth).unwrap();
        assert_eq!(real, AuthConfig::Bearer { token: String::new(), prefix: crate::model::auth::default_bearer_prefix() }, "must not fabricate a token from the mask");
    }

    #[test]
    fn reimporting_already_masked_request_auth_clears_secret_and_warns() {
        let mut conn = crate::store::db::open_in_memory().unwrap();
        let ws = crate::store::workspaces::ensure_default(&mut conn).unwrap();

        let mut tree = sample_tree();
        tree.auth = AuthConfig::None;
        tree.requests[0].auth =
            RequestAuth::Own(AuthConfig::Bearer { token: crate::model::variable::SECRET_MASK.into(), prefix: crate::model::auth::default_bearer_prefix() });

        let report = apply_import(&conn, &ws.id, None, &tree, ConflictMode::Skip, ImportPlacement::AsSubfolder).unwrap();
        assert_eq!(report.warnings.len(), 1, "{:?}", report.warnings);
        assert!(report.warnings[0].contains("Get thing"), "{:?}", report.warnings);

        let roots = store::collections::list_children(&conn, &ws.id, None).unwrap();
        let reqs = store::requests::list_by_collection(&conn, &roots[0].id).unwrap();
        let stored = reqs.iter().find(|r| r.name == "Get thing").unwrap();
        assert_eq!(stored.auth, RequestAuth::Own(AuthConfig::Bearer { token: String::new(), prefix: crate::model::auth::default_bearer_prefix() }));
    }

    #[test]
    fn into_folder_placement_adds_requests_directly_to_target() {
        let mut conn = crate::store::db::open_in_memory().unwrap();
        let ws = crate::store::workspaces::ensure_default(&mut conn).unwrap();
        let target = store::collections::create(&conn, &ws.id, None, "API", None).unwrap();

        let curl_root = ImportedNode {
            name: "GET /users".into(),
            requests: vec![leaf_request("GET /users")],
            ..Default::default()
        };
        let report = apply_import(
            &conn,
            &ws.id,
            Some(&target.id),
            &curl_root,
            ConflictMode::Skip,
            ImportPlacement::IntoFolder,
        )
        .unwrap();

        assert_eq!(report.created_collections, 0);
        assert_eq!(report.created_requests, 1);
        let reqs = store::requests::list_by_collection(&conn, &target.id).unwrap();
        assert_eq!(reqs.len(), 1);
        assert_eq!(reqs[0].name, "GET /users");
        let children = store::collections::list_children(&conn, &ws.id, Some(&target.id)).unwrap();
        assert!(children.is_empty());
    }

    #[test]
    fn as_subfolder_placement_creates_nested_collection() {
        let mut conn = crate::store::db::open_in_memory().unwrap();
        let ws = crate::store::workspaces::ensure_default(&mut conn).unwrap();
        let target = store::collections::create(&conn, &ws.id, None, "API", None).unwrap();

        let curl_root = ImportedNode {
            name: "GET /users".into(),
            requests: vec![leaf_request("GET /users")],
            ..Default::default()
        };
        let report = apply_import(
            &conn,
            &ws.id,
            Some(&target.id),
            &curl_root,
            ConflictMode::Skip,
            ImportPlacement::AsSubfolder,
        )
        .unwrap();

        assert_eq!(report.created_collections, 1);
        assert_eq!(report.created_requests, 1);
        assert!(store::requests::list_by_collection(&conn, &target.id).unwrap().is_empty());
        let children = store::collections::list_children(&conn, &ws.id, Some(&target.id)).unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].name, "GET /users");
        let reqs = store::requests::list_by_collection(&conn, &children[0].id).unwrap();
        assert_eq!(reqs.len(), 1);
    }

    #[test]
    fn into_folder_placement_puts_subfolders_under_target() {
        let mut conn = crate::store::db::open_in_memory().unwrap();
        let ws = crate::store::workspaces::ensure_default(&mut conn).unwrap();
        let target = store::collections::create(&conn, &ws.id, None, "API", None).unwrap();

        let tree = ImportedNode {
            name: "Pet Store".into(),
            requests: vec![leaf_request("List pets")],
            children: vec![ImportedNode {
                name: "Users".into(),
                requests: vec![leaf_request("Get user")],
                ..Default::default()
            }],
            ..Default::default()
        };
        let report = apply_import(
            &conn,
            &ws.id,
            Some(&target.id),
            &tree,
            ConflictMode::Skip,
            ImportPlacement::IntoFolder,
        )
        .unwrap();

        assert_eq!(report.created_collections, 1); // Users subfolder only
        assert_eq!(report.created_requests, 2);
        let root_reqs = store::requests::list_by_collection(&conn, &target.id).unwrap();
        assert_eq!(root_reqs.len(), 1);
        assert_eq!(root_reqs[0].name, "List pets");
        let children = store::collections::list_children(&conn, &ws.id, Some(&target.id)).unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].name, "Users");
    }
}
