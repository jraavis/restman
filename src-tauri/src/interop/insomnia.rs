//! Insomnia (v4) import. An Insomnia export is a single JSON document with a
//! top-level `resources` array; each resource has `_id`, `parentId` (or
//! `"__WORKSPACE__"` for the root), `name`, `_type` (`workspace`/`request_group`/
//! `request`), and type-specific fields. We rebuild the tree from those
//! parent links, then map each `request` resource to an `ImportedRequest`.
//!
//! Export is intentionally NOT supported here — Insomnia's on-disk format is a
//! YAML file per workspace (a shape this app's `collect()` → text path doesn't
//! map onto cleanly), and `ExportFormat` has no Insomnia variant. Import-only,
//! mirroring how the original OpenAPI module started (import before export).
//!
//! Body: `body.text` (raw) or `body.params` (form) with an explicit `body.mimeType`.
//! Auth: a single `authentication` object with `type` (`bearer`/`basic`/`apikey`/
//! `oauth2`/`aws`). Secrets handled by the `apply_import` layer — see module doc.

use crate::error::{AppError, AppResult};
use crate::interop::{ImportPreview, ImportedNode, ImportedRequest};
use crate::model::auth::{
    ApiKeyLocation, AuthConfig, OAuth2Config, OAuth2GrantType, PkceMethod, RequestAuth,
};
use crate::model::http::{FormField, HeaderEntry, KeyValue, RequestBody, RequestOptions};
use serde_json::Value;

pub fn parse(content: &str) -> AppResult<ImportPreview> {
    let mut v: Value = serde_json::from_str(content).map_err(|e| AppError::Other(format!("invalid Insomnia export JSON: {e}")))?;
    // Some Insomnia exports nest under a top-level "data" key; others are bare
    // resource arrays. Accept either.
    if v.get("resources").is_none() && v.get("data").and_then(Value::as_array).is_some() {
        v = v.get("data").cloned().unwrap();
    }
    let arr = v.get("resources").and_then(Value::as_array).cloned().or_else(|| v.as_array().cloned()).ok_or_else(|| AppError::Other("not an Insomnia export: missing \"resources\" array".into()))?;
    let mut warnings = Vec::new();

    let workspace = arr
        .iter()
        .find(|r| r.get("_type").and_then(Value::as_str) == Some("workspace"))
        .ok_or_else(|| AppError::Other("Insomnia export has no workspace resource".into()))?;
    let root_name = workspace.get("name").and_then(Value::as_str).unwrap_or("Imported Insomnia").to_string();
    let root_desc = workspace.get("description").and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string);

    let root_auth = parse_auth(workspace.get("authentication"), &mut warnings);

    // Root children either point at the workspace's `_id` or have a missing/
    // null parentId (older exports). `is_root` absorbs both.
    let root_parent = workspace.get("_id").and_then(Value::as_str).unwrap_or("__WORKSPACE__").to_string();
    let root = build_node(&arr, &root_parent, true, root_name, root_desc, root_auth, &mut warnings);
    Ok(ImportPreview::new(root, warnings))
}

/// Rebuild one folder/request tree rooted at `parent_id`. `name`/`desc`/`auth`
/// come from the *parent* resource (the workspace itself for the root) —
/// they're passed in rather than re-read because the root's `_id` is
/// `__WORKSPACE__`, not a real by_id key.
///
/// Top-level requests whose `parentId` is missing or `null` are treated as
/// belonging to the workspace root — Insomnia exports sometimes omit the
/// field entirely for workspace-level children (older versions wrote
/// `parentId: null`, which JSON-serializes to `null`, not `"__WORKSPACE__"`).
/// `is_root` flags the root call so we can absorb those instead of skipping.
fn build_node(
    arr: &[Value],
    parent_id: &str,
    is_root: bool,
    name: String,
    desc: Option<String>,
    auth: AuthConfig,
    warnings: &mut Vec<String>,
) -> ImportedNode {
    let mut requests = Vec::new();
    let mut children = Vec::new();
    // Walk the original array in order so the import's request ordering
    // matches what the user authored (Insomnia's own `sort_order` field isn't
    // universally present; preserving array order is the saner default).
    for r in arr {
        let r_parent_field = r.get("parentId");
        let matches = match r_parent_field {
            None | Some(Value::Null) => is_root,
            Some(Value::String(s)) => s == parent_id,
            _ => false,
        };
        if !matches {
            continue;
        }
        match r.get("_type").and_then(Value::as_str) {
            Some("request_group") => {
                let gname = r.get("name").and_then(Value::as_str).unwrap_or("Untitled folder").to_string();
                let gdesc = r.get("description").and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string);
                let gauth = parse_auth(r.get("authentication"), warnings);
                let gid = r.get("_id").and_then(Value::as_str).unwrap_or("").to_string();
                children.push(build_node(arr, &gid, false, gname, gdesc, gauth, warnings));
            }
            Some("request") => {
                requests.push(parse_request(r, warnings));
            }
            _ => {}
        }
    }

    ImportedNode { name, description: desc, auth, requests, children }
}

fn parse_request(r: &Value, warnings: &mut Vec<String>) -> ImportedRequest {
    let name = r.get("name").and_then(Value::as_str).unwrap_or("Untitled request").to_string();
    let method = r.get("method").and_then(Value::as_str).unwrap_or("GET").to_uppercase();
    let (base_url, query, headers_no_query, _) = parse_url(r.get("url"));
    let mut headers: Vec<HeaderEntry> = r
        .get("headers")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|h| {
                    let name = h.get("name").and_then(Value::as_str)?.to_string();
                    if name.is_empty() {
                        return None;
                    }
                    let value = h.get("value").and_then(Value::as_str).unwrap_or_default().to_string();
                    let enabled = !h.get("disabled").and_then(Value::as_bool).unwrap_or(false);
                    Some(HeaderEntry { name, value, enabled })
                })
                .collect()
        })
        .unwrap_or_default();
    headers.extend(headers_no_query);

    let body = parse_body(r.get("body"), &headers, warnings);
    let auth = match r.get("authentication") {
        Some(a) => RequestAuth::Own(parse_auth(Some(a), warnings)),
        None => RequestAuth::Inherit,
    };

    ImportedRequest {
        name,
        method,
        url: base_url,
        headers,
        query,
        body,
        options: RequestOptions::default(),
        auth,
        pre_request_script: r.get("preRequestScript").and_then(Value::as_str).unwrap_or_default().to_string(),
        post_response_script: r.get("afterResponseScript").and_then(Value::as_str).unwrap_or_default().to_string(),
    }
}

/// Insomnia's `url` is an object `{raw, query: [{name, value, disabled}], ...}`;
/// we split the query out and return base + structured query.
fn parse_url(v: Option<&Value>) -> (String, Vec<KeyValue>, Vec<HeaderEntry>, Vec<KeyValue>) {
    let Some(v) = v else { return (String::new(), Vec::new(), Vec::new(), Vec::new()) };
    let raw = match v {
        Value::String(s) => s.clone(),
        Value::Object(_) => v.get("raw").and_then(Value::as_str).unwrap_or_default().to_string(),
        _ => String::new(),
    };
    let base = raw.split('?').next().unwrap_or(&raw).to_string();
    let query = v.get("query").and_then(Value::as_array).map(|list| {
        list.iter()
            .filter_map(|q| {
                let key = q.get("name").and_then(Value::as_str)?.to_string();
                let value = q.get("value").and_then(Value::as_str).unwrap_or_default().to_string();
                let enabled = !q.get("disabled").and_then(Value::as_bool).unwrap_or(false);
                Some(KeyValue { key, value, enabled })
            })
            .collect()
    }).unwrap_or_default();
    (base, query, Vec::new(), Vec::new())
}

fn parse_body(v: Option<&Value>, headers: &[HeaderEntry], warnings: &mut Vec<String>) -> RequestBody {
    let Some(body) = v else { return RequestBody::None };
    let mime = body.get("mimeType").and_then(Value::as_str).unwrap_or_default().to_ascii_lowercase();
    if mime.is_empty() {
        return RequestBody::None;
    }
    if mime.contains("json") {
        return RequestBody::Json(body.get("text").and_then(Value::as_str).unwrap_or_default().to_string());
    }
    if mime.contains("x-www-form-urlencoded") {
        return RequestBody::UrlEncoded(
            body.get("params")
                .and_then(Value::as_array)
                .map(|list| {
                    list.iter()
                        .filter_map(|p| {
                            let key = p.get("name").and_then(Value::as_str)?.to_string();
                            let value = p.get("value").and_then(Value::as_str).unwrap_or_default().to_string();
                            let enabled = !p.get("disabled").and_then(Value::as_bool).unwrap_or(false);
                            Some(KeyValue { key, value, enabled })
                        })
                        .collect()
                })
                .unwrap_or_else(|| {
                    let text = body.get("text").and_then(Value::as_str).unwrap_or_default();
                    text.split('&').filter(|s| !s.is_empty()).map(|pair| match pair.split_once('=') {
                        Some((k, v)) => KeyValue { key: k.to_string(), value: v.to_string(), enabled: true },
                        None => KeyValue { key: pair.to_string(), value: String::new(), enabled: true },
                    }).collect()
                }),
        );
    }
    if mime.contains("multipart/form-data") {
        return RequestBody::FormData(
            body.get("params")
                .and_then(Value::as_array)
                .map(|list| {
                    list.iter()
                        .filter_map(|p| {
                            let key = p.get("name").and_then(Value::as_str)?.to_string();
                            let value = p.get("value").and_then(Value::as_str).unwrap_or_default().to_string();
                            let is_file = p.get("type").and_then(Value::as_str) == Some("file");
                            let enabled = !p.get("disabled").and_then(Value::as_bool).unwrap_or(false);
                            let content_type = p.get("contentType").and_then(Value::as_str).map(str::to_string);
                            Some(FormField { key, value: if is_file { String::new() } else { value }, enabled, is_file, content_type })
                        })
                        .collect()
                })
                .unwrap_or_default(),
        );
    }
    if mime.contains("graphql") {
        let text = body.get("text").and_then(Value::as_str).unwrap_or_default();
        let parsed: Value = serde_json::from_str(text).unwrap_or(Value::Null);
        let query = parsed.get("query").and_then(Value::as_str).unwrap_or_default().to_string();
        let variables = parsed.get("variables").map(|v| match v {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        });
        let operation_name = parsed.get("operationName").and_then(Value::as_str).map(str::to_string);
        return RequestBody::Graphql { query, variables, operation_name };
    }
    if let Some(text) = body.get("text").and_then(Value::as_str) {
        let language = mime.split('/').nth(1).map(str::to_string);
        return RequestBody::Raw { content: text.to_string(), language };
    }
    let _ = headers;
    warnings.push(format!("Insomnia body with mimeType \"{mime}\" has no recognizable shape — imported with an empty body"));
    RequestBody::None
}

fn parse_auth(v: Option<&Value>, warnings: &mut Vec<String>) -> AuthConfig {
    let Some(a) = v else { return AuthConfig::None };
    if a.is_null() {
        return AuthConfig::None;
    }
    match a.get("type").and_then(Value::as_str).unwrap_or("none") {
        "none" | "" => AuthConfig::None,
        "bearer" => AuthConfig::Bearer { token: a.get("token").and_then(Value::as_str).unwrap_or_default().to_string() },
        "basic" => AuthConfig::Basic {
            username: a.get("username").and_then(Value::as_str).unwrap_or_default().to_string(),
            password: a.get("password").and_then(Value::as_str).unwrap_or_default().to_string(),
        },
        "apikey" => AuthConfig::ApiKey {
            key: a.get("key").and_then(Value::as_str).unwrap_or_default().to_string(),
            value: a.get("value").and_then(Value::as_str).unwrap_or_default().to_string(),
            location: if a.get("disabled").and_then(Value::as_bool).unwrap_or(false) {
                ApiKeyLocation::Query
            } else {
                // Insomnia uses a single `in` field, but older exports use a
                // disabled/`prefix`/disabled flag — default to header.
                if a.get("in").and_then(Value::as_str) == Some("query") { ApiKeyLocation::Query } else { ApiKeyLocation::Header }
            },
        },
        "oauth2" => AuthConfig::OAuth2(OAuth2Config {
            grant_type: match a.get("grantType").and_then(Value::as_str).unwrap_or("client_credentials") {
                "authorization_code" => OAuth2GrantType::AuthorizationCode,
                "password" => OAuth2GrantType::Password,
                "refresh_token" => OAuth2GrantType::RefreshToken,
                _ => OAuth2GrantType::ClientCredentials,
            },
            auth_url: a.get("authorizationUrl").and_then(Value::as_str).unwrap_or_default().to_string(),
            token_url: a.get("accessTokenUrl").and_then(Value::as_str).unwrap_or_default().to_string(),
            client_id: a.get("clientId").and_then(Value::as_str).unwrap_or_default().to_string(),
            client_secret: a.get("clientSecret").and_then(Value::as_str).unwrap_or_default().to_string(),
            scope: a.get("scope").or_else(|| a.get("scopes")).and_then(Value::as_str).unwrap_or_default().to_string(),
            redirect_uri: a.get("redirectUri").or_else(|| a.get("redirectUrl")).and_then(Value::as_str).unwrap_or_default().to_string(),
            pkce: if a.get("pkce").and_then(Value::as_bool).unwrap_or(false) { PkceMethod::S256 } else { PkceMethod::None },
            username: a.get("username").and_then(Value::as_str).unwrap_or_default().to_string(),
            password: a.get("password").and_then(Value::as_str).unwrap_or_default().to_string(),
            refresh_token: a.get("refreshToken").and_then(Value::as_str).unwrap_or_default().to_string(),
        }),
        "aws" => AuthConfig::AwsSigV4(crate::model::auth::AwsSigV4Config {
            access_key: a.get("accessKeyId").and_then(Value::as_str).unwrap_or_default().to_string(),
            secret_key: a.get("secretAccessKey").and_then(Value::as_str).unwrap_or_default().to_string(),
            region: a.get("region").and_then(Value::as_str).unwrap_or_default().to_string(),
            service: a.get("service").and_then(Value::as_str).unwrap_or_default().to_string(),
            session_token: a.get("sessionToken").and_then(Value::as_str).unwrap_or_default().to_string(),
        }),
        other => {
            warnings.push(format!("unsupported Insomnia auth type \"{other}\" — imported as No Auth"));
            AuthConfig::None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INSOMNIA_FIXTURE: &str = r#"{
        "resources": [
            {
                "_id": "ws_1",
                "_type": "workspace",
                "name": "Petstore",
                "description": "Sample API",
                "parentId": null,
                "authentication": {"type": "bearer", "token": "ws-bearer-token"}
            },
            {
                "_id": "grp_1",
                "_type": "request_group",
                "name": "Pets",
                "parentId": "ws_1"
            },
            {
                "_id": "req_1",
                "_type": "request",
                "name": "Get pet",
                "method": "GET",
                "url": {"raw": "https://petstore.example.com/pets/123?verbose=true", "query": [{"name": "verbose", "value": "true", "disabled": false}]},
                "headers": [{"name": "Accept", "value": "application/json"}],
                "authentication": {"type": "basic", "username": "alice", "password": "secret"}
            },
            {
                "_id": "req_2",
                "_type": "request",
                "name": "Create pet",
                "method": "POST",
                "url": {"raw": "https://petstore.example.com/pets"},
                "body": {"mimeType": "application/json", "text": "{\"name\":\"Fido\"}"},
                "parentId": "grp_1"
            },
            {
                "_id": "req_3",
                "_type": "request",
                "name": "Login",
                "method": "POST",
                "url": {"raw": "https://petstore.example.com/login"},
                "body": {"mimeType": "application/x-www-form-urlencoded", "params": [{"name": "user", "value": "alice"}]},
                "authentication": {"type": "oauth2", "grantType": "password", "accessTokenUrl": "https://petstore.example.com/token", "username": "alice", "password": "secret"}
            }
        ]
    }"#;

    #[test]
    fn imports_realistic_insomnia_fixture_with_expected_shape() {
        let preview = parse(INSOMNIA_FIXTURE).unwrap();
        assert_eq!(preview.root.name, "Petstore");
        assert_eq!(preview.root.description, Some("Sample API".to_string()));
        assert_eq!(preview.root.auth, AuthConfig::Bearer { token: "ws-bearer-token".into() });
        assert_eq!(preview.root.children.len(), 1);
        assert_eq!(preview.root.children[0].name, "Pets");
        assert_eq!(preview.root.requests.len(), 2);
        assert_eq!(preview.root.children[0].requests.len(), 1);

        let get_pet = preview.root.requests.iter().find(|r| r.name == "Get pet").unwrap();
        assert_eq!(get_pet.method, "GET");
        assert_eq!(get_pet.url, "https://petstore.example.com/pets/123");
        assert_eq!(get_pet.query, vec![KeyValue { key: "verbose".into(), value: "true".into(), enabled: true }]);
        assert_eq!(get_pet.auth, RequestAuth::Own(AuthConfig::Basic { username: "alice".into(), password: "secret".into() }));
        assert!(get_pet.headers.iter().any(|h| h.name == "Accept"));

        let create_pet = &preview.root.children[0].requests[0];
        assert_eq!(create_pet.method, "POST");
        assert_eq!(create_pet.auth, RequestAuth::Inherit);
        assert_eq!(create_pet.body, RequestBody::Json("{\"name\":\"Fido\"}".into()));

        let login = preview.root.requests.iter().find(|r| r.name == "Login").unwrap();
        assert_eq!(login.method, "POST");
        match &login.body {
            RequestBody::UrlEncoded(kv) => assert_eq!(kv, &[KeyValue { key: "user".into(), value: "alice".into(), enabled: true }]),
            other => panic!("expected UrlEncoded body, got {other:?}"),
        }
        match &login.auth {
            RequestAuth::Own(AuthConfig::OAuth2(c)) => {
                assert_eq!(c.grant_type, OAuth2GrantType::Password);
                assert_eq!(c.token_url, "https://petstore.example.com/token");
            }
            other => panic!("expected OAuth2 Own auth, got {other:?}"),
        }

        assert_eq!(preview.stats.requests, 3);
        assert_eq!(preview.stats.folders, 1);
    }

    #[test]
    fn rejects_non_insomnia_json() {
        let err = parse(r#"{"foo": "bar"}"#).unwrap_err();
        assert!(err.to_string().contains("resources"));
    }
}
