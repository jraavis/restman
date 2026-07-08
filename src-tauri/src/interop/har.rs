//! HAR (HTTP Archive) 1.2 import/export. A HAR document captures recorded
//! browser/devtools exchanges — every entry is a request/response pair, with
//! no collection or folder notion of its own. Like `curl`, `parse` wraps the
//! flat request list in a synthetic root `ImportedNode` (named after the HAR
//! file's first page, or "Imported HAR" if none); `export` walks the tree
//! and emits one entry per request, with synthesized placeholder responses
//! (HAR requires a `response` object on every entry, even though this app's
//! own model doesn't store responses alongside requests).
//!
//! Only `request` is meaningful on import — `response`/`timings`/`cache` are
//! discarded (this app records its own history). `headers` is an array of
//! `{name, value}` (no `disabled` flag in HAR — every header is enabled).
//! `queryString` mirrors that shape. `postData` carries `mimeType` plus one
//! of `text`/`params` — see `parse_post_data` for the mapping.
//!
//! Secrets: respect the same mask-on-write contract as every other interop
//! module — see `interop` module doc. Import goes through `apply_import` (no
//! extra masking here); export reads through `collect()` (already masked).

use crate::error::{AppError, AppResult};
use crate::interop::{ImportPreview, ImportedNode, ImportedRequest};
use crate::model::auth::{ApiKeyLocation, AuthConfig, RequestAuth};
use crate::model::http::{FormField, HeaderEntry, KeyValue, RequestBody, RequestOptions};
use serde_json::{json, Value};

pub fn parse(content: &str) -> AppResult<ImportPreview> {
    let v: Value = serde_json::from_str(content).map_err(|e| AppError::Other(format!("invalid HAR JSON: {e}")))?;
    let log = v
        .get("log")
        .ok_or_else(|| AppError::Other("not a HAR document: missing \"log\" object".into()))?;
    let pages = log.get("pages").and_then(Value::as_array);
    let root_name = pages
        .and_then(|a| a.first())
        .and_then(|p| p.get("title"))
        .and_then(Value::as_str)
        .unwrap_or("Imported HAR")
        .to_string();
    let root_desc = pages
        .and_then(|a| a.first())
        .and_then(|p| p.get("id").or_else(|| p.get("comment")))
        .and_then(description_to_string);

    let entries = log.get("entries").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut warnings = Vec::new();
    let requests: Vec<ImportedRequest> = entries
        .iter()
        .filter_map(|e| {
            let req = e.get("request")?;
            Some(parse_request(req, &mut warnings))
        })
        .collect();

    let root = ImportedNode { name: root_name, description: root_desc, auth: AuthConfig::None, requests, children: Vec::new() };
    Ok(ImportPreview::new(root, warnings))
}

pub fn export(node: &ImportedNode) -> AppResult<String> {
    let mut entries = Vec::new();
    collect_entries(node, &mut entries);
    if entries.is_empty() {
        return Err(AppError::Other("nothing to export: no requests in this collection".into()));
    }
    let log = json!({
        "log": {
            "version": "1.2",
            "creator": { "name": "restman", "version": "0.1.0" },
            "entries": entries,
        }
    });
    Ok(serde_json::to_string_pretty(&log)?)
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

fn parse_request(req: &Value, warnings: &mut Vec<String>) -> ImportedRequest {
    let method = req.get("method").and_then(Value::as_str).unwrap_or("GET").to_uppercase();
    let raw_url = req.get("url").map(url_to_string).unwrap_or_default();
    let (base, query) = split_url(&raw_url, req.get("queryString"));
    let mut headers = req
        .get("headers")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|h| {
                    let name = h.get("name").and_then(Value::as_str)?.to_string();
                    let value = h.get("value").and_then(Value::as_str).unwrap_or_default().to_string();
                    Some(HeaderEntry { name, value, enabled: true })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let auth = match headers.iter().position(|h| h.name.eq_ignore_ascii_case("authorization")) {
        Some(idx) => {
            let value = headers[idx].value.clone();
            if let Some(rest) = value.strip_prefix("Basic ") {
                let decoded = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, rest).unwrap_or_default();
                let creds = String::from_utf8_lossy(&decoded);
                let (username, password) = creds.split_once(':').map(|(u, p)| (u.to_string(), p.to_string())).unwrap_or((creds.to_string(), String::new()));
                headers.remove(idx);
                RequestAuth::Own(AuthConfig::Basic { username, password })
            } else if let Some(token) = value.strip_prefix("Bearer ") {
                // Keep the Authorization header in the request — real HARs
                // recorded from Postman/Insomnia usually duplicate Bearer
                // under both an explicit header and (for Postman-exported
                // HAR) an auth section; preserving the header matches what
                // the recording actually sent. Remove only when Basic, since
                // we represent Basic as -u-style auth, not as a header.
                RequestAuth::Own(AuthConfig::Bearer { token: token.to_string(), prefix: crate::model::auth::default_bearer_prefix() })
            } else {
                RequestAuth::Inherit
            }
        }
        None => RequestAuth::Inherit,
    };

    let body = req.get("postData").map(|pd| parse_post_data(pd, &headers, warnings)).unwrap_or(RequestBody::None);
    let name = derive_name(&method, &base);

    ImportedRequest {
        name,
        method,
        url: base,
        headers,
        query,
        body,
        options: RequestOptions::default(),
        auth,
        pre_request_script: String::new(),
        post_response_script: String::new(),
        ..Default::default()
    }
}

fn description_to_string(v: &Value) -> Option<String> {
    let s = match v {
        Value::String(s) => s.clone(),
        _ => return None,
    };
    if s.is_empty() { None } else { Some(s) }
}

fn url_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Object(_) => v.get("url").and_then(Value::as_str).unwrap_or_default().to_string(),
        _ => String::new(),
    }
}

fn split_url(raw: &str, structured: Option<&Value>) -> (String, Vec<KeyValue>) {
    let query = match structured.and_then(Value::as_array) {
        Some(list) => list
            .iter()
            .filter_map(|q| {
                let key = q.get("name").and_then(Value::as_str)?.to_string();
                let value = q.get("value").and_then(Value::as_str).unwrap_or_default().to_string();
                Some(KeyValue { key, value, enabled: true })
            })
            .collect(),
        None => raw
            .split_once('?')
            .map(|(_, qs)| {
                qs.split('&')
                    .filter(|s| !s.is_empty())
                    .map(|pair| match pair.split_once('=') {
                        Some((k, v)) => KeyValue { key: k.to_string(), value: v.to_string(), enabled: true },
                        None => KeyValue { key: pair.to_string(), value: String::new(), enabled: true },
                    })
                    .collect()
            })
            .unwrap_or_default(),
    };
    let base = raw.split('?').next().unwrap_or(raw).to_string();
    (base, query)
}

fn parse_post_data(pd: &Value, headers: &[HeaderEntry], warnings: &mut Vec<String>) -> RequestBody {
    let mime = pd.get("mimeType").and_then(Value::as_str).unwrap_or_default().to_ascii_lowercase();
    if mime.contains("json") {
        return RequestBody::Json(pd.get("text").and_then(Value::as_str).unwrap_or_default().to_string());
    }
    if mime.contains("x-www-form-urlencoded") {
        return RequestBody::UrlEncoded(
            pd.get("params")
                .and_then(Value::as_array)
                .map(|list| {
                    list.iter()
                        .filter_map(|p| {
                            let key = p.get("name").and_then(Value::as_str)?.to_string();
                            let value = p.get("value").and_then(Value::as_str).unwrap_or_default().to_string();
                            Some(KeyValue { key, value, enabled: true })
                        })
                        .collect()
                })
                .unwrap_or_else(|| {
                    let text = pd.get("text").and_then(Value::as_str).unwrap_or_default();
                    text.split('&')
                        .filter(|s| !s.is_empty())
                        .map(|pair| match pair.split_once('=') {
                            Some((k, v)) => KeyValue { key: k.to_string(), value: v.to_string(), enabled: true },
                            None => KeyValue { key: pair.to_string(), value: String::new(), enabled: true },
                        })
                        .collect()
                }),
        );
    }
    if mime.contains("multipart/form-data") {
        return RequestBody::FormData(
            pd.get("params")
                .and_then(Value::as_array)
                .map(|list| {
                    list.iter()
                        .filter_map(|p| {
                            let key = p.get("name").and_then(Value::as_str)?.to_string();
                            let value = p.get("value").and_then(Value::as_str).unwrap_or_default().to_string();
                            let is_file = p.get("fileName").and_then(Value::as_str).is_some();
                            let content_type = p.get("contentType").and_then(Value::as_str).map(str::to_string);
                            Some(FormField { key, value: if is_file { String::new() } else { value }, enabled: true, is_file, content_type })
                        })
                        .collect()
                })
                .unwrap_or_default(),
        );
    }
    if let Some(text) = pd.get("text").and_then(Value::as_str) {
        let language = mime.split('/').nth(1).map(str::to_string);
        return RequestBody::Raw { content: text.to_string(), language };
    }
    if let Some(file) = pd.get("fileName").and_then(Value::as_str) {
        return RequestBody::Binary { path: file.to_string() };
    }
    let _ = headers;
    warnings.push(format!("HAR postData with mimeType \"{mime}\" has no recognizable body shape — imported with an empty body"));
    RequestBody::None
}

fn derive_name(method: &str, url: &str) -> String {
    let path = url
        .split_once("://")
        .and_then(|(_, rest)| rest.split_once('/'))
        .map(|(_, p)| format!("/{p}"))
        .filter(|p| p != "/");
    format!("{method} {}", path.unwrap_or_else(|| url.to_string()))
}

// ---------------------------------------------------------------------------
// Exporting
// ---------------------------------------------------------------------------

fn collect_entries(node: &ImportedNode, entries: &mut Vec<Value>) {
    for req in &node.requests {
        entries.push(build_entry(req));
    }
    for child in &node.children {
        collect_entries(child, entries);
    }
}

fn build_entry(req: &ImportedRequest) -> Value {
    let (mut effective_headers, effective_query) = effective_parts(req);

    let url_value = if effective_query.is_empty() {
        req.url.clone()
    } else if let Ok(mut url) = reqwest::Url::parse(req.url.trim()) {
        {
            let mut pairs = url.query_pairs_mut();
            for (k, v) in &effective_query {
                pairs.append_pair(k, v);
            }
        }
        url.to_string()
    } else {
        let qs: String = effective_query.iter().map(|(k, v)| format!("{}={}", k, v)).collect::<Vec<_>>().join("&");
        format!("{}?{}", req.url, qs)
    };

    // Avoid a duplicate Authorization header when auth is rendered into
    // effective_headers but the request also carried an explicit one (which
    // would already be present in req.headers, doubled by effective_parts).
    effective_headers.dedup_by(|a, b| a.0.eq_ignore_ascii_case(&b.0));

    json!({
        "startedDateTime": "1970-01-01T00:00:00.000Z",
        "time": 0,
        "request": {
            "method": req.method,
            "url": url_value,
            "httpVersion": "HTTP/1.1",
            "headers": effective_headers.iter().map(|(k, v)| json!({"name": k, "value": v})).collect::<Vec<_>>(),
            "queryString": effective_query.iter().map(|(k, v)| json!({"name": k, "value": v})).collect::<Vec<_>>(),
            "headersSize": -1,
            "bodySize": -1,
            "postData": build_post_data(&req.body),
        },
        "response": {
            "status": 0,
            "statusText": "",
            "httpVersion": "HTTP/1.1",
            "headers": [],
            "cookies": [],
            "content": {"size": 0, "mimeType": "text/plain"},
            "redirectURL": "",
            "headersSize": -1,
            "bodySize": -1,
        },
        "cache": {},
        "timings": {"send": 0, "wait": 0, "receive": 0},
    })
}

fn effective_parts(req: &ImportedRequest) -> (Vec<(String, String)>, Vec<(String, String)>) {
    let mut headers: Vec<(String, String)> = req.headers.iter().filter(|h| h.enabled).map(|h| (h.name.clone(), h.value.clone())).collect();
    let mut query: Vec<(String, String)> = req.query.iter().filter(|q| q.enabled).map(|q| (q.key.clone(), q.value.clone())).collect();
    match &req.auth {
        RequestAuth::Own(AuthConfig::Bearer { token, prefix }) => {
            headers.push(("Authorization".into(), crate::model::auth::bearer_header_value(prefix, token)));
        }
        RequestAuth::Own(AuthConfig::Basic { username, password }) => {
            let encoded = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, format!("{username}:{password}"));
            headers.push(("Authorization".into(), format!("Basic {encoded}")));
        }
        RequestAuth::Own(AuthConfig::ApiKey { key, value, location }) => match location {
            ApiKeyLocation::Header => headers.push((key.clone(), value.clone())),
            ApiKeyLocation::Query => query.push((key.clone(), value.clone())),
        },
        _ => {}
    }
    (headers, query)
}

fn build_post_data(body: &RequestBody) -> Value {
    match body {
        RequestBody::None => Value::Null,
        RequestBody::Json(content) => json!({"mimeType": "application/json", "text": content}),
        RequestBody::Raw { content, language } => {
            let mime = match language.as_deref() {
                Some("xml") => "application/xml",
                Some("html") => "text/html",
                _ => "text/plain",
            };
            json!({"mimeType": mime, "text": content})
        }
        RequestBody::UrlEncoded(list) => json!({
            "mimeType": "application/x-www-form-urlencoded",
            "params": list.iter().filter(|kv| kv.enabled).map(|kv| json!({"name": kv.key, "value": kv.value})).collect::<Vec<_>>(),
        }),
        RequestBody::FormData(list) => json!({
            "mimeType": "multipart/form-data",
            "params": list.iter().filter(|f| f.enabled).map(|f| {
                if f.is_file {
                    json!({"name": f.key, "fileName": f.value})
                } else {
                    json!({"name": f.key, "value": f.value})
                }
            }).collect::<Vec<_>>(),
        }),
        RequestBody::Binary { path } => json!({"mimeType": "application/octet-stream", "fileName": path}),
        RequestBody::Graphql { query, variables, operation_name } => {
            let mut parts = vec![format!("\"query\":{}", serde_json::to_string(query).unwrap_or_default())];
            if let Some(v) = variables {
                parts.push(format!("\"variables\":{v}"));
            }
            if let Some(name) = operation_name {
                if !name.trim().is_empty() {
                    parts.push(format!("\"operationName\":{}", serde_json::to_string(name).unwrap_or_default()));
                }
            }
            let body = format!("{{{}}}", parts.join(","));
            json!({"mimeType": "application/json", "text": body})
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HAR_FIXTURE: &str = r#"{
        "log": {
            "version": "1.2",
            "creator": {"name": "WebInspector", "version": "537.36"},
            "pages": [{"title": "Page 1", "comment": "recorded session"}],
            "entries": [
                {
                    "startedDateTime": "2024-01-01T00:00:00.000Z",
                    "time": 42,
                    "request": {
                        "method": "POST",
                        "url": "https://api.example.com/items?limit=5",
                        "httpVersion": "HTTP/1.1",
                        "headers": [
                            {"name": "Content-Type", "value": "application/json"},
                            {"name": "Authorization", "value": "Bearer abc123"}
                        ],
                        "queryString": [{"name": "limit", "value": "5"}],
                        "postData": {"mimeType": "application/json", "text": "{\"name\":\"Fido\"}"},
                        "headersSize": -1,
                        "bodySize": -1
                    },
                    "response": {"status": 200, "statusText": "OK"}
                },
                {
                    "startedDateTime": "2024-01-01T00:00:01.000Z",
                    "time": 10,
                    "request": {
                        "method": "GET",
                        "url": "https://api.example.com/items",
                        "httpVersion": "HTTP/1.1",
                        "headers": [],
                        "queryString": [],
                        "headersSize": -1,
                        "bodySize": -1
                    },
                    "response": {"status": 200}
                },
                {
                    "startedDateTime": "2024-01-01T00:00:02.000Z",
                    "time": 10,
                    "request": {
                        "method": "POST",
                        "url": "https://api.example.com/login",
                        "headers": [{"name": "Authorization", "value": "Basic YWxpY2U6c2VjcmV0"}],
                        "queryString": [],
                        "postData": {"mimeType": "application/x-www-form-urlencoded", "params": [{"name": "user", "value": "alice"}]}
                    },
                    "response": {"status": 200}
                }
            ]
        }
    }"#;

    #[test]
    fn imports_realistic_har_fixture_with_expected_shape() {
        let preview = parse(HAR_FIXTURE).unwrap();
        assert_eq!(preview.root.name, "Page 1");
        assert_eq!(preview.root.description, Some("recorded session".to_string()));
        assert_eq!(preview.root.requests.len(), 3);

        let post = &preview.root.requests[0];
        assert_eq!(post.method, "POST");
        assert_eq!(post.url, "https://api.example.com/items");
        assert_eq!(post.query, vec![KeyValue { key: "limit".into(), value: "5".into(), enabled: true }]);
        assert_eq!(post.auth, RequestAuth::Own(AuthConfig::Bearer { token: "abc123".into(), prefix: crate::model::auth::default_bearer_prefix() }));
        assert_eq!(post.body, RequestBody::Json("{\"name\":\"Fido\"}".into()));
        assert_eq!(post.name, "POST /items");

        let get = &preview.root.requests[1];
        assert_eq!(get.method, "GET");
        assert_eq!(get.auth, RequestAuth::Inherit);
        assert_eq!(get.body, RequestBody::None);

        let login = &preview.root.requests[2];
        assert_eq!(login.headers.len(), 0, "Basic auth header should be consumed into the auth config");
        assert_eq!(
            login.auth,
            RequestAuth::Own(AuthConfig::Basic { username: "alice".into(), password: "secret".into() })
        );
        match &login.body {
            RequestBody::UrlEncoded(kv) => assert_eq!(kv, &[KeyValue { key: "user".into(), value: "alice".into(), enabled: true }]),
            other => panic!("expected UrlEncoded body, got {other:?}"),
        }

        assert_eq!(preview.stats.requests, 3);
        assert_eq!(preview.stats.folders, 0);
    }

    #[test]
    fn har_json_round_trips_through_model_twice() {
        let preview_a = parse(HAR_FIXTURE).unwrap();
        let json2 = export(&preview_a.root).unwrap();
        let preview_b = parse(&json2).unwrap();
        assert_eq!(preview_a.root.requests.len(), preview_b.root.requests.len());
        for (a, b) in preview_a.root.requests.iter().zip(preview_b.root.requests.iter()) {
            assert_eq!(a.method, b.method);
            assert_eq!(a.url, b.url);
        }
    }

    #[test]
    fn export_synthesizes_required_response_object() {
        let req = ImportedRequest {
            name: "Get".into(),
            method: "GET".into(),
            url: "https://api.test/x".into(),
            headers: vec![],
            query: vec![],
            body: RequestBody::None,
            options: RequestOptions::default(),
            auth: RequestAuth::Inherit,
            pre_request_script: String::new(),
            post_response_script: String::new(),
            ..Default::default()
        };
        let node = ImportedNode { name: "x".into(), description: None, auth: AuthConfig::None, requests: vec![req], children: vec![] };
        let out = export(&node).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        let entry = &v["log"]["entries"][0];
        assert_eq!(entry["request"]["method"], "GET");
        assert_eq!(entry["response"]["status"], 0);
    }

    #[test]
    fn rejects_non_har_json() {
        let err = parse(r#"{"foo": "bar"}"#).unwrap_err();
        assert!(err.to_string().contains("log"));
    }
}
