//! OpenAPI 3.0 / Swagger 2.0 import, OpenAPI 3.0 export. Parses into and
//! serializes out of the shared `interop` IR — see module doc there for the
//! secret-handling contract this must respect.
//!
//! Like `postman`, parsing works off `serde_json::Value` rather than typed
//! structs: real-world specs are full of `$ref` indirection and optional
//! fields, and a `Value` walk lets a missing/unsupported bit degrade to a
//! warning instead of a hard parse failure.
//!
//! Input may be JSON or YAML (Swagger/OpenAPI documents are commonly
//! authored as YAML) — `parse_yaml_or_json` tries JSON first, then falls
//! back to `saphyr` for YAML, bridging to `serde_json::Value` so the rest of
//! this module only ever deals with one value type. Export always emits
//! JSON: OpenAPI-as-JSON is fully valid and round-trips through `serde_json`
//! without needing a reverse Value->Yaml bridge.
//!
//! `$ref` resolution is local-only (`#/...` JSON Pointers via
//! `Value::pointer`), single chain with a depth guard against cycles.
//! External refs (anything not starting with `#`) are never followed — no
//! network fetch is ever attempted during parse, mirroring the no-live-call
//! rule already in place for OAuth2/AWS SigV4 codegen.

use crate::error::{AppError, AppResult};
use crate::interop::{ImportPreview, ImportedNode, ImportedRequest};
use crate::model::auth::{ApiKeyLocation, AuthConfig, OAuth2Config, OAuth2GrantType, RequestAuth};
use crate::model::http::{FormField, HeaderEntry, KeyValue, RequestBody, RequestOptions};
use saphyr::{LoadableYamlNode, Scalar, Yaml};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub fn parse(content: &str) -> AppResult<ImportPreview> {
    let v = parse_yaml_or_json(content)?;
    let mut warnings = Vec::new();
    let root = if v.get("openapi").and_then(Value::as_str).is_some() {
        parse_openapi3(&v, &mut warnings)?
    } else if v.get("swagger").and_then(Value::as_str).is_some() {
        parse_swagger2(&v, &mut warnings)?
    } else {
        return Err(AppError::Other(
            "not an OpenAPI/Swagger document: missing \"openapi\" or \"swagger\" version field".into(),
        ));
    };
    Ok(ImportPreview::new(root, warnings))
}

pub fn export(node: &ImportedNode) -> AppResult<String> {
    let mut paths = serde_json::Map::new();
    let mut origin = None;
    let mut schemes = serde_json::Map::new();
    let root_security = security_ref_for(&node.auth, &mut schemes);
    collect_paths(node, None, &node.auth, &node.auth, &mut paths, &mut origin, &mut schemes);

    let mut root = serde_json::Map::new();
    root.insert("openapi".into(), Value::String("3.0.3".into()));

    let mut info = serde_json::Map::new();
    info.insert("title".into(), Value::String(node.name.clone()));
    if let Some(d) = &node.description {
        info.insert("description".into(), Value::String(d.clone()));
    }
    info.insert("version".into(), Value::String("1.0.0".into()));
    root.insert("info".into(), Value::Object(info));

    if let Some(origin) = origin.filter(|o: &String| !o.is_empty()) {
        root.insert("servers".into(), json!([{ "url": origin }]));
    }

    root.insert("paths".into(), Value::Object(paths));

    if !schemes.is_empty() {
        let mut components = serde_json::Map::new();
        components.insert("securitySchemes".into(), Value::Object(schemes));
        root.insert("components".into(), Value::Object(components));
    }
    if let Some(sec) = root_security {
        root.insert("security".into(), Value::Array(vec![sec]));
    }

    Ok(serde_json::to_string_pretty(&Value::Object(root))?)
}

// ---------------------------------------------------------------------------
// YAML/JSON bridge
// ---------------------------------------------------------------------------

fn parse_yaml_or_json(content: &str) -> AppResult<Value> {
    if let Ok(v) = serde_json::from_str::<Value>(content) {
        return Ok(v);
    }
    let docs = Yaml::load_from_str(content).map_err(|e| AppError::Other(format!("invalid YAML/JSON: {e}")))?;
    let doc = docs.first().ok_or_else(|| AppError::Other("empty YAML document".into()))?;
    Ok(yaml_to_json(doc))
}

fn yaml_to_json(y: &Yaml) -> Value {
    match y {
        Yaml::Value(Scalar::Null) => Value::Null,
        Yaml::Value(Scalar::Boolean(b)) => Value::Bool(*b),
        Yaml::Value(Scalar::Integer(i)) => Value::Number((*i).into()),
        Yaml::Value(Scalar::FloatingPoint(f)) => serde_json::Number::from_f64(f.into_inner()).map(Value::Number).unwrap_or(Value::Null),
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

// ---------------------------------------------------------------------------
// $ref resolution — local JSON Pointers only, single chain, depth-guarded
// ---------------------------------------------------------------------------

fn resolve_ref(root: &Value, v: &Value, warnings: &mut Vec<String>) -> Value {
    resolve_ref_depth(root, v, warnings, 0)
}

fn resolve_ref_depth(root: &Value, v: &Value, warnings: &mut Vec<String>, depth: u8) -> Value {
    let Some(r) = v.get("$ref").and_then(Value::as_str) else { return v.clone() };
    if depth > 8 {
        warnings.push(format!("$ref \"{r}\": too many levels of indirection — skipped"));
        return Value::Null;
    }
    let Some(pointer) = r.strip_prefix('#') else {
        warnings.push(format!("$ref \"{r}\": external references are not supported — skipped"));
        return Value::Null;
    };
    match root.pointer(pointer) {
        Some(target) => resolve_ref_depth(root, target, warnings, depth + 1),
        None => {
            warnings.push(format!("$ref \"{r}\": not found in document — skipped"));
            Value::Null
        }
    }
}

// ---------------------------------------------------------------------------
// OpenAPI 3.0 import
// ---------------------------------------------------------------------------

struct Operation {
    tag: Option<String>,
    request: ImportedRequest,
}

/// Read-only context threaded through operation-building — grouped into one
/// struct purely to keep `build_operation_v3`'s argument count under
/// clippy's threshold; it carries no behavior of its own.
struct Context<'a> {
    root: &'a Value,
    base_url: &'a str,
    security_schemes: &'a Value,
}

fn parse_openapi3(root: &Value, warnings: &mut Vec<String>) -> AppResult<ImportedNode> {
    let info = root.get("info").ok_or_else(|| AppError::Other("not an OpenAPI document: missing \"info\" object".into()))?;
    let name = info.get("title").and_then(Value::as_str).unwrap_or("Imported API").to_string();
    let description = info.get("description").and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string);

    let base_url =
        root.get("servers").and_then(Value::as_array).and_then(|a| a.first()).and_then(|s| s.get("url")).and_then(Value::as_str).unwrap_or("").to_string();

    let security_schemes = root.get("components").and_then(|c| c.get("securitySchemes")).cloned().unwrap_or(Value::Null);
    let global_security = root.get("security").and_then(Value::as_array).cloned().unwrap_or_default();
    let root_auth = resolve_security(&global_security, &security_schemes, warnings);

    let ctx = Context { root, base_url: &base_url, security_schemes: &security_schemes };
    let mut ops = Vec::new();
    if let Some(paths) = root.get("paths").and_then(Value::as_object) {
        for (path, path_item) in paths {
            if path_item.get("$ref").is_some() {
                warnings.push(format!("path \"{path}\": $ref path items are not supported — skipped"));
                continue;
            }
            let path_level_params: Vec<Value> = path_item.get("parameters").and_then(Value::as_array).cloned().unwrap_or_default();
            for method in ["get", "put", "post", "delete", "options", "head", "patch", "trace"] {
                let Some(op) = path_item.get(method) else { continue };
                ops.push(build_operation_v3(&ctx, path, method, op, &path_level_params, warnings));
            }
        }
    }

    Ok(group_by_tag(ops, name, description, root_auth))
}

fn group_by_tag(ops: Vec<Operation>, name: String, description: Option<String>, auth: AuthConfig) -> ImportedNode {
    let mut root = ImportedNode { name, description, auth, ..Default::default() };
    let mut folders: BTreeMap<String, Vec<ImportedRequest>> = BTreeMap::new();
    for op in ops {
        match op.tag {
            Some(tag) => folders.entry(tag).or_default().push(op.request),
            None => root.requests.push(op.request),
        }
    }
    root.children = folders.into_iter().map(|(name, requests)| ImportedNode { name, requests, ..Default::default() }).collect();
    root
}

fn build_operation_v3(
    ctx: &Context,
    path: &str,
    method: &str,
    op: &Value,
    path_level_params: &[Value],
    warnings: &mut Vec<String>,
) -> Operation {
    let name = op.get("summary").and_then(Value::as_str).or_else(|| op.get("operationId").and_then(Value::as_str)).unwrap_or(path).to_string();
    let tag = op.get("tags").and_then(Value::as_array).and_then(|a| a.first()).and_then(Value::as_str).map(str::to_string);

    let mut params: Vec<Value> = path_level_params.iter().map(|p| resolve_ref(ctx.root, p, warnings)).collect();
    if let Some(op_params) = op.get("parameters").and_then(Value::as_array) {
        params.extend(op_params.iter().map(|p| resolve_ref(ctx.root, p, warnings)));
    }

    let mut headers = Vec::new();
    let mut query = Vec::new();
    for p in &params {
        let pname = p.get("name").and_then(Value::as_str).unwrap_or_default().to_string();
        if pname.is_empty() {
            continue;
        }
        let value = param_example(p);
        match p.get("in").and_then(Value::as_str) {
            Some("query") => query.push(KeyValue { key: pname, value, enabled: true }),
            Some("header") => headers.push(HeaderEntry { name: pname, value, enabled: true }),
            Some("cookie") => {
                warnings.push(format!("{} {path}: cookie parameter \"{pname}\" is not supported — skipped", method.to_uppercase()))
            }
            _ => {} // "path" is already satisfied by the literal {name} segment in the URL
        }
    }

    let body = op.get("requestBody").map(|rb| build_request_body_v3(ctx.root, rb, warnings)).unwrap_or(RequestBody::None);

    let auth = match op.get("security") {
        Some(Value::Array(arr)) => RequestAuth::Own(resolve_security(arr, ctx.security_schemes, warnings)),
        _ => RequestAuth::Inherit,
    };

    let request = ImportedRequest {
        name,
        method: method.to_uppercase(),
        url: join_url(ctx.base_url, path),
        headers,
        query,
        body,
        options: RequestOptions::default(),
        auth,
        pre_request_script: String::new(),
        post_response_script: String::new(),
        ..Default::default()
    };
    Operation { tag, request }
}

fn join_url(base: &str, path: &str) -> String {
    let path = untemplatize_path(path);
    if base.is_empty() {
        path
    } else {
        format!("{}{}", base.trim_end_matches('/'), path)
    }
}

/// Inverse of `templatize_path` (export side): OpenAPI's `{name}` path
/// templates become this app's `{{name}}` variable syntax, so an imported
/// request's URL is actually substitutable rather than carrying literal
/// braces, and so parse -> export -> parse round-trips back to `{name}`.
fn untemplatize_path(path: &str) -> String {
    let mut out = String::new();
    let mut rest = path;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        if let Some(end) = after.find('}') {
            out.push_str("{{");
            out.push_str(&after[..end]);
            out.push_str("}}");
            rest = &after[end + 1..];
        } else {
            out.push_str(&rest[start..]);
            rest = "";
            break;
        }
    }
    out.push_str(rest);
    out
}

fn param_example(p: &Value) -> String {
    if let Some(s) = p.get("example") {
        return value_to_plain_string(s);
    }
    if let Some(schema) = p.get("schema") {
        if let Some(s) = schema.get("example") {
            return value_to_plain_string(s);
        }
        if let Some(d) = schema.get("default") {
            return value_to_plain_string(d);
        }
    }
    String::new()
}

fn value_to_plain_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Request body (3.0) — `requestBody.content`, keyed by media type
// ---------------------------------------------------------------------------

fn build_request_body_v3(root: &Value, rb: &Value, warnings: &mut Vec<String>) -> RequestBody {
    let rb = resolve_ref(root, rb, warnings);
    let Some(content) = rb.get("content").and_then(Value::as_object) else { return RequestBody::None };

    if let Some(mt) = content.get("application/json") {
        return RequestBody::Json(media_example_json(root, mt, warnings));
    }
    if let Some(mt) = content.get("application/x-www-form-urlencoded") {
        return RequestBody::UrlEncoded(schema_properties_to_kv(root, mt.get("schema"), warnings));
    }
    if let Some(mt) = content.get("multipart/form-data") {
        return RequestBody::FormData(schema_properties_to_form(root, mt.get("schema"), warnings));
    }
    if let Some((mime, mt)) = content.iter().next() {
        let language = if mime.contains("xml") {
            Some("xml".to_string())
        } else if mime.contains("json") {
            Some("json".to_string())
        } else {
            None
        };
        return RequestBody::Raw { content: media_example_raw(root, mt, warnings), language };
    }
    RequestBody::None
}

fn media_example_value(root: &Value, mt: &Value, warnings: &mut Vec<String>) -> Value {
    if let Some(ex) = mt.get("example") {
        return ex.clone();
    }
    if let Some(examples) = mt.get("examples").and_then(Value::as_object) {
        if let Some((_, first)) = examples.iter().next() {
            let first = resolve_ref(root, first, warnings);
            if let Some(v) = first.get("value") {
                return v.clone();
            }
        }
    }
    if let Some(schema) = mt.get("schema") {
        return schema_to_example(root, schema, warnings, 0);
    }
    Value::Null
}

fn media_example_json(root: &Value, mt: &Value, warnings: &mut Vec<String>) -> String {
    serde_json::to_string_pretty(&media_example_value(root, mt, warnings)).unwrap_or_default()
}

fn media_example_raw(root: &Value, mt: &Value, warnings: &mut Vec<String>) -> String {
    match media_example_value(root, mt, warnings) {
        Value::String(s) => s,
        Value::Null => String::new(),
        other => serde_json::to_string_pretty(&other).unwrap_or_default(),
    }
}

fn schema_to_example(root: &Value, schema: &Value, warnings: &mut Vec<String>, depth: u8) -> Value {
    if depth > 6 {
        return Value::Null;
    }
    let schema = resolve_ref(root, schema, warnings);
    if let Some(ex) = schema.get("example") {
        return ex.clone();
    }
    if let Some(d) = schema.get("default") {
        return d.clone();
    }
    if let Some(all_of) = schema.get("allOf").and_then(Value::as_array) {
        let mut merged = serde_json::Map::new();
        for sub in all_of {
            if let Value::Object(m) = schema_to_example(root, sub, warnings, depth + 1) {
                merged.extend(m);
            }
        }
        return Value::Object(merged);
    }
    match schema.get("type").and_then(Value::as_str) {
        Some("array") => {
            let item = schema.get("items").map(|i| schema_to_example(root, i, warnings, depth + 1)).unwrap_or(Value::Null);
            Value::Array(vec![item])
        }
        Some("string") => {
            Value::String(schema.get("enum").and_then(Value::as_array).and_then(|a| a.first()).and_then(Value::as_str).unwrap_or("string").to_string())
        }
        Some("integer") => json!(0),
        Some("number") => json!(0.0),
        Some("boolean") => Value::Bool(true),
        Some("object") => object_example(root, &schema, warnings, depth),
        None if schema.get("properties").is_some() => object_example(root, &schema, warnings, depth),
        _ => Value::Null,
    }
}

fn object_example(root: &Value, schema: &Value, warnings: &mut Vec<String>, depth: u8) -> Value {
    let mut m = serde_json::Map::new();
    if let Some(props) = schema.get("properties").and_then(Value::as_object) {
        for (k, v) in props {
            m.insert(k.clone(), schema_to_example(root, v, warnings, depth + 1));
        }
    }
    Value::Object(m)
}

fn schema_properties_to_kv(root: &Value, schema: Option<&Value>, warnings: &mut Vec<String>) -> Vec<KeyValue> {
    let Some(schema) = schema else { return Vec::new() };
    let schema = resolve_ref(root, schema, warnings);
    let Some(props) = schema.get("properties").and_then(Value::as_object) else { return Vec::new() };
    props
        .iter()
        .map(|(k, v)| KeyValue { key: k.clone(), value: value_to_plain_string(&schema_to_example(root, v, warnings, 0)), enabled: true })
        .collect()
}

fn schema_properties_to_form(root: &Value, schema: Option<&Value>, warnings: &mut Vec<String>) -> Vec<FormField> {
    let Some(schema) = schema else { return Vec::new() };
    let schema = resolve_ref(root, schema, warnings);
    let Some(props) = schema.get("properties").and_then(Value::as_object) else { return Vec::new() };
    props
        .iter()
        .map(|(k, v)| {
            let v = resolve_ref(root, v, warnings);
            let is_file = v.get("format").and_then(Value::as_str) == Some("binary");
            let value = if is_file { String::new() } else { value_to_plain_string(&schema_to_example(root, &v, warnings, 0)) };
            FormField { key: k.clone(), value, enabled: true, is_file, content_type: None }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Security schemes -> AuthConfig (import) and back (export)
// ---------------------------------------------------------------------------

fn resolve_security(security: &[Value], schemes: &Value, warnings: &mut Vec<String>) -> AuthConfig {
    let Some(first) = security.first() else { return AuthConfig::None };
    let Some(req) = first.as_object() else { return AuthConfig::None };
    let Some((scheme_name, scopes)) = req.iter().next() else { return AuthConfig::None };
    let Some(scheme) = schemes.get(scheme_name) else {
        warnings.push(format!("security scheme \"{scheme_name}\" is not defined in components.securitySchemes — skipped"));
        return AuthConfig::None;
    };
    let scopes: Vec<String> = scopes.as_array().map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default();
    security_scheme_to_auth(scheme_name, scheme, &scopes, warnings)
}

fn security_scheme_to_auth(name: &str, scheme: &Value, scopes: &[String], warnings: &mut Vec<String>) -> AuthConfig {
    match scheme.get("type").and_then(Value::as_str) {
        Some("http") => match scheme.get("scheme").and_then(Value::as_str) {
            Some("bearer") => AuthConfig::Bearer { token: String::new(), prefix: crate::model::auth::default_bearer_prefix() },
            Some("basic") => AuthConfig::Basic { username: String::new(), password: String::new() },
            Some(other) => {
                warnings.push(format!("security scheme \"{name}\": unsupported http scheme \"{other}\" — imported as No Auth"));
                AuthConfig::None
            }
            None => AuthConfig::None,
        },
        Some("apiKey") => {
            let key = scheme.get("name").and_then(Value::as_str).unwrap_or_default().to_string();
            match scheme.get("in").and_then(Value::as_str) {
                Some("query") => AuthConfig::ApiKey { key, value: String::new(), location: ApiKeyLocation::Query },
                Some("cookie") => {
                    warnings.push(format!("security scheme \"{name}\": apiKey in \"cookie\" is not supported — imported as header"));
                    AuthConfig::ApiKey { key, value: String::new(), location: ApiKeyLocation::Header }
                }
                _ => AuthConfig::ApiKey { key, value: String::new(), location: ApiKeyLocation::Header },
            }
        }
        Some("oauth2") => oauth2_from_flows(name, scheme.get("flows"), scopes, warnings),
        Some("openIdConnect") => {
            warnings.push(format!("security scheme \"{name}\": openIdConnect is not supported — imported as No Auth"));
            AuthConfig::None
        }
        Some(other) => {
            warnings.push(format!("security scheme \"{name}\": unsupported type \"{other}\" — imported as No Auth"));
            AuthConfig::None
        }
        None => AuthConfig::None,
    }
}

fn oauth2_from_flows(name: &str, flows: Option<&Value>, scopes: &[String], warnings: &mut Vec<String>) -> AuthConfig {
    let Some(flows) = flows else { return AuthConfig::None };
    let scope_str = scopes.join(" ");
    if let Some(f) = flows.get("authorizationCode") {
        return AuthConfig::OAuth2(OAuth2Config {
            grant_type: OAuth2GrantType::AuthorizationCode,
            auth_url: f.get("authorizationUrl").and_then(Value::as_str).unwrap_or_default().to_string(),
            token_url: f.get("tokenUrl").and_then(Value::as_str).unwrap_or_default().to_string(),
            scope: flow_scope(f, &scope_str),
            ..Default::default()
        });
    }
    if let Some(f) = flows.get("clientCredentials") {
        return AuthConfig::OAuth2(OAuth2Config {
            grant_type: OAuth2GrantType::ClientCredentials,
            token_url: f.get("tokenUrl").and_then(Value::as_str).unwrap_or_default().to_string(),
            scope: flow_scope(f, &scope_str),
            ..Default::default()
        });
    }
    if let Some(f) = flows.get("password") {
        return AuthConfig::OAuth2(OAuth2Config {
            grant_type: OAuth2GrantType::Password,
            token_url: f.get("tokenUrl").and_then(Value::as_str).unwrap_or_default().to_string(),
            scope: flow_scope(f, &scope_str),
            ..Default::default()
        });
    }
    if flows.get("implicit").is_some() {
        // No `OAuth2GrantType` variant represents "implicit" (it has no token
        // endpoint — the browser redirect carries the token directly), so
        // synthesizing any grant type here would misrepresent the scheme
        // rather than just leaving a documented gap.
        warnings.push(format!("security scheme \"{name}\": OAuth2 implicit flow is not supported — imported as No Auth (configure manually)"));
        return AuthConfig::None;
    }
    AuthConfig::None
}

fn flow_scope(f: &Value, fallback: &str) -> String {
    if !fallback.is_empty() {
        return fallback.to_string();
    }
    f.get("scopes").and_then(Value::as_object).map(|m| m.keys().cloned().collect::<Vec<_>>().join(" ")).unwrap_or_default()
}

/// Export-side mirror of `security_scheme_to_auth`: registers `cfg`'s scheme
/// under a fixed canonical name in `schemes` (idempotent — every config of
/// the same kind anywhere in the tree shares one definition, since secrets
/// are already masked before export and so never differentiate the scheme
/// shape) and returns the security *requirement* object referencing it.
fn security_ref_for(cfg: &AuthConfig, schemes: &mut serde_json::Map<String, Value>) -> Option<Value> {
    let (name, scheme_json, scopes): (&str, Value, Vec<Value>) = match cfg {
        AuthConfig::None => return None,
        AuthConfig::Bearer { .. } => ("bearerAuth", json!({"type": "http", "scheme": "bearer"}), vec![]),
        AuthConfig::Basic { .. } => ("basicAuth", json!({"type": "http", "scheme": "basic"}), vec![]),
        AuthConfig::ApiKey { key, location, .. } => (
            "apiKeyAuth",
            json!({"type": "apiKey", "name": key, "in": if *location == ApiKeyLocation::Query { "query" } else { "header" }}),
            vec![],
        ),
        AuthConfig::OAuth2(c) => {
            let scope_names: Vec<Value> = c.scope.split_whitespace().map(|s| Value::String(s.to_string())).collect();
            let mut scopes_map = serde_json::Map::new();
            for s in c.scope.split_whitespace() {
                scopes_map.insert(s.to_string(), Value::String(String::new()));
            }
            // OpenAPI's OAuth Flows Object has exactly 4 named flows and no
            // "refresh" flow — refresh is an operational detail of token
            // exchange, not a distinct authorization flow, so RefreshToken
            // maps to its closest representable equivalent.
            let flow_key = match c.grant_type {
                OAuth2GrantType::AuthorizationCode => "authorizationCode",
                OAuth2GrantType::ClientCredentials | OAuth2GrantType::RefreshToken => "clientCredentials",
                OAuth2GrantType::Password => "password",
            };
            let mut flow = serde_json::Map::new();
            if matches!(c.grant_type, OAuth2GrantType::AuthorizationCode) {
                flow.insert("authorizationUrl".into(), Value::String(c.auth_url.clone()));
            }
            flow.insert("tokenUrl".into(), Value::String(c.token_url.clone()));
            flow.insert("scopes".into(), Value::Object(scopes_map));
            ("oauth2Auth", json!({"type": "oauth2", "flows": {flow_key: Value::Object(flow)}}), scope_names)
        }
        // OpenAPI 3.0 core has no AWS SigV4 scheme type — there is nothing
        // valid to emit, so this auth is dropped silently on export, same as
        // every other format-inherent gap here (export never warns, by
        // existing convention — see `postman::export`).
        AuthConfig::AwsSigV4(_) => return None,
    };
    schemes.entry(name.to_string()).or_insert(scheme_json);
    Some(json!({name: scopes}))
}

// ---------------------------------------------------------------------------
// Swagger 2.0 import (import-only — export always emits OpenAPI 3.0, see
// module doc). Reuses `resolve_ref`/`schema_to_example`/`group_by_tag`/
// `join_url`/`untemplatize_path` as-is: JSON Pointers, JSON-Schema example
// synthesis, tag-grouping, and path-templating all work identically in 2.0.
// What differs and gets its own handling below: base-URL assembly
// (`host`+`basePath`+`schemes` instead of `servers`), flat (un-nested)
// parameter `type`/`default` instead of a nested `schema`, a single `in:
// "body"` parameter instead of `requestBody`, `in: "formData"` parameters
// instead of multipart/urlencoded content, and `securityDefinitions`'
// flatter scheme shapes (no `http`+`scheme` nesting, a single `flow` string
// instead of a `flows` object, no `cookie` location, no `openIdConnect`).
// ---------------------------------------------------------------------------

struct ContextV2<'a> {
    root: &'a Value,
    base_url: &'a str,
    security_schemes: &'a Value,
    doc_consumes: &'a [String],
}

fn parse_swagger2(root: &Value, warnings: &mut Vec<String>) -> AppResult<ImportedNode> {
    let info = root.get("info").ok_or_else(|| AppError::Other("not a Swagger document: missing \"info\" object".into()))?;
    let name = info.get("title").and_then(Value::as_str).unwrap_or("Imported API").to_string();
    let description = info.get("description").and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string);

    let scheme = root.get("schemes").and_then(Value::as_array).and_then(|a| a.first()).and_then(Value::as_str).unwrap_or("https");
    let host = root.get("host").and_then(Value::as_str).unwrap_or("");
    let base_path = root.get("basePath").and_then(Value::as_str).unwrap_or("");
    let base_url = if host.is_empty() { String::new() } else { format!("{scheme}://{host}{base_path}") };

    let security_schemes = root.get("securityDefinitions").cloned().unwrap_or(Value::Null);
    let global_security = root.get("security").and_then(Value::as_array).cloned().unwrap_or_default();
    let root_auth = resolve_security_v2(&global_security, &security_schemes, warnings);
    let doc_consumes: Vec<String> =
        root.get("consumes").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default();

    let ctx = ContextV2 { root, base_url: &base_url, security_schemes: &security_schemes, doc_consumes: &doc_consumes };
    let mut ops = Vec::new();
    if let Some(paths) = root.get("paths").and_then(Value::as_object) {
        for (path, path_item) in paths {
            if path_item.get("$ref").is_some() {
                warnings.push(format!("path \"{path}\": $ref path items are not supported — skipped"));
                continue;
            }
            let path_level_params: Vec<Value> = path_item.get("parameters").and_then(Value::as_array).cloned().unwrap_or_default();
            // No "trace" method in Swagger 2.0 (added later, in OpenAPI 3.0).
            for method in ["get", "put", "post", "delete", "options", "head", "patch"] {
                let Some(op) = path_item.get(method) else { continue };
                ops.push(build_operation_v2(&ctx, path, method, op, &path_level_params, warnings));
            }
        }
    }

    Ok(group_by_tag(ops, name, description, root_auth))
}

fn build_operation_v2(
    ctx: &ContextV2,
    path: &str,
    method: &str,
    op: &Value,
    path_level_params: &[Value],
    warnings: &mut Vec<String>,
) -> Operation {
    let name = op.get("summary").and_then(Value::as_str).or_else(|| op.get("operationId").and_then(Value::as_str)).unwrap_or(path).to_string();
    let tag = op.get("tags").and_then(Value::as_array).and_then(|a| a.first()).and_then(Value::as_str).map(str::to_string);

    let mut params: Vec<Value> = path_level_params.iter().map(|p| resolve_ref(ctx.root, p, warnings)).collect();
    if let Some(op_params) = op.get("parameters").and_then(Value::as_array) {
        params.extend(op_params.iter().map(|p| resolve_ref(ctx.root, p, warnings)));
    }

    let consumes: Vec<String> = op
        .get("consumes")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_else(|| ctx.doc_consumes.to_vec());
    let is_multipart = consumes.iter().any(|c| c == "multipart/form-data");

    let mut headers = Vec::new();
    let mut query = Vec::new();
    let mut form_fields = Vec::new();
    let mut body = RequestBody::None;
    for p in &params {
        let pname = p.get("name").and_then(Value::as_str).unwrap_or_default().to_string();
        if pname.is_empty() {
            continue;
        }
        match p.get("in").and_then(Value::as_str) {
            Some("query") => query.push(KeyValue { key: pname, value: param_example_v2(p), enabled: true }),
            Some("header") => headers.push(HeaderEntry { name: pname, value: param_example_v2(p), enabled: true }),
            Some("body") => {
                if let Some(schema) = p.get("schema") {
                    body = RequestBody::Json(serde_json::to_string_pretty(&schema_to_example(ctx.root, schema, warnings, 0)).unwrap_or_default());
                }
            }
            Some("formData") => {
                let is_file = p.get("type").and_then(Value::as_str) == Some("file");
                let value = if is_file { String::new() } else { param_example_v2(p) };
                form_fields.push(FormField { key: pname, value, enabled: true, is_file, content_type: None });
            }
            _ => {} // "path" is already satisfied by the literal {name} segment in the URL
        }
    }
    if !form_fields.is_empty() {
        body = if is_multipart {
            RequestBody::FormData(form_fields)
        } else {
            RequestBody::UrlEncoded(form_fields.into_iter().map(|f| KeyValue { key: f.key, value: f.value, enabled: f.enabled }).collect())
        };
    }

    let auth = match op.get("security") {
        Some(Value::Array(arr)) => RequestAuth::Own(resolve_security_v2(arr, ctx.security_schemes, warnings)),
        _ => RequestAuth::Inherit,
    };

    let request = ImportedRequest {
        name,
        method: method.to_uppercase(),
        url: join_url(ctx.base_url, path),
        headers,
        query,
        body,
        options: RequestOptions::default(),
        auth,
        pre_request_script: String::new(),
        post_response_script: String::new(),
        ..Default::default()
    };
    Operation { tag, request }
}

/// Swagger 2.0 parameters carry their type info flat (`type`/`default`
/// directly on the Parameter Object), not nested under a `schema` like 3.0 —
/// and have no parameter-level `example` field at all, only `default`.
fn param_example_v2(p: &Value) -> String {
    p.get("default").map(value_to_plain_string).unwrap_or_default()
}

fn resolve_security_v2(security: &[Value], schemes: &Value, warnings: &mut Vec<String>) -> AuthConfig {
    let Some(first) = security.first() else { return AuthConfig::None };
    let Some(req) = first.as_object() else { return AuthConfig::None };
    let Some((scheme_name, scopes)) = req.iter().next() else { return AuthConfig::None };
    let Some(scheme) = schemes.get(scheme_name) else {
        warnings.push(format!("security scheme \"{scheme_name}\" is not defined in securityDefinitions — skipped"));
        return AuthConfig::None;
    };
    let scopes: Vec<String> = scopes.as_array().map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default();
    security_scheme_to_auth_v2(scheme_name, scheme, &scopes, warnings)
}

fn security_scheme_to_auth_v2(name: &str, scheme: &Value, scopes: &[String], warnings: &mut Vec<String>) -> AuthConfig {
    match scheme.get("type").and_then(Value::as_str) {
        Some("basic") => AuthConfig::Basic { username: String::new(), password: String::new() },
        Some("apiKey") => {
            let key = scheme.get("name").and_then(Value::as_str).unwrap_or_default().to_string();
            match scheme.get("in").and_then(Value::as_str) {
                Some("query") => AuthConfig::ApiKey { key, value: String::new(), location: ApiKeyLocation::Query },
                _ => AuthConfig::ApiKey { key, value: String::new(), location: ApiKeyLocation::Header },
            }
        }
        Some("oauth2") => {
            let token_url = scheme.get("tokenUrl").and_then(Value::as_str).unwrap_or_default().to_string();
            let auth_url = scheme.get("authorizationUrl").and_then(Value::as_str).unwrap_or_default().to_string();
            // Swagger 2.0's `scopes` map sits flat on the scheme itself
            // (there's no per-flow nesting like 3.0's `flows.X.scopes`), so
            // `flow_scope` — written for 3.0's flow objects — applies as-is.
            let scope = flow_scope(scheme, &scopes.join(" "));
            match scheme.get("flow").and_then(Value::as_str) {
                Some("accessCode") => {
                    AuthConfig::OAuth2(OAuth2Config { grant_type: OAuth2GrantType::AuthorizationCode, auth_url, token_url, scope, ..Default::default() })
                }
                Some("application") => AuthConfig::OAuth2(OAuth2Config { grant_type: OAuth2GrantType::ClientCredentials, token_url, scope, ..Default::default() }),
                Some("password") => AuthConfig::OAuth2(OAuth2Config { grant_type: OAuth2GrantType::Password, token_url, scope, ..Default::default() }),
                Some("implicit") => {
                    warnings.push(format!("security scheme \"{name}\": OAuth2 implicit flow is not supported — imported as No Auth (configure manually)"));
                    AuthConfig::None
                }
                _ => AuthConfig::None,
            }
        }
        Some(other) => {
            warnings.push(format!("security scheme \"{name}\": unsupported type \"{other}\" — imported as No Auth"));
            AuthConfig::None
        }
        None => AuthConfig::None,
    }
}

// ---------------------------------------------------------------------------
// OpenAPI 3.0 export
// ---------------------------------------------------------------------------

fn collect_paths(
    node: &ImportedNode,
    tag: Option<&str>,
    root_auth: &AuthConfig,
    inherited_auth: &AuthConfig,
    paths: &mut serde_json::Map<String, Value>,
    first_origin: &mut Option<String>,
    schemes: &mut serde_json::Map<String, Value>,
) {
    for req in &node.requests {
        let (origin, raw_path) = split_origin_and_path(&req.url);
        if first_origin.is_none() && !origin.is_empty() {
            *first_origin = Some(origin);
        }
        let (mut path_key, path_params) = templatize_path(&raw_path);
        if path_key.is_empty() {
            path_key = "/".to_string();
        }
        let effective_auth = match &req.auth {
            RequestAuth::Own(cfg) => cfg,
            RequestAuth::Inherit => inherited_auth,
        };
        // OpenAPI has no folder/tag-scoped security equivalent, so a folder
        // that overrides auth would be silently dropped unless its effective
        // auth is baked into each contained operation directly. Only emit an
        // explicit override when it actually differs from the document-global
        // default, to keep untouched operations inheriting it for free.
        let auth_override = if effective_auth == root_auth { None } else { Some(effective_auth) };
        let entry = paths.entry(path_key).or_insert_with(|| Value::Object(Default::default()));
        if let Value::Object(path_item) = entry {
            path_item.insert(req.method.to_lowercase(), build_operation_json(req, &path_params, tag, auth_override, schemes));
        }
    }
    for child in &node.children {
        let next_inherited = if child.auth == AuthConfig::None { inherited_auth } else { &child.auth };
        collect_paths(child, Some(&child.name), root_auth, next_inherited, paths, first_origin, schemes);
    }
}

/// Splits a full URL into `(origin, path)` so that only the path half is
/// ever scanned for `{{var}}` placeholders to templatize — a host-only
/// variable like `{{baseUrl}}` must never turn into an OpenAPI path
/// parameter.
fn split_origin_and_path(url: &str) -> (String, String) {
    if let Some(scheme_end) = url.find("://") {
        let after_scheme = scheme_end + 3;
        match url[after_scheme..].find('/') {
            Some(slash) => (url[..after_scheme + slash].to_string(), url[after_scheme + slash..].to_string()),
            None => (url.to_string(), "/".to_string()),
        }
    } else if let Some(slash) = url.find('/') {
        (url[..slash].to_string(), url[slash..].to_string())
    } else {
        (url.to_string(), "/".to_string())
    }
}

fn templatize_path(path: &str) -> (String, Vec<String>) {
    let mut out = String::new();
    let mut names = Vec::new();
    let mut rest = path;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        if let Some(end) = after.find("}}") {
            let name = after[..end].trim().to_string();
            out.push('{');
            out.push_str(&name);
            out.push('}');
            names.push(name);
            rest = &after[end + 2..];
        } else {
            out.push_str(&rest[start..]);
            rest = "";
            break;
        }
    }
    out.push_str(rest);
    (out, names)
}

fn build_operation_json(
    req: &ImportedRequest,
    path_params: &[String],
    tag: Option<&str>,
    auth_override: Option<&AuthConfig>,
    schemes: &mut serde_json::Map<String, Value>,
) -> Value {
    let mut op = serde_json::Map::new();
    if let Some(tag) = tag {
        op.insert("tags".into(), json!([tag]));
    }
    op.insert("summary".into(), Value::String(req.name.clone()));

    let mut params: Vec<Value> =
        path_params.iter().map(|name| json!({"name": name, "in": "path", "required": true, "schema": {"type": "string"}})).collect();
    for q in req.query.iter().filter(|q| q.enabled) {
        params.push(json!({"name": q.key, "in": "query", "required": false, "schema": {"type": "string", "example": q.value}}));
    }
    for h in req.headers.iter().filter(|h| h.enabled) {
        params.push(json!({"name": h.name, "in": "header", "required": false, "schema": {"type": "string", "example": h.value}}));
    }
    if !params.is_empty() {
        op.insert("parameters".into(), Value::Array(params));
    }

    if let Some(rb) = build_request_body_json(&req.body) {
        op.insert("requestBody".into(), rb);
    }

    if let Some(cfg) = auth_override {
        let security = security_ref_for(cfg, schemes).map(|s| vec![s]).unwrap_or_default();
        op.insert("security".into(), Value::Array(security));
    }

    // `responses` is the one field the Operation Object actually requires.
    op.insert("responses".into(), json!({"default": {"description": "Response"}}));
    Value::Object(op)
}

fn build_request_body_json(body: &RequestBody) -> Option<Value> {
    let media = match body {
        RequestBody::None => return None,
        RequestBody::Json(content) => {
            let example = serde_json::from_str::<Value>(content).unwrap_or_else(|_| Value::String(content.clone()));
            json!({"application/json": {"schema": {"type": "object"}, "example": example}})
        }
        RequestBody::Raw { content, language } => {
            let mime = match language.as_deref() {
                Some("xml") => "application/xml",
                Some("html") => "text/html",
                _ => "text/plain",
            };
            json!({mime: {"schema": {"type": "string"}, "example": content}})
        }
        RequestBody::UrlEncoded(list) => {
            let props: serde_json::Map<String, Value> = list.iter().map(|kv| (kv.key.clone(), json!({"type": "string", "example": kv.value}))).collect();
            json!({"application/x-www-form-urlencoded": {"schema": {"type": "object", "properties": props}}})
        }
        RequestBody::FormData(list) => {
            let props: serde_json::Map<String, Value> = list
                .iter()
                .map(|f| {
                    let schema = if f.is_file { json!({"type": "string", "format": "binary"}) } else { json!({"type": "string", "example": f.value}) };
                    (f.key.clone(), schema)
                })
                .collect();
            json!({"multipart/form-data": {"schema": {"type": "object", "properties": props}}})
        }
        RequestBody::Binary { .. } => json!({"application/octet-stream": {"schema": {"type": "string", "format": "binary"}}}),
        RequestBody::Graphql { query, variables, operation_name } => {
            let mut obj = serde_json::Map::new();
            obj.insert("query".into(), Value::String(query.clone()));
            if let Some(v) = variables {
                if let Ok(parsed) = serde_json::from_str::<Value>(v) {
                    obj.insert("variables".into(), parsed);
                }
            }
            if let Some(name) = operation_name {
                if !name.trim().is_empty() {
                    obj.insert("operationName".into(), Value::String(name.clone()));
                }
            }
            json!({"application/json": {"schema": {"type": "object"}, "example": Value::Object(obj)}})
        }
    };
    Some(json!({"content": media, "required": true}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    const PETSTORE_FIXTURE: &str = r##"{
        "openapi": "3.0.3",
        "info": { "title": "Petstore", "description": "Sample pet store API", "version": "1.0.0" },
        "servers": [{ "url": "https://petstore.example.com" }],
        "security": [{ "bearerAuth": [] }],
        "components": {
            "securitySchemes": {
                "bearerAuth": { "type": "http", "scheme": "bearer" },
                "basicAuth": { "type": "http", "scheme": "basic" }
            },
            "schemas": {
                "Pet": {
                    "type": "object",
                    "properties": { "name": { "type": "string", "example": "Fido" } }
                }
            },
            "requestBodies": {
                "PetBody": {
                    "content": {
                        "application/json": { "schema": { "$ref": "#/components/schemas/Pet" } }
                    }
                }
            },
            "parameters": {
                "VerboseParam": {
                    "name": "verbose",
                    "in": "query",
                    "schema": { "type": "string", "example": "true" }
                }
            }
        },
        "paths": {
            "/pets/{petId}": {
                "parameters": [
                    { "name": "petId", "in": "path", "required": true, "schema": { "type": "string" } }
                ],
                "get": {
                    "tags": ["Pets"],
                    "summary": "Get pet by id",
                    "parameters": [{ "$ref": "#/components/parameters/VerboseParam" }],
                    "responses": { "200": { "description": "ok" } }
                },
                "delete": {
                    "tags": ["Pets"],
                    "summary": "Delete pet",
                    "security": [],
                    "responses": { "204": { "description": "deleted" } }
                }
            },
            "/pets": {
                "post": {
                    "tags": ["Pets"],
                    "summary": "Create pet",
                    "requestBody": { "$ref": "#/components/requestBodies/PetBody" },
                    "security": [{ "basicAuth": [] }],
                    "responses": { "201": { "description": "created" } }
                }
            },
            "/pets/{petId}/photo": {
                "post": {
                    "summary": "Upload photo",
                    "requestBody": {
                        "content": {
                            "multipart/form-data": {
                                "schema": {
                                    "type": "object",
                                    "properties": {
                                        "caption": { "type": "string", "example": "cute dog" },
                                        "photo": { "type": "string", "format": "binary" }
                                    }
                                }
                            }
                        }
                    },
                    "responses": { "default": { "description": "ok" } }
                }
            },
            "/login": {
                "post": {
                    "summary": "Login",
                    "requestBody": {
                        "content": {
                            "application/x-www-form-urlencoded": {
                                "schema": {
                                    "type": "object",
                                    "properties": { "user": { "type": "string", "example": "alice" } }
                                }
                            }
                        }
                    },
                    "responses": { "default": { "description": "ok" } }
                }
            },
            "/unknown-scheme": {
                "get": {
                    "summary": "Uses an undefined scheme",
                    "security": [{ "missingScheme": [] }],
                    "responses": { "default": { "description": "ok" } }
                }
            }
        }
    }"##;

    fn all_requests<'a>(node: &'a ImportedNode, out: &mut Vec<&'a ImportedRequest>) {
        out.extend(node.requests.iter());
        for child in &node.children {
            all_requests(child, out);
        }
    }

    fn find_request<'a>(node: &'a ImportedNode, name: &str) -> &'a ImportedRequest {
        let mut all = Vec::new();
        all_requests(node, &mut all);
        all.into_iter().find(|r| r.name == name).unwrap_or_else(|| panic!("no request named {name:?}"))
    }

    #[test]
    fn imports_realistic_openapi_fixture_with_expected_shape_and_warnings() {
        let preview = parse(PETSTORE_FIXTURE).unwrap();
        assert_eq!(preview.root.name, "Petstore");
        assert_eq!(preview.root.description, Some("Sample pet store API".to_string()));
        assert_eq!(preview.root.auth, AuthConfig::Bearer { token: String::new(), prefix: crate::model::auth::default_bearer_prefix() });

        assert_eq!(preview.stats.requests, 6);
        assert_eq!(preview.stats.folders, 1);
        assert_eq!(preview.root.children.len(), 1);
        let pets = &preview.root.children[0];
        assert_eq!(pets.name, "Pets");
        assert_eq!(pets.requests.len(), 3);
        assert_eq!(preview.root.requests.len(), 3);

        let get_pet = find_request(&preview.root, "Get pet by id");
        assert_eq!(get_pet.method, "GET");
        assert_eq!(get_pet.url, "https://petstore.example.com/pets/{{petId}}");
        assert_eq!(get_pet.auth, RequestAuth::Inherit);
        assert_eq!(get_pet.query, vec![KeyValue { key: "verbose".into(), value: "true".into(), enabled: true }]);

        let delete_pet = find_request(&preview.root, "Delete pet");
        assert_eq!(delete_pet.method, "DELETE");
        assert_eq!(delete_pet.auth, RequestAuth::Own(AuthConfig::None));

        let create_pet = find_request(&preview.root, "Create pet");
        assert_eq!(create_pet.method, "POST");
        assert_eq!(create_pet.url, "https://petstore.example.com/pets");
        assert_eq!(create_pet.auth, RequestAuth::Own(AuthConfig::Basic { username: String::new(), password: String::new() }));
        match &create_pet.body {
            RequestBody::Json(s) => assert_eq!(serde_json::from_str::<Value>(s).unwrap(), json!({"name": "Fido"})),
            other => panic!("expected Json body, got {other:?}"),
        }

        let photo = find_request(&preview.root, "Upload photo");
        assert_eq!(photo.url, "https://petstore.example.com/pets/{{petId}}/photo");
        match &photo.body {
            RequestBody::FormData(fields) => {
                let caption = fields.iter().find(|f| f.key == "caption").unwrap();
                assert_eq!(caption.value, "cute dog");
                assert!(!caption.is_file);
                let file = fields.iter().find(|f| f.key == "photo").unwrap();
                assert!(file.is_file);
                assert_eq!(file.value, "");
            }
            other => panic!("expected FormData body, got {other:?}"),
        }

        let login = find_request(&preview.root, "Login");
        match &login.body {
            RequestBody::UrlEncoded(fields) => {
                assert_eq!(fields, &vec![KeyValue { key: "user".into(), value: "alice".into(), enabled: true }]);
            }
            other => panic!("expected UrlEncoded body, got {other:?}"),
        }

        let unknown = find_request(&preview.root, "Uses an undefined scheme");
        assert_eq!(unknown.auth, RequestAuth::Own(AuthConfig::None));
        assert!(preview.warnings.iter().any(|w| w.contains("missingScheme")), "{:?}", preview.warnings);
    }

    #[test]
    fn openapi_json_round_trips_through_model_twice() {
        let preview_a = parse(PETSTORE_FIXTURE).unwrap();
        let exported = export(&preview_a.root).unwrap();
        let preview_b = parse(&exported).unwrap();

        let mut a = Vec::new();
        all_requests(&preview_a.root, &mut a);
        let mut b = Vec::new();
        all_requests(&preview_b.root, &mut b);

        // Lossy by nature (example-value synthesis, only-first-security
        // alternative honored, etc.) — assert on what's expected to survive
        // exactly: every request's method+url, and the tag-derived folders.
        let a_set: BTreeSet<_> = a.iter().map(|r| (r.method.clone(), r.url.clone())).collect();
        let b_set: BTreeSet<_> = b.iter().map(|r| (r.method.clone(), r.url.clone())).collect();
        assert_eq!(a_set, b_set);

        let a_folders: BTreeSet<_> = preview_a.root.children.iter().map(|c| c.name.clone()).collect();
        let b_folders: BTreeSet<_> = preview_b.root.children.iter().map(|c| c.name.clone()).collect();
        assert_eq!(a_folders, b_folders);
    }

    #[test]
    fn yaml_input_parses_to_the_same_shape_as_equivalent_json() {
        let yaml = "openapi: 3.0.0\ninfo:\n  title: X\n  version: \"1.0\"\npaths:\n  /ping:\n    get:\n      summary: Ping\n      responses:\n        default:\n          description: ok\n";
        let preview = parse(yaml).unwrap();
        assert_eq!(preview.root.name, "X");
        assert_eq!(preview.root.requests.len(), 1);
        assert_eq!(preview.root.requests[0].name, "Ping");
        assert_eq!(preview.root.requests[0].method, "GET");
        assert_eq!(preview.root.requests[0].url, "/ping");
    }

    #[test]
    fn cookie_parameter_and_open_id_connect_scheme_are_warned_and_skipped() {
        let spec = r#"{
            "openapi": "3.0.0",
            "info": {"title": "X", "version": "1.0"},
            "components": {
                "securitySchemes": {
                    "sso": {"type": "openIdConnect", "openIdConnectUrl": "https://example.com/.well-known"}
                }
            },
            "security": [{"sso": []}],
            "paths": {
                "/x": {
                    "get": {
                        "parameters": [{"name": "session", "in": "cookie", "schema": {"type": "string"}}],
                        "responses": {"default": {"description": "ok"}}
                    }
                }
            }
        }"#;
        let preview = parse(spec).unwrap();
        assert_eq!(preview.root.auth, AuthConfig::None);
        assert_eq!(preview.root.requests[0].headers.len(), 0);
        assert_eq!(preview.root.requests[0].query.len(), 0);
        assert!(preview.warnings.iter().any(|w| w.contains("openIdConnect")), "{:?}", preview.warnings);
        assert!(preview.warnings.iter().any(|w| w.contains("cookie")), "{:?}", preview.warnings);
    }

    #[test]
    fn api_key_in_cookie_falls_back_to_header_with_a_warning() {
        let spec = r#"{
            "openapi": "3.0.0",
            "info": {"title": "X", "version": "1.0"},
            "components": {
                "securitySchemes": {
                    "session": {"type": "apiKey", "in": "cookie", "name": "sid"}
                }
            },
            "security": [{"session": []}],
            "paths": {}
        }"#;
        let preview = parse(spec).unwrap();
        assert_eq!(preview.root.auth, AuthConfig::ApiKey { key: "sid".into(), value: String::new(), location: ApiKeyLocation::Header });
        assert!(preview.warnings.iter().any(|w| w.contains("cookie")), "{:?}", preview.warnings);
    }

    #[test]
    fn oauth2_authorization_code_flow_maps_urls_and_scopes() {
        let spec = r#"{
            "openapi": "3.0.0",
            "info": {"title": "X", "version": "1.0"},
            "components": {
                "securitySchemes": {
                    "oauth": {
                        "type": "oauth2",
                        "flows": {
                            "authorizationCode": {
                                "authorizationUrl": "https://auth.example.com/authorize",
                                "tokenUrl": "https://auth.example.com/token",
                                "scopes": {"read": "Read access", "write": "Write access"}
                            }
                        }
                    }
                }
            },
            "security": [{"oauth": ["read", "write"]}],
            "paths": {}
        }"#;
        let preview = parse(spec).unwrap();
        assert_eq!(
            preview.root.auth,
            AuthConfig::OAuth2(OAuth2Config {
                grant_type: OAuth2GrantType::AuthorizationCode,
                auth_url: "https://auth.example.com/authorize".into(),
                token_url: "https://auth.example.com/token".into(),
                scope: "read write".into(),
                ..Default::default()
            })
        );
    }

    #[test]
    fn oauth2_implicit_only_flow_imports_as_no_auth_with_a_warning() {
        let spec = r#"{
            "openapi": "3.0.0",
            "info": {"title": "X", "version": "1.0"},
            "components": {
                "securitySchemes": {
                    "oauth": {"type": "oauth2", "flows": {"implicit": {"authorizationUrl": "https://a.test/auth", "scopes": {}}}}
                }
            },
            "security": [{"oauth": []}],
            "paths": {}
        }"#;
        let preview = parse(spec).unwrap();
        assert_eq!(preview.root.auth, AuthConfig::None);
        assert!(preview.warnings.iter().any(|w| w.contains("implicit")), "{:?}", preview.warnings);
    }

    #[test]
    fn external_ref_is_skipped_with_a_warning_never_fetched() {
        let spec = r#"{
            "openapi": "3.0.0",
            "info": {"title": "X", "version": "1.0"},
            "paths": {
                "/x": {
                    "post": {
                        "requestBody": {"$ref": "https://evil.example.com/schemas/body.json"},
                        "responses": {"default": {"description": "ok"}}
                    }
                }
            }
        }"#;
        let preview = parse(spec).unwrap();
        assert_eq!(preview.root.requests[0].body, RequestBody::None);
        assert!(preview.warnings.iter().any(|w| w.contains("external references")), "{:?}", preview.warnings);
    }

    #[test]
    fn cyclical_schema_ref_does_not_infinite_loop() {
        let spec = r##"{
            "openapi": "3.0.0",
            "info": {"title": "X", "version": "1.0"},
            "components": {
                "schemas": {
                    "A": {"type": "object", "properties": {"b": {"$ref": "#/components/schemas/B"}}},
                    "B": {"type": "object", "properties": {"a": {"$ref": "#/components/schemas/A"}}}
                }
            },
            "paths": {
                "/x": {
                    "post": {
                        "requestBody": {"content": {"application/json": {"schema": {"$ref": "#/components/schemas/A"}}}},
                        "responses": {"default": {"description": "ok"}}
                    }
                }
            }
        }"##;
        let preview = parse(spec).unwrap();
        assert!(matches!(preview.root.requests[0].body, RequestBody::Json(_)));
    }

    #[test]
    fn untemplatize_path_and_templatize_path_are_inverses() {
        assert_eq!(untemplatize_path("/pets/{petId}/photo"), "/pets/{{petId}}/photo");
        let (key, names) = templatize_path("/pets/{{petId}}/photo");
        assert_eq!(key, "/pets/{petId}/photo");
        assert_eq!(names, vec!["petId".to_string()]);
    }

    #[test]
    fn export_emits_required_responses_and_a_path_parameter() {
        let node = ImportedNode {
            name: "Demo".into(),
            requests: vec![ImportedRequest {
                name: "Get user".into(),
                method: "GET".into(),
                url: "https://api.example.com/users/{{userId}}".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let out = export(&node).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        let op = v.pointer("/paths/~1users~1{userId}/get").expect("path+method present");
        assert_eq!(op.pointer("/responses/default/description").and_then(Value::as_str), Some("Response"));
        let params = op.get("parameters").and_then(Value::as_array).unwrap();
        assert!(params.iter().any(|p| p.get("name").and_then(Value::as_str) == Some("userId") && p.get("in").and_then(Value::as_str) == Some("path")));
    }

    #[test]
    fn export_never_templatizes_a_host_only_variable() {
        let node = ImportedNode {
            name: "Demo".into(),
            requests: vec![ImportedRequest {
                name: "Get user".into(),
                method: "GET".into(),
                url: "{{baseUrl}}/users/{{userId}}".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let out = export(&node).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v.pointer("/servers/0/url").and_then(Value::as_str), Some("{{baseUrl}}"));
        assert!(v["paths"].as_object().unwrap().contains_key("/users/{userId}"));
    }

    #[test]
    fn export_bakes_inherited_folder_auth_into_each_contained_operation() {
        let node = ImportedNode {
            name: "Demo".into(),
            auth: AuthConfig::Bearer { token: String::new(), prefix: crate::model::auth::default_bearer_prefix() },
            children: vec![ImportedNode {
                name: "Admin".into(),
                auth: AuthConfig::Basic { username: String::new(), password: String::new() },
                requests: vec![ImportedRequest {
                    name: "List users".into(),
                    method: "GET".into(),
                    url: "https://api.example.com/admin/users".into(),
                    auth: RequestAuth::Inherit,
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let out = export(&node).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        let op = v.pointer("/paths/~1admin~1users/get").expect("path+method present");
        assert!(op["security"][0].get("basicAuth").is_some(), "{op}");
        assert!(v["security"][0].get("bearerAuth").is_some(), "{v}");
    }

    const SWAGGER_FIXTURE: &str = r#"{
        "swagger": "2.0",
        "info": { "title": "Petstore2", "description": "Sample pet store API (v2)", "version": "1.0.0" },
        "host": "petstore.example.com",
        "basePath": "/v2",
        "schemes": ["https"],
        "security": [{ "basicAuth": [] }],
        "securityDefinitions": {
            "basicAuth": { "type": "basic" },
            "apiKeyAuth": { "type": "apiKey", "in": "header", "name": "X-API-Key" }
        },
        "paths": {
            "/pets/{petId}": {
                "parameters": [
                    { "name": "petId", "in": "path", "required": true, "type": "string" }
                ],
                "get": {
                    "tags": ["Pets"],
                    "summary": "Get pet by id",
                    "parameters": [{ "name": "verbose", "in": "query", "type": "string", "default": "true" }],
                    "responses": { "200": { "description": "ok" } }
                },
                "delete": {
                    "tags": ["Pets"],
                    "summary": "Delete pet",
                    "security": [],
                    "responses": { "204": { "description": "deleted" } }
                }
            },
            "/pets": {
                "post": {
                    "tags": ["Pets"],
                    "summary": "Create pet",
                    "parameters": [
                        {
                            "name": "body",
                            "in": "body",
                            "required": true,
                            "schema": { "type": "object", "properties": { "name": { "type": "string", "example": "Fido" } } }
                        }
                    ],
                    "security": [{ "apiKeyAuth": [] }],
                    "responses": { "201": { "description": "created" } }
                }
            },
            "/pets/{petId}/photo": {
                "post": {
                    "summary": "Upload photo",
                    "consumes": ["multipart/form-data"],
                    "parameters": [
                        { "name": "petId", "in": "path", "required": true, "type": "string" },
                        { "name": "caption", "in": "formData", "type": "string", "default": "cute dog" },
                        { "name": "photo", "in": "formData", "type": "file" }
                    ],
                    "responses": { "default": { "description": "ok" } }
                }
            },
            "/login": {
                "post": {
                    "summary": "Login",
                    "consumes": ["application/x-www-form-urlencoded"],
                    "parameters": [
                        { "name": "user", "in": "formData", "type": "string", "default": "alice" }
                    ],
                    "responses": { "default": { "description": "ok" } }
                }
            },
            "/unknown-scheme": {
                "get": {
                    "summary": "Uses an undefined scheme",
                    "security": [{ "missingScheme": [] }],
                    "responses": { "default": { "description": "ok" } }
                }
            }
        }
    }"#;

    #[test]
    fn imports_realistic_swagger2_fixture_with_expected_shape_and_warnings() {
        let preview = parse(SWAGGER_FIXTURE).unwrap();
        assert_eq!(preview.root.name, "Petstore2");
        assert_eq!(preview.root.description, Some("Sample pet store API (v2)".to_string()));
        assert_eq!(preview.root.auth, AuthConfig::Basic { username: String::new(), password: String::new() });

        assert_eq!(preview.stats.requests, 6);
        assert_eq!(preview.stats.folders, 1);
        assert_eq!(preview.root.children.len(), 1);
        let pets = &preview.root.children[0];
        assert_eq!(pets.name, "Pets");
        assert_eq!(pets.requests.len(), 3);
        assert_eq!(preview.root.requests.len(), 3);

        let get_pet = find_request(&preview.root, "Get pet by id");
        assert_eq!(get_pet.method, "GET");
        assert_eq!(get_pet.url, "https://petstore.example.com/v2/pets/{{petId}}");
        assert_eq!(get_pet.auth, RequestAuth::Inherit);
        assert_eq!(get_pet.query, vec![KeyValue { key: "verbose".into(), value: "true".into(), enabled: true }]);

        let delete_pet = find_request(&preview.root, "Delete pet");
        assert_eq!(delete_pet.method, "DELETE");
        assert_eq!(delete_pet.auth, RequestAuth::Own(AuthConfig::None));

        let create_pet = find_request(&preview.root, "Create pet");
        assert_eq!(create_pet.method, "POST");
        assert_eq!(create_pet.url, "https://petstore.example.com/v2/pets");
        assert_eq!(
            create_pet.auth,
            RequestAuth::Own(AuthConfig::ApiKey { key: "X-API-Key".into(), value: String::new(), location: ApiKeyLocation::Header })
        );
        match &create_pet.body {
            RequestBody::Json(s) => assert_eq!(serde_json::from_str::<Value>(s).unwrap(), json!({"name": "Fido"})),
            other => panic!("expected Json body, got {other:?}"),
        }

        let photo = find_request(&preview.root, "Upload photo");
        assert_eq!(photo.url, "https://petstore.example.com/v2/pets/{{petId}}/photo");
        match &photo.body {
            RequestBody::FormData(fields) => {
                let caption = fields.iter().find(|f| f.key == "caption").unwrap();
                assert_eq!(caption.value, "cute dog");
                assert!(!caption.is_file);
                let file = fields.iter().find(|f| f.key == "photo").unwrap();
                assert!(file.is_file);
                assert_eq!(file.value, "");
            }
            other => panic!("expected FormData body, got {other:?}"),
        }

        let login = find_request(&preview.root, "Login");
        match &login.body {
            RequestBody::UrlEncoded(fields) => {
                assert_eq!(fields, &vec![KeyValue { key: "user".into(), value: "alice".into(), enabled: true }]);
            }
            other => panic!("expected UrlEncoded body, got {other:?}"),
        }

        let unknown = find_request(&preview.root, "Uses an undefined scheme");
        assert_eq!(unknown.auth, RequestAuth::Own(AuthConfig::None));
        assert!(preview.warnings.iter().any(|w| w.contains("missingScheme")), "{:?}", preview.warnings);
    }

    #[test]
    fn swagger2_oauth2_access_code_flow_honors_requested_scopes_over_scheme_defaults() {
        // The scheme defines two scopes, but the security requirement only
        // asks for one — the requested scope must win, not the full set
        // defined on the scheme (that fallback only applies when nothing was
        // actually requested, e.g. an empty `[]`).
        let spec = r#"{
            "swagger": "2.0",
            "info": {"title": "X", "version": "1.0"},
            "securityDefinitions": {
                "oauth": {
                    "type": "oauth2",
                    "flow": "accessCode",
                    "authorizationUrl": "https://auth.example.com/authorize",
                    "tokenUrl": "https://auth.example.com/token",
                    "scopes": {"read": "Read access", "write": "Write access"}
                }
            },
            "security": [{"oauth": ["read"]}],
            "paths": {}
        }"#;
        let preview = parse(spec).unwrap();
        assert_eq!(
            preview.root.auth,
            AuthConfig::OAuth2(OAuth2Config {
                grant_type: OAuth2GrantType::AuthorizationCode,
                auth_url: "https://auth.example.com/authorize".into(),
                token_url: "https://auth.example.com/token".into(),
                scope: "read".into(),
                ..Default::default()
            })
        );
    }

    #[test]
    fn swagger2_oauth2_implicit_flow_imports_as_no_auth_with_a_warning() {
        let spec = r#"{
            "swagger": "2.0",
            "info": {"title": "X", "version": "1.0"},
            "securityDefinitions": {
                "oauth": {"type": "oauth2", "flow": "implicit", "authorizationUrl": "https://a.test/auth", "scopes": {}}
            },
            "security": [{"oauth": []}],
            "paths": {}
        }"#;
        let preview = parse(spec).unwrap();
        assert_eq!(preview.root.auth, AuthConfig::None);
        assert!(preview.warnings.iter().any(|w| w.contains("implicit")), "{:?}", preview.warnings);
    }

    #[test]
    fn swagger2_round_trips_through_openapi3_export() {
        // Export always emits OpenAPI 3.0 regardless of the original import
        // format (see module doc) — re-parsing that output is a cross-format
        // round trip, not a same-format one, so only method+url and the
        // tag-derived folder are asserted, same as the 3.0 round-trip test.
        let preview_a = parse(SWAGGER_FIXTURE).unwrap();
        let exported = export(&preview_a.root).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&exported).unwrap()["openapi"], json!("3.0.3"));
        let preview_b = parse(&exported).unwrap();

        let mut a = Vec::new();
        all_requests(&preview_a.root, &mut a);
        let mut b = Vec::new();
        all_requests(&preview_b.root, &mut b);
        let a_set: BTreeSet<_> = a.iter().map(|r| (r.method.clone(), r.url.clone())).collect();
        let b_set: BTreeSet<_> = b.iter().map(|r| (r.method.clone(), r.url.clone())).collect();
        assert_eq!(a_set, b_set);

        let a_folders: BTreeSet<_> = preview_a.root.children.iter().map(|c| c.name.clone()).collect();
        let b_folders: BTreeSet<_> = preview_b.root.children.iter().map(|c| c.name.clone()).collect();
        assert_eq!(a_folders, b_folders);
    }

    #[test]
    fn parse_rejects_a_document_with_neither_openapi_nor_swagger_version_field() {
        let spec = r#"{"info": {"title": "X"}, "paths": {}}"#;
        let err = parse(spec).unwrap_err();
        assert!(err.to_string().contains("openapi") && err.to_string().contains("swagger"), "{err}");
    }
}
