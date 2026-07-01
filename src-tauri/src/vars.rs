//! `{{var}}` resolution and interpolation. Resolution priority (lowest to
//! highest, later entries win on key collision): global → workspace →
//! collection → environment. A "local" tier (script-set, highest priority)
//! arrives with Phase 4's scripting sandbox — `resolve` takes no input for it
//! yet since nothing produces local vars until then.

use crate::error::AppResult;
use crate::model::http::{HttpRequest, RequestBody};
use crate::model::{VarScope, Variable, SECRET_MASK};
use crate::store::{environments, variables};
use rusqlite::Connection;
use std::collections::{HashMap, HashSet};

/// Resolved variables for one send, plus which resolved values came from a
/// secret variable. `secrets` holds plaintext values (not keys) so callers
/// can redact them out of anything derived from the interpolated request —
/// see `redact_request`.
#[derive(Debug, Default)]
pub struct Resolved {
    pub values: HashMap<String, String>,
    pub secrets: HashSet<String>,
}

/// Merge all scopes that apply to a request being sent from `collection_id`
/// (if it lives in one) under `workspace_id`, using the workspace's active
/// environment if one is set.
pub fn resolve(conn: &Connection, workspace_id: &str, collection_id: Option<&str>) -> AppResult<Resolved> {
    let mut resolved = Resolved::default();
    merge(&mut resolved, variables::list(conn, &VarScope::Global)?);
    merge(&mut resolved, variables::list(conn, &VarScope::Workspace(workspace_id.to_string()))?);
    if let Some(cid) = collection_id {
        merge(&mut resolved, variables::list(conn, &VarScope::Collection(cid.to_string()))?);
    }
    if let Some(env) = environments::active_for_workspace(conn, workspace_id)? {
        // A collection-scoped env only applies when sending from that exact
        // collection — otherwise an active env scoped to collection A would
        // leak into sends from collection B (or no collection at all). A
        // workspace-global env (collection_id None) always applies.
        let applies = match env.collection_id.as_deref() {
            None => true,
            Some(env_cid) => collection_id == Some(env_cid),
        };
        if applies {
            merge(&mut resolved, variables::list(conn, &VarScope::Environment(env.id))?);
        }
    }
    Ok(resolved)
}

fn merge(resolved: &mut Resolved, list: Vec<Variable>) {
    for v in list {
        if !v.enabled {
            continue;
        }
        if v.is_secret {
            // The column is always empty for a secret row (see
            // `store::variables`) — the real value lives in the OS
            // keychain, keyed by variable id. A read failure (keychain
            // locked/unavailable) degrades to an empty value rather than
            // failing the whole resolve, so one unrelated unreadable
            // secret can't block every send that doesn't even use it; the
            // `get_secret_backend_status` command covers the up-front
            // "keychain unavailable" warning instead.
            let secret = crate::secrets::get(&variables::keychain_key(&v.id)).ok().flatten().unwrap_or_default();
            // Guard empty values: an empty string as a "secret" would
            // otherwise become a redact needle that matches (and mangles)
            // everything.
            if !secret.is_empty() {
                resolved.secrets.insert(secret.clone());
            }
            resolved.values.insert(v.key, secret);
            continue;
        }
        resolved.values.insert(v.key, v.value);
    }
}

/// Replace every `{{key}}` in `text` with its resolved value. A key with no
/// match in `vars` is left as literal `{{key}}` text (visible, not silently
/// blanked) and an unterminated `{{` is emitted as-is.
pub fn interpolate(text: &str, vars: &HashMap<String, String>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    loop {
        match rest.find("{{") {
            None => {
                out.push_str(rest);
                break;
            }
            Some(start) => {
                out.push_str(&rest[..start]);
                let after_open = &rest[start + 2..];
                match after_open.find("}}") {
                    None => {
                        out.push_str(&rest[start..]);
                        break;
                    }
                    Some(end) => {
                        let key = after_open[..end].trim();
                        match vars.get(key) {
                            Some(value) => out.push_str(value),
                            None => {
                                out.push_str("{{");
                                out.push_str(&after_open[..end]);
                                out.push_str("}}");
                            }
                        }
                        rest = &after_open[end + 2..];
                    }
                }
            }
        }
    }
    out
}

/// Apply `interpolate` across every text field of a request: URL, headers,
/// query params, and body (all modes except `Binary`'s file path).
pub fn interpolate_request(req: &mut HttpRequest, vars: &HashMap<String, String>) {
    req.url = interpolate(&req.url, vars);
    for h in &mut req.headers {
        h.name = interpolate(&h.name, vars);
        h.value = interpolate(&h.value, vars);
    }
    for q in &mut req.query {
        q.key = interpolate(&q.key, vars);
        q.value = interpolate(&q.value, vars);
    }
    match &mut req.body {
        RequestBody::None | RequestBody::Binary { .. } => {}
        RequestBody::Json(s) => *s = interpolate(s, vars),
        RequestBody::Raw { content, .. } => *content = interpolate(content, vars),
        RequestBody::UrlEncoded(kvs) => {
            for kv in kvs {
                kv.key = interpolate(&kv.key, vars);
                kv.value = interpolate(&kv.value, vars);
            }
        }
        RequestBody::FormData(fields) => {
            for f in fields {
                f.key = interpolate(&f.key, vars);
                if !f.is_file {
                    f.value = interpolate(&f.value, vars);
                }
            }
        }
        RequestBody::Graphql { query, variables, operation_name: _ } => {
            *query = interpolate(query, vars);
            if let Some(v) = variables {
                *v = interpolate(v, vars);
            }
        }
    }
}

/// Replace every occurrence of a resolved secret value with a fixed mask.
/// Used only for the copy written to history — never on the request that's
/// actually sent over the wire.
pub fn redact(text: &str, secrets: &HashSet<String>) -> String {
    let mut out = text.to_string();
    for s in secrets {
        out = out.replace(s.as_str(), SECRET_MASK);
    }
    out
}

/// Redact `secrets` out of a copy of `req`. Applied to the already-resolved
/// request right before it's written to history, so a token interpolated
/// into e.g. an `Authorization` header is never persisted in plaintext.
///
/// Known limitation: once redacted, the real value is gone from this copy
/// for good, so replaying a history entry whose original send used a secret
/// resends the mask text, not the secret — an accepted trade-off for never
/// writing secrets to disk. See FEATURES.md's "redacted logs" requirement.
pub fn redact_request(req: &HttpRequest, secrets: &HashSet<String>) -> HttpRequest {
    let mut out = req.clone();
    if secrets.is_empty() {
        return out;
    }
    out.url = redact(&out.url, secrets);
    for h in &mut out.headers {
        h.value = redact(&h.value, secrets);
    }
    for q in &mut out.query {
        q.value = redact(&q.value, secrets);
    }
    match &mut out.body {
        RequestBody::None | RequestBody::Binary { .. } => {}
        RequestBody::Json(s) => *s = redact(s, secrets),
        RequestBody::Raw { content, .. } => *content = redact(content, secrets),
        RequestBody::UrlEncoded(kvs) => {
            for kv in kvs {
                kv.value = redact(&kv.value, secrets);
            }
        }
        RequestBody::FormData(fields) => {
            for f in fields {
                if !f.is_file {
                    f.value = redact(&f.value, secrets);
                }
            }
        }
        RequestBody::Graphql { query, variables, operation_name: _ } => {
            *query = redact(query, secrets);
            if let Some(v) = variables {
                *v = redact(v, secrets);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpolate_replaces_known_vars_and_leaves_unknown_literal() {
        let mut vars = HashMap::new();
        vars.insert("host".to_string(), "api.example.com".to_string());
        assert_eq!(
            interpolate("https://{{host}}/users/{{id}}", &vars),
            "https://api.example.com/users/{{id}}"
        );
    }

    #[test]
    fn interpolate_tolerates_unterminated_braces() {
        let vars = HashMap::new();
        assert_eq!(interpolate("no close {{here", &vars), "no close {{here");
    }

    #[test]
    fn interpolate_trims_whitespace_inside_braces() {
        let mut vars = HashMap::new();
        vars.insert("x".to_string(), "1".to_string());
        assert_eq!(interpolate("{{ x }}", &vars), "1");
    }

    #[test]
    fn resolve_priority_environment_overrides_workspace_overrides_global() {
        let mut conn = crate::store::db::open_in_memory().unwrap();
        let ws = crate::store::workspaces::ensure_default(&mut conn).unwrap();

        variables::create(
            &conn,
            &VarScope::Global,
            &crate::model::VariableInput {
                key: "level".into(),
                value: "global".into(),
                var_type: crate::model::VarType::String,
                is_secret: false,
                enabled: true,
            },
        )
        .unwrap();
        variables::create(
            &conn,
            &VarScope::Workspace(ws.id.clone()),
            &crate::model::VariableInput {
                key: "level".into(),
                value: "workspace".into(),
                var_type: crate::model::VarType::String,
                is_secret: false,
                enabled: true,
            },
        )
        .unwrap();

        let resolved = resolve(&conn, &ws.id, None).unwrap();
        assert_eq!(resolved.values.get("level").unwrap(), "workspace");

        let env = environments::create(&conn, &ws.id, None, "Dev", None).unwrap();
        variables::create(
            &conn,
            &VarScope::Environment(env.id.clone()),
            &crate::model::VariableInput {
                key: "level".into(),
                value: "environment".into(),
                var_type: crate::model::VarType::String,
                is_secret: false,
                enabled: true,
            },
        )
        .unwrap();
        environments::set_active(&mut conn, &ws.id, Some(&env.id)).unwrap();

        let resolved = resolve(&conn, &ws.id, None).unwrap();
        assert_eq!(resolved.values.get("level").unwrap(), "environment");
    }

    #[test]
    fn disabled_variables_are_excluded() {
        let mut conn = crate::store::db::open_in_memory().unwrap();
        let ws = crate::store::workspaces::ensure_default(&mut conn).unwrap();
        variables::create(
            &conn,
            &VarScope::Workspace(ws.id.clone()),
            &crate::model::VariableInput {
                key: "off".into(),
                value: "nope".into(),
                var_type: crate::model::VarType::String,
                is_secret: false,
                enabled: false,
            },
        )
        .unwrap();
        let resolved = resolve(&conn, &ws.id, None).unwrap();
        assert!(!resolved.values.contains_key("off"));
    }

    #[test]
    fn resolve_collects_secret_values_for_redaction() {
        let mut conn = crate::store::db::open_in_memory().unwrap();
        let ws = crate::store::workspaces::ensure_default(&mut conn).unwrap();
        variables::create(
            &conn,
            &VarScope::Workspace(ws.id.clone()),
            &crate::model::VariableInput {
                key: "token".into(),
                value: "tok_abc123".into(),
                var_type: crate::model::VarType::String,
                is_secret: true,
                enabled: true,
            },
        )
        .unwrap();
        let resolved = resolve(&conn, &ws.id, None).unwrap();
        assert!(resolved.secrets.contains("tok_abc123"));
    }

    #[test]
    fn redact_request_masks_secret_values_in_url_and_headers() {
        let mut req = HttpRequest {
            url: "https://api.example.com/users?key=tok_abc123".into(),
            ..Default::default()
        };
        req.headers.push(crate::model::http::HeaderEntry {
            name: "Authorization".into(),
            value: "Bearer tok_abc123".into(),
            enabled: true,
        });
        let mut secrets = HashSet::new();
        secrets.insert("tok_abc123".to_string());

        let redacted = redact_request(&req, &secrets);
        assert!(!redacted.url.contains("tok_abc123"));
        assert!(!redacted.headers[0].value.contains("tok_abc123"));
        assert_eq!(redacted.headers[0].value, format!("Bearer {SECRET_MASK}"));

        // The real request handed to the network layer is untouched.
        assert!(req.url.contains("tok_abc123"));
    }

    #[test]
    fn redact_request_is_noop_with_no_secrets() {
        let req = HttpRequest {
            url: "https://api.example.com".into(),
            ..Default::default()
        };
        let redacted = redact_request(&req, &HashSet::new());
        assert_eq!(redacted.url, req.url);
    }

    #[test]
    fn active_collection_scoped_env_does_not_leak_to_other_collections() {
        let mut conn = crate::store::db::open_in_memory().unwrap();
        let ws = crate::store::workspaces::ensure_default(&mut conn).unwrap();
        let collection_a = crate::store::collections::create(&conn, &ws.id, None, "A", None).unwrap();
        let collection_b = crate::store::collections::create(&conn, &ws.id, None, "B", None).unwrap();

        let env = environments::create(&conn, &ws.id, Some(&collection_a.id), "A-only", None).unwrap();
        variables::create(
            &conn,
            &VarScope::Environment(env.id.clone()),
            &crate::model::VariableInput {
                key: "scoped".into(),
                value: "a-value".into(),
                var_type: crate::model::VarType::String,
                is_secret: false,
                enabled: true,
            },
        )
        .unwrap();
        environments::set_active(&mut conn, &ws.id, Some(&env.id)).unwrap();

        let in_a = resolve(&conn, &ws.id, Some(&collection_a.id)).unwrap();
        assert_eq!(in_a.values.get("scoped").unwrap(), "a-value");

        let in_b = resolve(&conn, &ws.id, Some(&collection_b.id)).unwrap();
        assert!(!in_b.values.contains_key("scoped"));

        let unscoped = resolve(&conn, &ws.id, None).unwrap();
        assert!(!unscoped.values.contains_key("scoped"));
    }

    #[test]
    fn workspace_global_active_env_applies_regardless_of_collection() {
        let mut conn = crate::store::db::open_in_memory().unwrap();
        let ws = crate::store::workspaces::ensure_default(&mut conn).unwrap();
        let collection = crate::store::collections::create(&conn, &ws.id, None, "C", None).unwrap();

        let env = environments::create(&conn, &ws.id, None, "Global env", None).unwrap();
        variables::create(
            &conn,
            &VarScope::Environment(env.id.clone()),
            &crate::model::VariableInput {
                key: "g".into(),
                value: "g-value".into(),
                var_type: crate::model::VarType::String,
                is_secret: false,
                enabled: true,
            },
        )
        .unwrap();
        environments::set_active(&mut conn, &ws.id, Some(&env.id)).unwrap();

        let in_collection = resolve(&conn, &ws.id, Some(&collection.id)).unwrap();
        assert_eq!(in_collection.values.get("g").unwrap(), "g-value");

        let no_collection = resolve(&conn, &ws.id, None).unwrap();
        assert_eq!(no_collection.values.get("g").unwrap(), "g-value");
    }
}
