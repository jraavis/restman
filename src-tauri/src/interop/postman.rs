//! Postman Collection v2.1 import/export. Parses into and serializes out of
//! the shared `interop` IR — see module doc there for the secret-handling
//! contract this must respect.
//!
//! Parsing works off `serde_json::Value` rather than typed structs: Postman's
//! `url`/`body`/`auth` shapes are too variant-heavy (string-or-object, a
//! different field set per auth/body "type" tag) for straightforward serde
//! derives, and a `Value` walk lets one missing/extra field degrade to a
//! warning instead of a hard parse failure — see PLAN.md's "partial import"
//! requirement.

use crate::error::{AppError, AppResult};
use crate::interop::{ImportPreview, ImportedNode, ImportedRequest};
use crate::model::auth::{
    ApiKeyLocation, AuthConfig, AwsSigV4Config, OAuth2Config, OAuth2GrantType, PkceMethod, RequestAuth,
};
use crate::model::http::{FormField, HeaderEntry, KeyValue, RequestBody, RequestOptions};
use serde_json::{json, Value};

const SCHEMA_URL: &str = "https://schema.getpostman.com/json/collection/v2.1.0/collection.json";

pub fn parse(content: &str) -> AppResult<ImportPreview> {
    let v: Value = serde_json::from_str(content)
        .map_err(|e| AppError::Other(format!("invalid Postman collection JSON: {e}")))?;
    let info = v
        .get("info")
        .ok_or_else(|| AppError::Other("not a Postman collection: missing \"info\" object".into()))?;
    let name = info.get("name").and_then(Value::as_str).unwrap_or("Imported collection").to_string();
    let description = info.get("description").and_then(description_to_string);

    let mut warnings = Vec::new();
    let auth = v.get("auth").map(|a| parse_auth(a, &mut warnings)).unwrap_or(AuthConfig::None);
    if v.get("variable").and_then(Value::as_array).is_some_and(|a| !a.is_empty()) {
        warnings.push(
            "collection-level variables are not imported — re-create them via Environment import".into(),
        );
    }

    let items = v.get("item").and_then(Value::as_array).cloned().unwrap_or_default();
    let (requests, children) = parse_items(&items, &mut warnings);
    let root = ImportedNode { name, description, auth, requests, children };
    Ok(ImportPreview::new(root, warnings))
}

pub fn export(node: &ImportedNode) -> AppResult<String> {
    Ok(serde_json::to_string_pretty(&build_collection_json(node))?)
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

fn parse_items(items: &[Value], warnings: &mut Vec<String>) -> (Vec<ImportedRequest>, Vec<ImportedNode>) {
    let mut requests = Vec::new();
    let mut children = Vec::new();
    for item in items {
        if item.get("request").is_some() {
            requests.push(parse_request_item(item, warnings));
        } else {
            let name = item.get("name").and_then(Value::as_str).unwrap_or("Untitled folder").to_string();
            let description = item.get("description").and_then(description_to_string);
            let auth = item.get("auth").map(|a| parse_auth(a, warnings)).unwrap_or(AuthConfig::None);
            let sub_items = item.get("item").and_then(Value::as_array).cloned().unwrap_or_default();
            let (sub_requests, sub_children) = parse_items(&sub_items, warnings);
            children.push(ImportedNode { name, description, auth, requests: sub_requests, children: sub_children });
        }
    }
    (requests, children)
}

fn parse_request_item(item: &Value, warnings: &mut Vec<String>) -> ImportedRequest {
    let name = item.get("name").and_then(Value::as_str).unwrap_or("Untitled request").to_string();
    let req = item.get("request").cloned().unwrap_or(Value::Null);
    let method = req.get("method").and_then(Value::as_str).unwrap_or("GET").to_uppercase();
    let headers = parse_kv_list(req.get("header")).into_iter().map(|(name, value, enabled)| HeaderEntry { name, value, enabled }).collect();
    let (url, query) = parse_url(req.get("url"));
    let body = parse_body(req.get("body"), warnings);
    let auth = match req.get("auth") {
        Some(a) => RequestAuth::Own(parse_auth(a, warnings)),
        None => RequestAuth::Inherit,
    };
    let (pre_request_script, post_response_script) = parse_events(item.get("event"));
    ImportedRequest {
        name,
        method,
        url,
        headers,
        query,
        body,
        options: RequestOptions::default(),
        auth,
        pre_request_script,
        post_response_script,
    }
}

fn description_to_string(v: &Value) -> Option<String> {
    let s = match v {
        Value::String(s) => s.clone(),
        Value::Object(_) => v.get("content").and_then(Value::as_str)?.to_string(),
        _ => return None,
    };
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Shared shape for Postman's `header`/`urlencoded`/query-array entries:
/// `{key, value, disabled}`. Returns `(key, value, enabled)` triples.
fn parse_kv_list(v: Option<&Value>) -> Vec<(String, String, bool)> {
    v.and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|e| {
                    let key = e.get("key").and_then(Value::as_str)?.to_string();
                    let value = e.get("value").and_then(Value::as_str).unwrap_or_default().to_string();
                    let enabled = !e.get("disabled").and_then(Value::as_bool).unwrap_or(false);
                    Some((key, value, enabled))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_url(v: Option<&Value>) -> (String, Vec<KeyValue>) {
    let Some(v) = v else { return (String::new(), Vec::new()) };
    let raw = match v {
        Value::String(s) => s.clone(),
        Value::Object(_) => v.get("raw").and_then(Value::as_str).unwrap_or_default().to_string(),
        _ => String::new(),
    };
    let base = raw.split('?').next().unwrap_or(&raw).to_string();

    // Prefer the structured `query` array when present — unlike `raw`, it
    // also carries disabled params (Postman omits those from `raw`
    // entirely). Only fall back to splitting `raw` by hand for a bare
    // string `url` with no structured breakdown at all.
    let query = match v.get("query").and_then(Value::as_array) {
        Some(_) => parse_kv_list(v.get("query"))
            .into_iter()
            .map(|(key, value, enabled)| KeyValue { key, value, enabled })
            .collect(),
        None => raw
            .split_once('?')
            .map(|(_, qs)| {
                qs.split('&')
                    .filter(|s| !s.is_empty())
                    .map(|pair| match pair.split_once('=') {
                        Some((k, val)) => KeyValue { key: k.to_string(), value: val.to_string(), enabled: true },
                        None => KeyValue { key: pair.to_string(), value: String::new(), enabled: true },
                    })
                    .collect()
            })
            .unwrap_or_default(),
    };
    (base, query)
}

fn parse_body(v: Option<&Value>, warnings: &mut Vec<String>) -> RequestBody {
    let Some(v) = v else { return RequestBody::None };
    let mode = v.get("mode").and_then(Value::as_str).unwrap_or("");
    match mode {
        "raw" => {
            let content = v.get("raw").and_then(Value::as_str).unwrap_or_default().to_string();
            let language =
                v.get("options").and_then(|o| o.get("raw")).and_then(|r| r.get("language")).and_then(Value::as_str);
            match language {
                Some("json") => RequestBody::Json(content),
                Some(lang) => RequestBody::Raw { content, language: Some(lang.to_string()) },
                None => RequestBody::Raw { content, language: None },
            }
        }
        "urlencoded" => RequestBody::UrlEncoded(
            parse_kv_list(v.get("urlencoded"))
                .into_iter()
                .map(|(key, value, enabled)| KeyValue { key, value, enabled })
                .collect(),
        ),
        "formdata" => RequestBody::FormData(parse_form_array(v.get("formdata"))),
        "graphql" => {
            let g = v.get("graphql");
            let query = g.and_then(|g| g.get("query")).and_then(Value::as_str).unwrap_or_default().to_string();
            let variables = g.and_then(|g| g.get("variables")).and_then(Value::as_str).map(str::to_string);
            RequestBody::Graphql { query, variables }
        }
        "file" => RequestBody::Binary {
            path: v.get("file").and_then(|f| f.get("src")).and_then(Value::as_str).unwrap_or_default().to_string(),
        },
        "" => RequestBody::None,
        other => {
            warnings.push(format!("unsupported body mode \"{other}\" — imported with an empty body"));
            RequestBody::None
        }
    }
}

fn parse_form_array(v: Option<&Value>) -> Vec<FormField> {
    v.and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|e| {
                    let key = e.get("key").and_then(Value::as_str)?.to_string();
                    let is_file = e.get("type").and_then(Value::as_str) == Some("file");
                    let value = if is_file {
                        e.get("src").and_then(Value::as_str).unwrap_or_default().to_string()
                    } else {
                        e.get("value").and_then(Value::as_str).unwrap_or_default().to_string()
                    };
                    let enabled = !e.get("disabled").and_then(Value::as_bool).unwrap_or(false);
                    let content_type = e.get("contentType").and_then(Value::as_str).map(str::to_string);
                    Some(FormField { key, value, enabled, is_file, content_type })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_events(v: Option<&Value>) -> (String, String) {
    let Some(list) = v.and_then(Value::as_array) else { return (String::new(), String::new()) };
    let mut pre = String::new();
    let mut post = String::new();
    for e in list {
        let listen = e.get("listen").and_then(Value::as_str).unwrap_or_default();
        let script = e
            .get("script")
            .and_then(|s| s.get("exec"))
            .and_then(Value::as_array)
            .map(|lines| lines.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("\n"))
            .unwrap_or_default();
        match listen {
            "prerequest" => pre = script,
            "test" => post = script,
            _ => {}
        }
    }
    (pre, post)
}

fn auth_param(a: &Value, group: &str, key: &str) -> String {
    a.get(group)
        .and_then(Value::as_array)
        .and_then(|list| list.iter().find(|p| p.get("key").and_then(Value::as_str) == Some(key)))
        .and_then(|p| p.get("value"))
        .map(value_as_string)
        .unwrap_or_default()
}

fn value_as_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

fn parse_auth(a: &Value, warnings: &mut Vec<String>) -> AuthConfig {
    let ty = a.get("type").and_then(Value::as_str).unwrap_or("noauth");
    match ty {
        "noauth" => AuthConfig::None,
        "bearer" => AuthConfig::Bearer { token: auth_param(a, "bearer", "token") },
        "basic" => {
            AuthConfig::Basic { username: auth_param(a, "basic", "username"), password: auth_param(a, "basic", "password") }
        }
        "apikey" => AuthConfig::ApiKey {
            key: auth_param(a, "apikey", "key"),
            value: auth_param(a, "apikey", "value"),
            location: if auth_param(a, "apikey", "in") == "query" { ApiKeyLocation::Query } else { ApiKeyLocation::Header },
        },
        "oauth2" => AuthConfig::OAuth2(OAuth2Config {
            grant_type: parse_grant_type(&auth_param(a, "oauth2", "grant_type"), warnings),
            auth_url: auth_param(a, "oauth2", "authUrl"),
            token_url: auth_param(a, "oauth2", "accessTokenUrl"),
            client_id: auth_param(a, "oauth2", "clientId"),
            client_secret: auth_param(a, "oauth2", "clientSecret"),
            scope: auth_param(a, "oauth2", "scope"),
            redirect_uri: auth_param(a, "oauth2", "redirect_uri"),
            pkce: PkceMethod::None,
            username: auth_param(a, "oauth2", "username"),
            password: auth_param(a, "oauth2", "password"),
            refresh_token: auth_param(a, "oauth2", "refreshToken"),
        }),
        "awsv4" => AuthConfig::AwsSigV4(AwsSigV4Config {
            access_key: auth_param(a, "awsv4", "accessKey"),
            secret_key: auth_param(a, "awsv4", "secretKey"),
            region: auth_param(a, "awsv4", "region"),
            service: auth_param(a, "awsv4", "service"),
            session_token: auth_param(a, "awsv4", "sessionToken"),
        }),
        other => {
            warnings.push(format!("unsupported auth type \"{other}\" — imported as No Auth"));
            AuthConfig::None
        }
    }
}

fn parse_grant_type(raw: &str, warnings: &mut Vec<String>) -> OAuth2GrantType {
    match raw {
        "authorization_code" => OAuth2GrantType::AuthorizationCode,
        "client_credentials" | "" => OAuth2GrantType::ClientCredentials,
        "password_credentials" | "password" => OAuth2GrantType::Password,
        "refresh_token" => OAuth2GrantType::RefreshToken,
        other => {
            warnings.push(format!("unsupported OAuth2 grant type \"{other}\" — imported as Client Credentials"));
            OAuth2GrantType::ClientCredentials
        }
    }
}

// ---------------------------------------------------------------------------
// Exporting
// ---------------------------------------------------------------------------

fn build_collection_json(node: &ImportedNode) -> Value {
    let mut info = serde_json::Map::new();
    info.insert("_postman_id".into(), Value::String(uuid::Uuid::new_v4().to_string()));
    info.insert("name".into(), Value::String(node.name.clone()));
    if let Some(d) = &node.description {
        info.insert("description".into(), Value::String(d.clone()));
    }
    info.insert("schema".into(), Value::String(SCHEMA_URL.to_string()));

    let mut root = serde_json::Map::new();
    root.insert("info".into(), Value::Object(info));
    root.insert("item".into(), Value::Array(build_items(node)));
    root.insert("auth".into(), auth_to_json(&node.auth));
    Value::Object(root)
}

fn build_items(node: &ImportedNode) -> Vec<Value> {
    node.children.iter().map(build_folder_item).chain(node.requests.iter().map(build_request_item)).collect()
}

fn build_folder_item(child: &ImportedNode) -> Value {
    let mut m = serde_json::Map::new();
    m.insert("name".into(), Value::String(child.name.clone()));
    if let Some(d) = &child.description {
        m.insert("description".into(), Value::String(d.clone()));
    }
    m.insert("auth".into(), auth_to_json(&child.auth));
    m.insert("item".into(), Value::Array(build_items(child)));
    Value::Object(m)
}

fn build_request_item(req: &ImportedRequest) -> Value {
    let mut request = serde_json::Map::new();
    request.insert("method".into(), Value::String(req.method.clone()));
    request.insert(
        "header".into(),
        Value::Array(
            req.headers.iter().map(|h| json!({"key": h.name, "value": h.value, "disabled": !h.enabled})).collect(),
        ),
    );
    request.insert("url".into(), build_url_json(&req.url, &req.query));
    if !matches!(req.body, RequestBody::None) {
        request.insert("body".into(), build_body_json(&req.body));
    }
    if let RequestAuth::Own(cfg) = &req.auth {
        request.insert("auth".into(), auth_to_json(cfg));
    }

    let mut item = serde_json::Map::new();
    item.insert("name".into(), Value::String(req.name.clone()));
    item.insert("request".into(), Value::Object(request));
    let events = build_events(&req.pre_request_script, &req.post_response_script);
    if !events.is_empty() {
        item.insert("event".into(), Value::Array(events));
    }
    Value::Object(item)
}

/// `raw` is percent-encoded the same way `engine::http::build_url` does (via
/// `Url::query_pairs_mut`) — a naive join would let a value containing `&`,
/// `#`, or `=` corrupt `raw` into a different URL, even though the separate
/// structured `query` array below (which this app's own importer prefers)
/// stays correct either way.
fn build_url_json(base: &str, query: &[KeyValue]) -> Value {
    let enabled: Vec<&KeyValue> = query.iter().filter(|q| q.enabled).collect();
    let raw = if enabled.is_empty() {
        base.to_string()
    } else if let Ok(mut url) = reqwest::Url::parse(base.trim()) {
        {
            let mut pairs = url.query_pairs_mut();
            for q in &enabled {
                pairs.append_pair(&q.key, &q.value);
            }
        }
        url.to_string()
    } else {
        let qs: String = enabled.iter().map(|q| format!("{}={}", q.key, q.value)).collect::<Vec<_>>().join("&");
        format!("{base}?{qs}")
    };

    let mut m = serde_json::Map::new();
    m.insert("raw".into(), Value::String(raw));
    if let Some((scheme, rest)) = base.split_once("://") {
        m.insert("protocol".into(), Value::String(scheme.to_string()));
        let (host_part, path_part) = rest.split_once('/').unwrap_or((rest, ""));
        m.insert("host".into(), Value::Array(host_part.split('.').map(|h| Value::String(h.to_string())).collect()));
        if !path_part.is_empty() {
            m.insert("path".into(), Value::Array(path_part.split('/').map(|p| Value::String(p.to_string())).collect()));
        }
    }
    if !query.is_empty() {
        m.insert(
            "query".into(),
            Value::Array(query.iter().map(|q| json!({"key": q.key, "value": q.value, "disabled": !q.enabled})).collect()),
        );
    }
    Value::Object(m)
}

fn build_body_json(body: &RequestBody) -> Value {
    match body {
        RequestBody::None => Value::Object(Default::default()),
        RequestBody::Json(content) => json!({"mode": "raw", "raw": content, "options": {"raw": {"language": "json"}}}),
        RequestBody::Raw { content, language } => {
            let mut m = serde_json::Map::new();
            m.insert("mode".into(), Value::String("raw".into()));
            m.insert("raw".into(), Value::String(content.clone()));
            if let Some(lang) = language {
                m.insert("options".into(), json!({"raw": {"language": lang}}));
            }
            Value::Object(m)
        }
        RequestBody::UrlEncoded(list) => json!({
            "mode": "urlencoded",
            "urlencoded": list.iter().map(|kv| json!({"key": kv.key, "value": kv.value, "disabled": !kv.enabled})).collect::<Vec<_>>(),
        }),
        RequestBody::FormData(list) => json!({
            "mode": "formdata",
            "formdata": list.iter().map(build_form_field_json).collect::<Vec<_>>(),
        }),
        RequestBody::Binary { path } => json!({"mode": "file", "file": {"src": path}}),
        RequestBody::Graphql { query, variables } => {
            let mut g = serde_json::Map::new();
            g.insert("query".into(), Value::String(query.clone()));
            if let Some(v) = variables {
                g.insert("variables".into(), Value::String(v.clone()));
            }
            json!({"mode": "graphql", "graphql": Value::Object(g)})
        }
    }
}

fn build_form_field_json(f: &FormField) -> Value {
    if f.is_file {
        json!({"key": f.key, "type": "file", "src": f.value, "disabled": !f.enabled})
    } else {
        let mut m = serde_json::Map::new();
        m.insert("key".into(), Value::String(f.key.clone()));
        m.insert("value".into(), Value::String(f.value.clone()));
        m.insert("type".into(), Value::String("text".into()));
        m.insert("disabled".into(), Value::Bool(!f.enabled));
        if let Some(ct) = &f.content_type {
            m.insert("contentType".into(), Value::String(ct.clone()));
        }
        Value::Object(m)
    }
}

fn build_events(pre: &str, post: &str) -> Vec<Value> {
    let mut events = Vec::new();
    // `split('\n')` (not `.lines()`) deliberately keeps a trailing empty
    // element for a trailing newline, matching real Postman exports and
    // making export -> import -> export byte-stable for `exec`.
    if !pre.is_empty() {
        events.push(json!({"listen": "prerequest", "script": {"type": "text/javascript", "exec": pre.split('\n').collect::<Vec<_>>()}}));
    }
    if !post.is_empty() {
        events.push(json!({"listen": "test", "script": {"type": "text/javascript", "exec": post.split('\n').collect::<Vec<_>>()}}));
    }
    events
}

fn param(key: &str, value: &str) -> Value {
    json!({"key": key, "value": value, "type": "string"})
}

fn grant_type_str(g: OAuth2GrantType) -> &'static str {
    match g {
        OAuth2GrantType::AuthorizationCode => "authorization_code",
        OAuth2GrantType::ClientCredentials => "client_credentials",
        OAuth2GrantType::Password => "password_credentials",
        OAuth2GrantType::RefreshToken => "refresh_token",
    }
}

fn auth_to_json(cfg: &AuthConfig) -> Value {
    match cfg {
        AuthConfig::None => json!({"type": "noauth"}),
        AuthConfig::Bearer { token } => json!({"type": "bearer", "bearer": [param("token", token)]}),
        AuthConfig::Basic { username, password } => {
            json!({"type": "basic", "basic": [param("username", username), param("password", password)]})
        }
        AuthConfig::ApiKey { key, value, location } => json!({
            "type": "apikey",
            "apikey": [
                param("key", key),
                param("value", value),
                param("in", if *location == ApiKeyLocation::Query { "query" } else { "header" }),
            ],
        }),
        AuthConfig::OAuth2(c) => json!({
            "type": "oauth2",
            "oauth2": [
                param("grant_type", grant_type_str(c.grant_type)),
                param("authUrl", &c.auth_url),
                param("accessTokenUrl", &c.token_url),
                param("clientId", &c.client_id),
                param("clientSecret", &c.client_secret),
                param("scope", &c.scope),
                param("redirect_uri", &c.redirect_uri),
                param("username", &c.username),
                param("password", &c.password),
                param("refreshToken", &c.refresh_token),
            ],
        }),
        AuthConfig::AwsSigV4(c) => json!({
            "type": "awsv4",
            "awsv4": [
                param("accessKey", &c.access_key),
                param("secretKey", &c.secret_key),
                param("region", &c.region),
                param("service", &c.service),
                param("sessionToken", &c.session_token),
            ],
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PETSTORE_FIXTURE: &str = r#"{
      "info": {
        "_postman_id": "11111111-1111-1111-1111-111111111111",
        "name": "Petstore",
        "description": "Sample pet store API collection",
        "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json"
      },
      "auth": { "type": "bearer", "bearer": [{"key": "token", "value": "collection-level-token", "type": "string"}] },
      "item": [
        {
          "name": "Pets",
          "description": "Pet endpoints",
          "item": [
            {
              "name": "Get pet by id",
              "request": {
                "method": "GET",
                "header": [
                  {"key": "Accept", "value": "application/json", "disabled": false},
                  {"key": "X-Debug", "value": "1", "disabled": true}
                ],
                "url": {
                  "raw": "https://petstore.example.com/pets/123?verbose=true",
                  "protocol": "https",
                  "host": ["petstore", "example", "com"],
                  "path": ["pets", "123"],
                  "query": [
                    {"key": "verbose", "value": "true", "disabled": false},
                    {"key": "debug", "value": "1", "disabled": true}
                  ]
                },
                "auth": {
                  "type": "basic",
                  "basic": [
                    {"key": "username", "value": "alice", "type": "string"},
                    {"key": "password", "value": "secret", "type": "string"}
                  ]
                }
              },
              "event": [
                {"listen": "prerequest", "script": {"type": "text/javascript", "exec": ["console.log('before');", "pm.environment.set('x', '1');"]}},
                {"listen": "test", "script": {"type": "text/javascript", "exec": ["pm.test('status is 200', function () {", "  pm.response.to.have.status(200);", "});"]}}
              ]
            },
            {
              "name": "Delete pet",
              "request": {
                "method": "DELETE",
                "header": [],
                "url": { "raw": "https://petstore.example.com/pets/123", "protocol": "https", "host": ["petstore","example","com"], "path": ["pets","123"] },
                "auth": { "type": "noauth" }
              }
            }
          ]
        },
        {
          "name": "Create pet",
          "request": {
            "method": "POST",
            "header": [{"key": "Content-Type", "value": "application/json", "disabled": false}],
            "url": { "raw": "https://petstore.example.com/pets", "protocol": "https", "host": ["petstore","example","com"], "path": ["pets"] },
            "body": { "mode": "raw", "raw": "{\"name\":\"Fido\"}", "options": { "raw": { "language": "json" } } }
          }
        },
        {
          "name": "Update pet form",
          "request": {
            "method": "PUT",
            "header": [],
            "url": { "raw": "https://petstore.example.com/pets/123", "protocol": "https", "host": ["petstore","example","com"], "path": ["pets","123"] },
            "body": {
              "mode": "urlencoded",
              "urlencoded": [
                {"key": "name", "value": "Fido", "disabled": false},
                {"key": "legacy", "value": "x", "disabled": true}
              ]
            }
          }
        },
        {
          "name": "Upload pet photo",
          "request": {
            "method": "POST",
            "header": [],
            "url": { "raw": "https://petstore.example.com/pets/123/photo", "protocol": "https", "host": ["petstore","example","com"], "path": ["pets","123","photo"] },
            "body": {
              "mode": "formdata",
              "formdata": [
                {"key": "caption", "value": "cute dog", "type": "text", "disabled": false},
                {"key": "photo", "type": "file", "src": "/tmp/fido.png", "disabled": false}
              ]
            },
            "auth": { "type": "digest", "digest": [{"key": "username", "value": "x", "type": "string"}] }
          }
        },
        {
          "name": "Pet graphql",
          "request": {
            "method": "POST",
            "header": [],
            "url": { "raw": "https://petstore.example.com/graphql", "protocol": "https", "host": ["petstore","example","com"], "path": ["graphql"] },
            "body": { "mode": "graphql", "graphql": { "query": "query { pets { id name } }", "variables": "{}" } }
          }
        }
      ],
      "variable": [{"key": "base_url", "value": "https://petstore.example.com"}]
    }"#;

    #[test]
    fn imports_realistic_postman_fixture_with_expected_shape_and_warnings() {
        let preview = parse(PETSTORE_FIXTURE).unwrap();
        assert_eq!(preview.root.name, "Petstore");
        assert_eq!(preview.root.auth, AuthConfig::Bearer { token: "collection-level-token".into() });
        assert_eq!(preview.root.children.len(), 1);
        assert_eq!(preview.root.requests.len(), 4);

        let pets_folder = &preview.root.children[0];
        assert_eq!(pets_folder.requests.len(), 2);
        let get_pet = &pets_folder.requests[0];
        assert_eq!(get_pet.url, "https://petstore.example.com/pets/123");
        assert_eq!(
            get_pet.query,
            vec![
                KeyValue { key: "verbose".into(), value: "true".into(), enabled: true },
                KeyValue { key: "debug".into(), value: "1".into(), enabled: false },
            ]
        );
        assert!(!get_pet.headers.iter().find(|h| h.name == "X-Debug").unwrap().enabled);
        assert_eq!(
            get_pet.auth,
            RequestAuth::Own(AuthConfig::Basic { username: "alice".into(), password: "secret".into() })
        );
        assert!(get_pet.pre_request_script.contains("before"));
        assert!(get_pet.post_response_script.contains("status is 200"));

        let delete_pet = &pets_folder.requests[1];
        assert_eq!(delete_pet.auth, RequestAuth::Own(AuthConfig::None));

        let create_pet = preview.root.requests.iter().find(|r| r.name == "Create pet").unwrap();
        assert_eq!(create_pet.auth, RequestAuth::Inherit);
        assert_eq!(create_pet.body, RequestBody::Json("{\"name\":\"Fido\"}".into()));

        let update_form = preview.root.requests.iter().find(|r| r.name == "Update pet form").unwrap();
        assert_eq!(
            update_form.body,
            RequestBody::UrlEncoded(vec![
                KeyValue { key: "name".into(), value: "Fido".into(), enabled: true },
                KeyValue { key: "legacy".into(), value: "x".into(), enabled: false },
            ])
        );

        let upload = preview.root.requests.iter().find(|r| r.name == "Upload pet photo").unwrap();
        assert_eq!(upload.auth, RequestAuth::Own(AuthConfig::None));
        assert_eq!(
            upload.body,
            RequestBody::FormData(vec![
                FormField { key: "caption".into(), value: "cute dog".into(), enabled: true, is_file: false, content_type: None },
                FormField { key: "photo".into(), value: "/tmp/fido.png".into(), enabled: true, is_file: true, content_type: None },
            ])
        );

        let graphql = preview.root.requests.iter().find(|r| r.name == "Pet graphql").unwrap();
        assert_eq!(
            graphql.body,
            RequestBody::Graphql { query: "query { pets { id name } }".into(), variables: Some("{}".into()) }
        );

        assert!(preview.warnings.iter().any(|w| w.contains("variable")));
        assert!(preview.warnings.iter().any(|w| w.contains("digest")));
        assert_eq!(preview.stats.requests, 6);
        assert_eq!(preview.stats.folders, 1);
    }

    #[test]
    fn postman_json_round_trips_through_model_twice() {
        let preview_a = parse(PETSTORE_FIXTURE).unwrap();
        let json2 = export(&preview_a.root).unwrap();
        let preview_b = parse(&json2).unwrap();
        assert_eq!(preview_a.root, preview_b.root);
    }

    #[test]
    fn build_url_json_percent_encodes_raw_query_values_with_special_characters() {
        let query = vec![KeyValue { key: "q".into(), value: "a&b c".into(), enabled: true }];
        let v = build_url_json("https://api.example.com/items", &query);
        let raw = v.get("raw").and_then(Value::as_str).unwrap();
        assert!(!raw.contains("a&b c"), "{raw}");
        let reparsed = reqwest::Url::parse(raw).unwrap();
        let pairs: Vec<(String, String)> = reparsed.query_pairs().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        assert_eq!(pairs, vec![("q".to_string(), "a&b c".to_string())]);
    }

    #[test]
    fn bare_string_url_with_no_structured_query_still_splits_params() {
        let (base, query) = parse_url(Some(&Value::String("https://a.test/x?a=1&b=2".into())));
        assert_eq!(base, "https://a.test/x");
        assert_eq!(
            query,
            vec![
                KeyValue { key: "a".into(), value: "1".into(), enabled: true },
                KeyValue { key: "b".into(), value: "2".into(), enabled: true },
            ]
        );
    }
}
