//! `.http` file import (JetBrains HTTP Client / VS Code REST Client grammar).
//! A `.http` file is a sequence of requests separated by `###` comment lines:
//!
//! ```http
//! @host = https://api.example.com
//! @token = abc123
//!
//! # First request
//! GET {{host}}/items?limit=5
//! Accept: application/json
//! Authorization: Bearer {{token}}
//!
//! ###
//!
//! # Second request
//! POST {{host}}/items
//! Content-Type: application/json
//!
//! {"name": "Fido"}
//! ```
//!
//! Optional `# @name my-request` lines give requests a friendly name (the
//! JetBrains/VS Code convention). In-file variables (`@name = value`) are
//! inlined into URLs/headers/bodies before building the IR tree — this app's
//! own `{{var}}` interpolation is workspace/env-scoped, not file-scoped, so
//! there's no place to carry a `.http` variable as a persistent variable.
//! Inlining keeps the user from losing the substitution entirely; it matches
//! what every other importer here does with format-intrinsic features it
//! can't directly represent (warn or substitute, not silently drop).
//!
//! Export is intentionally NOT supported here — same convention as `insomnia`/
//! `bruno` (import-only for formats that aren't a natural fit for this app's
//! single-text-blob export path; `.http` round-trips exist in the JetBrains
//! ecosystem but aren't in this app's stated export targets).

use crate::error::{AppError, AppResult};
use crate::interop::{ImportPreview, ImportedNode, ImportedRequest};
use crate::model::auth::{AuthConfig, RequestAuth};
use crate::model::http::{HeaderEntry, KeyValue, RequestBody, RequestOptions};
use std::collections::HashMap;

pub fn parse(content: &str) -> AppResult<ImportPreview> {
    let mut warnings = Vec::new();
    let (vars, body_lines) = extract_variables(content);
    let blocks = split_request_blocks(&body_lines);

    if blocks.is_empty() {
        return Err(AppError::Other("not an .http file: no request blocks found (use `###` to separate, or a single bare request)".into()));
    }

    let requests: Vec<ImportedRequest> = blocks.iter().filter_map(|b| parse_block(b, &vars, &mut warnings)).collect();
    if requests.is_empty() {
        return Err(AppError::Other(".http file had request blocks but none parsed into a request".into()));
    }

    let root = ImportedNode { name: "Imported .http".into(), description: None, auth: AuthConfig::None, requests, children: vec![] };
    Ok(ImportPreview::new(root, warnings))
}

fn extract_variables(content: &str) -> (HashMap<String, String>, String) {
    let mut vars = HashMap::new();
    let mut kept = String::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("@") {
            if let Some((k, v)) = rest.split_once('=') {
                vars.insert(k.trim().to_string(), v.trim().trim_matches('"').to_string());
                continue;
            }
        }
        kept.push_str(line);
        kept.push('\n');
    }
    (vars, kept)
}

fn split_request_blocks(body: &str) -> Vec<Vec<String>> {
    let mut blocks = Vec::new();
    let mut current = Vec::new();
    for line in body.lines() {
        if line.trim().starts_with("###") {
            if !current.is_empty() {
                blocks.push(std::mem::take(&mut current));
            }
        } else {
            current.push(line.to_string());
        }
    }
    if !current.is_empty() {
        blocks.push(current);
    }
    blocks
}

fn parse_block(lines: &[String], vars: &HashMap<String, String>, warnings: &mut Vec<String>) -> Option<ImportedRequest> {
    // First non-comment, non-empty line with a method + URL is the request
    // line. Lines above it that begin with `# @name X` set the request name.
    let mut name: Option<String> = None;
    let mut request_line_idx: Option<usize> = None;
    let (method, url_raw) = loop {
        if request_line_idx >= Some(lines.len()) {
            return None;
        }
        let idx = request_line_idx.unwrap_or(0);
        let line = lines[idx].trim();
        request_line_idx = Some(idx + 1);
        if line.is_empty() || line.starts_with('#') {
            if let Some(rest) = line.strip_prefix("# @name ") {
                name = Some(rest.trim().to_string());
            } else if let Some(rest) = line.strip_prefix("#@name ") {
                name = Some(rest.trim().to_string());
            }
            continue;
        }
        break split_request_line(line);
    }?;

    let url = interpolate(&url_raw, vars);
    let (base_url, query_inline) = split_url(&url);
    let (headers_raw, body_lines_start) = parse_headers(&lines[request_line_idx.unwrap()..]);
    let headers: Vec<HeaderEntry> = headers_raw
        .into_iter()
        .map(|h| HeaderEntry { name: interpolate(&h.name, vars), value: interpolate(&h.value, vars), enabled: h.enabled })
        .collect();
    let body_text = interpolate(&body_lines_start.join("\n").trim_end().to_string(), vars);

    let (headers, auth) = collapse_auth(headers);
    let body = body_from_lines(body_text, &headers, warnings);

    let query = if query_inline.is_empty() { Vec::new() } else { query_inline };
    let final_name = name.unwrap_or_else(|| derive_name(&method, &base_url));
    let _ = query;

    Some(ImportedRequest {
        name: final_name,
        method,
        url: base_url,
        headers,
        query,
        body,
        options: RequestOptions::default(),
        auth: RequestAuth::Own(auth),
        pre_request_script: String::new(),
        post_response_script: String::new(),
    })
}

fn split_request_line(line: &str) -> Option<(String, String)> {
    let mut parts = line.splitn(2, char::is_whitespace);
    let first = parts.next()?.trim().to_string();
    if first.is_empty() {
        return None;
    }
    // Bare URL with no method/args → default to GET. `.http` lets you omit the
    // method entirely when the line is just a URL, so a single-token line
    // that looks like a URL is a valid (GET) request.
    if first.contains("://") {
        return Some(("GET".to_string(), first.to_string()));
    }
    // Otherwise the first token must be an HTTP method followed by a URL.
    let second = parts.next()?.trim().to_string();
    const METHODS: &[&str] = &["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS", "TRACE", "CONNECT"];
    if METHODS.iter().any(|m| m.eq_ignore_ascii_case(&first)) && !second.is_empty() {
        return Some((first.to_uppercase(), second.to_string()));
    }
    None
}

fn split_url(raw: &str) -> (String, Vec<KeyValue>) {
    if let Some((b, qs)) = raw.split_once('?') {
        let query = qs.split('&').filter(|s| !s.is_empty()).map(|pair| match pair.split_once('=') {
            Some((k, v)) => KeyValue { key: k.to_string(), value: v.to_string(), enabled: true },
            None => KeyValue { key: pair.to_string(), value: String::new(), enabled: true },
        }).collect();
        (b.to_string(), query)
    } else {
        (raw.to_string(), Vec::new())
    }
}

fn parse_headers(lines: &[String]) -> (Vec<HeaderEntry>, Vec<String>) {
    let mut headers = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = &lines[i];
        let trimmed = line.trim();
        if trimmed.is_empty() {
            // A single blank line separates headers from the body — but only
            // once we've seen at least one header. Subsequent blanks at the
            // very top (before any header) are just leading whitespace.
            if !headers.is_empty() {
                i += 1;
                break;
            }
            i += 1;
            continue;
        }
        if trimmed.starts_with('#') {
            i += 1;
            continue;
        }
        if let Some((k, v)) = trimmed.split_once(':') {
            headers.push(HeaderEntry { name: k.trim().to_string(), value: v.trim().to_string(), enabled: true });
        } else {
            break;
        }
        i += 1;
    }
    (headers, lines[i..].to_vec())
}

fn collapse_auth(headers: Vec<HeaderEntry>) -> (Vec<HeaderEntry>, AuthConfig) {
    let mut headers = headers;
    let auth = match headers.iter().position(|h| h.name.eq_ignore_ascii_case("authorization")) {
        Some(idx) => {
            let value = headers[idx].value.clone();
            headers.remove(idx);
            if let Some(token) = value.strip_prefix("Bearer ") {
                AuthConfig::Bearer { token: token.to_string() }
            } else if let Some(rest) = value.strip_prefix("Basic ") {
                let decoded = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, rest).unwrap_or_default();
                let creds = String::from_utf8_lossy(&decoded);
                let (u, p) = creds.split_once(':').map(|(u, p)| (u.to_string(), p.to_string())).unwrap_or((creds.to_string(), String::new()));
                AuthConfig::Basic { username: u, password: p }
            } else {
                AuthConfig::None
            }
        }
        None => AuthConfig::None,
    };
    (headers, auth)
}

fn body_from_lines(text: String, headers: &[HeaderEntry], warnings: &mut Vec<String>) -> RequestBody {
    if text.is_empty() {
        return RequestBody::None;
    }
    let content_type = headers.iter().find(|h| h.name.eq_ignore_ascii_case("content-type")).map(|h| h.value.to_ascii_lowercase());
    match content_type.as_deref() {
        Some(ct) if ct.contains("json") => RequestBody::Json(text),
        Some(ct) if ct.contains("x-www-form-urlencoded") => RequestBody::UrlEncoded(
            text.split('&').filter(|s| !s.is_empty()).map(|pair| match pair.split_once('=') {
                Some((k, v)) => KeyValue { key: k.to_string(), value: v.to_string(), enabled: true },
                None => KeyValue { key: pair.to_string(), value: String::new(), enabled: true },
            }).collect(),
        ),
        Some(ct) if ct.contains("multipart/form-data") => RequestBody::FormData(
            text.split('&').filter(|s| !s.is_empty()).filter_map(|pair| {
                let (k, v) = pair.split_once('=')?;
                let is_file = v.starts_with('@');
                Some(crate::model::http::FormField { key: k.to_string(), value: if is_file { v[1..].to_string() } else { v.to_string() }, enabled: true, is_file, content_type: None })
            }).collect(),
        ),
        Some(ct) => {
            let language = ct.split('/').nth(1).map(str::to_string);
            RequestBody::Raw { content: text, language }
        }
        None => {
            warnings.push(".http request has a body but no Content-Type header — imported as a raw text body".into());
            RequestBody::Raw { content: text, language: None }
        }
    }
}

fn interpolate(s: &str, vars: &HashMap<String, String>) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        if let Some(end) = after.find("}}") {
            let key = after[..end].trim();
            out.push_str(vars.get(key).map(|v| v.as_str()).unwrap_or(""));
            rest = &after[end + 2..];
        } else {
            out.push_str(&rest[start..]);
            return out;
        }
    }
    out.push_str(rest);
    out
}

fn derive_name(method: &str, url: &str) -> String {
    let path = url.split_once("://").and_then(|(_, r)| r.split_once('/')).map(|(_, p)| format!("/{p}")).filter(|p| p != "/");
    format!("{method} {}", path.unwrap_or_else(|| url.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const HTTP_FIXTURE: &str = r#"@host = https://api.example.com
@token = abc123

# @name List items
GET {{host}}/items?limit=5
Accept: application/json
Authorization: Bearer {{token}}

###

# @name Create item
POST {{host}}/items
Content-Type: application/json

{"name": "Fido"}

###

# Bare request, no method
https://api.example.com/health
"#;

    #[test]
    fn imports_realistic_http_fixture_with_expected_shape() {
        let preview = parse(HTTP_FIXTURE).unwrap();
        assert_eq!(preview.root.requests.len(), 3);

        let list = &preview.root.requests[0];
        assert_eq!(list.name, "List items");
        assert_eq!(list.method, "GET");
        assert_eq!(list.url, "https://api.example.com/items");
        assert_eq!(list.query, vec![KeyValue { key: "limit".into(), value: "5".into(), enabled: true }]);
        assert!(list.headers.iter().any(|h| h.name == "Accept"));
        // Authorization header was collapsed into Bearer auth (token interpolated).
        assert_eq!(list.auth, RequestAuth::Own(AuthConfig::Bearer { token: "abc123".into() }));
        assert!(!list.headers.iter().any(|h| h.name.eq_ignore_ascii_case("authorization")));

        let create = &preview.root.requests[1];
        assert_eq!(create.name, "Create item");
        assert_eq!(create.method, "POST");
        assert_eq!(create.body, RequestBody::Json("{\"name\": \"Fido\"}".into()));

        let health = &preview.root.requests[2];
        assert_eq!(health.method, "GET");
        assert_eq!(health.url, "https://api.example.com/health");
    }

    #[test]
    fn empty_fixture_rejects() {
        let err = parse("just text\nwith no request line").unwrap_err();
        // Either "no request blocks found" or "request blocks but none parsed" —
        // both indicate rejection of non-`.http` prose.
        let msg = err.to_string();
        assert!(msg.contains("request blocks") || msg.contains("not an .http"), "{msg}");
    }

    #[test]
    fn inline_variables_in_body_and_header() {
        let http = "@base = https://x.test\n@key = secret\n\nGET {{base}}/y\nX-Api-Key: {{key}}\n\nbody";
        let preview = parse(http).unwrap();
        let req = &preview.root.requests[0];
        assert_eq!(req.url, "https://x.test/y");
        assert!(req.headers.iter().any(|h| h.name == "X-Api-Key" && h.value == "secret"));
    }
}
