//! Bruno `.bru` request-file import. Bruno stores collections as a directory
//! tree with one `.bru` file per request plus a `bru.json` collection
//! manifest. The import flow here takes a single text blob (paste or upload),
//! so this module supports the **request** `.bru` grammar — one request,
//! wrapped in a synthetic root `ImportedNode` (like `curl`). A directory full
//! of `.bru` files would need a multi-file upload flow this app doesn't
//! expose yet; the `bru.json`-style multi-file shape is intentionally out of
//! scope — import a `.bru` request at a time instead, or reach for Postman
//! round-trip if you want the whole tree at once.
//!
//! Grammar (Bruno's request language, simplified to what this parser accepts):
//!
//! ```bru
//! meta {
//!   name: Get user
//!   method: GET
//! }
//! url {
//!   https://api.example.com/users/1
//! }
//! query {
//!   verbose: true
//!   disabled:limit: 10
//! }
//! headers {
//!   Accept: application/json
//!   disabled:X-Debug: 1
//! }
//! auth {
//!   bearer: tok123
//!   // or  basic: alice:secret
//!   // or  apikey: X-Key:val | header
//! }
//! body {
//!   json: {"name":"Fido"}
//!   // or  text: plain body
//!   // or  form-urlencoded: key=value & key2=value2
//!   // or  multipart-formdata: caption=cute dog, photo=@/tmp/fido.png
//! }
//! pre-request-script {
//!   console.log('before');
//! }
//! post-response-script {
//!   pm.test('ok', () => {});
//! }
//! ```
//!
//! Export is intentionally NOT supported (mirroring `insomnia`) — Bruno's
//! on-disk format is a directory, not a single text blob this app's export
//! path produces.

use crate::error::{AppError, AppResult};
use crate::interop::{ImportPreview, ImportedNode, ImportedRequest};
use crate::model::auth::{ApiKeyLocation, AuthConfig, RequestAuth};
use crate::model::http::{FormField, HeaderEntry, KeyValue, RequestBody, RequestOptions};

pub fn parse(content: &str) -> AppResult<ImportPreview> {
    let sections = parse_sections(content);
    if sections.is_empty() {
        return Err(AppError::Other("not a Bruno .bru file: no `{ ... }` sections found".into()));
    }

    let mut warnings = Vec::new();
    let meta = sections.iter().find(|(n, _)| *n == "meta").map(|(_, b)| b).cloned().unwrap_or_default();
    let name = kv_get(&meta, "name").unwrap_or_else(|| "Imported Bruno request".to_string());
    let method = kv_get(&meta, "method").unwrap_or_else(|| "GET".to_string()).to_uppercase();

    let url = sections
        .iter()
        .find(|(n, _)| *n == "url")
        .and_then(|(_, b)| b.lines().find(|l| !l.trim().is_empty()))
        .map(|l| l.trim().to_string())
        .unwrap_or_default();
    let (base_url, query) = if let Some((b, qs)) = url.split_once('?') {
        (b.to_string(), split_query(qs))
    } else {
        (url.clone(), Vec::new())
    };

    let headers = parse_headers_section(sections.iter().find(|(n, _)| *n == "headers").map(|(_, b)| b.as_str()).unwrap_or(""));
    let query = if query.is_empty() {
        parse_query_section(sections.iter().find(|(n, _)| *n == "query").map(|(_, b)| b.as_str()).unwrap_or(""))
    } else {
        query
    };
    let auth = parse_auth_section(sections.iter().find(|(n, _)| *n == "auth").map(|(_, b)| b.as_str()).unwrap_or(""), &mut warnings);
    let body = parse_body_section(sections.iter().find(|(n, _)| *n == "body").map(|(_, b)| b.as_str()).unwrap_or(""), &headers, &mut warnings);

    let pre = sections.iter().find(|(n, _)| *n == "pre-request-script").map(|(_, b)| b.trim_end_matches('\n').to_string()).unwrap_or_default();
    let post = sections.iter().find(|(n, _)| *n == "post-response-script").map(|(_, b)| b.trim_end_matches('\n').to_string()).unwrap_or_default();

    let request = ImportedRequest {
        name: name.clone(),
        method,
        url: base_url,
        headers,
        query,
        body,
        options: RequestOptions::default(),
        auth: RequestAuth::Own(auth),
        pre_request_script: pre,
        post_response_script: post,
    };
    let root = ImportedNode { name, description: None, auth: AuthConfig::None, requests: vec![request], children: vec![] };
    Ok(ImportPreview::new(root, warnings))
}

// ---------------------------------------------------------------------------
// Section parser: a brace-block splitter that ignores braces inside string
// literals only superficially (single quote) — Bruno blocks don't nest.
// ---------------------------------------------------------------------------

fn parse_sections(content: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut chars = content.chars().peekable();
    let mut name = String::new();
    let mut buf = String::new();
    let mut in_name = false;
    let mut in_block = false;
    let mut depth = 0;
    while let Some(c) = chars.next() {
        if !in_block && !in_name && c.is_alphabetic() {
            in_name = true;
            name.push(c);
            continue;
        }
        if in_name {
            if c.is_alphanumeric() || c == '-' {
                name.push(c);
            } else if c == ' ' || c == '\t' {
                // tolerate trailing whitespace
            } else if c == '{' {
                in_name = false;
                in_block = true;
                depth = 1;
            } else {
                in_name = false;
                name.clear();
            }
            continue;
        }
        if in_block {
            if c == '{' {
                depth += 1;
                buf.push(c);
            } else if c == '}' {
                depth -= 1;
                if depth == 0 {
                    out.push((std::mem::take(&mut name), std::mem::take(&mut buf)));
                    in_block = false;
                } else {
                    buf.push(c);
                }
            } else {
                buf.push(c);
            }
        }
    }
    out
}

fn kv_get(section: &str, key: &str) -> Option<String> {
    for line in section.lines() {
        let line = line.trim();
        if let Some((k, v)) = line.split_once(':') {
            if k.trim() == key {
                return Some(v.trim().trim_matches('"').to_string());
            }
        }
    }
    None
}

fn split_query(qs: &str) -> Vec<KeyValue> {
    qs.split('&').filter(|s| !s.is_empty()).map(|pair| match pair.split_once('=') {
        Some((k, v)) => KeyValue { key: k.to_string(), value: v.to_string(), enabled: true },
        None => KeyValue { key: pair.to_string(), value: String::new(), enabled: true },
    }).collect()
}

fn parse_query_section(section: &str) -> Vec<KeyValue> {
    section.lines().filter_map(|line| {
        let line = line.trim();
        if line.is_empty() { return None; }
        let (disabled, rest) = line.strip_prefix("disabled:").map(|r| (true, r)).unwrap_or((false, line));
        let (key, value) = rest.split_once(':').unwrap_or((rest, ""));
        Some(KeyValue { key: key.trim().to_string(), value: value.trim().to_string(), enabled: !disabled })
    }).collect()
}

fn parse_headers_section(section: &str) -> Vec<HeaderEntry> {
    section.lines().filter_map(|line| {
        let line = line.trim();
        if line.is_empty() { return None; }
        let (disabled, rest) = line.strip_prefix("disabled:").map(|r| (true, r)).unwrap_or((false, line));
        let (name, value) = rest.split_once(':').unwrap_or((rest, ""));
        Some(HeaderEntry { name: name.trim().to_string(), value: value.trim().to_string(), enabled: !disabled })
    }).collect()
}

fn parse_auth_section(section: &str, warnings: &mut Vec<String>) -> AuthConfig {
    for line in section.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("bearer:") {
            return AuthConfig::Bearer { token: rest.trim().to_string() };
        }
        if let Some(rest) = line.strip_prefix("basic:") {
            let (u, p) = rest.trim().split_once(':').map(|(u, p)| (u.to_string(), p.to_string())).unwrap_or((rest.trim().to_string(), String::new()));
            return AuthConfig::Basic { username: u, password: p };
        }
        if let Some(rest) = line.strip_prefix("apikey:") {
            // apikey: KEY:VALUE | header   or   apikey: KEY:VALUE | query
            let mut parts = rest.split('|');
            let head = parts.next().unwrap_or("").trim();
            let loc = parts.next().unwrap_or("header").trim();
            let (key, value) = head.split_once(':').unwrap_or((head, ""));
            return AuthConfig::ApiKey {
                key: key.trim().to_string(),
                value: value.trim().to_string(),
                location: if loc == "query" { ApiKeyLocation::Query } else { ApiKeyLocation::Header },
            };
        }
        if line.starts_with("awsv4:") {
            // awsv4: accessKey:secretKey:region:service
            let rest = line.trim_start_matches("awsv4:").trim();
            let parts: Vec<&str> = rest.splitn(4, ':').collect();
            if parts.len() == 4 {
                return AuthConfig::AwsSigV4(crate::model::auth::AwsSigV4Config {
                    access_key: parts[0].to_string(),
                    secret_key: parts[1].to_string(),
                    region: parts[2].to_string(),
                    service: parts[3].to_string(),
                    session_token: String::new(),
                });
            }
            warnings.push("Bruno awsv4 auth has wrong field count — dropped".into());
            return AuthConfig::None;
        }
    }
    AuthConfig::None
}

fn parse_body_section(section: &str, headers: &[HeaderEntry], warnings: &mut Vec<String>) -> RequestBody {
    for line in section.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("json:") {
            return RequestBody::Json(rest.trim().to_string());
        }
        if let Some(rest) = line.strip_prefix("text:") {
            return RequestBody::Raw { content: rest.trim().to_string(), language: None };
        }
        if let Some(rest) = line.strip_prefix("xml:") {
            return RequestBody::Raw { content: rest.trim().to_string(), language: Some("xml".to_string()) };
        }
        if let Some(rest) = line.strip_prefix("form-urlencoded:") {
            return RequestBody::UrlEncoded(
                rest.split('&').filter(|s| !s.is_empty()).map(|pair| match pair.split_once('=') {
                    Some((k, v)) => KeyValue { key: k.trim().to_string(), value: v.trim().to_string(), enabled: true },
                    None => KeyValue { key: pair.trim().to_string(), value: String::new(), enabled: true },
                }).collect(),
            );
        }
        if let Some(rest) = line.strip_prefix("multipart-formdata:") {
            return RequestBody::FormData(
                rest.split(',').filter(|s| !s.is_empty()).filter_map(|pair| {
                    let (k, v) = pair.split_once('=')?;
                    let is_file = v.starts_with('@');
                    Some(FormField { key: k.trim().to_string(), value: if is_file { v[1..].trim().to_string() } else { v.trim().to_string() }, enabled: true, is_file, content_type: None })
                }).collect(),
            );
        }
        if let Some(rest) = line.strip_prefix("graphql:") {
            let query = rest.trim().to_string();
            // Bruno separates query/variables with newlines; we keep just the query on this line.
            return RequestBody::Graphql { query, variables: None, operation_name: None };
        }
    }
    let _ = (headers, warnings);
    RequestBody::None
}

#[cfg(test)]
mod tests {
    use super::*;

    const BRU_FIXTURE: &str = r#"meta {
  name: Get user
  method: GET
}
url {
  https://api.example.com/users/1?verbose=true
}
headers {
  Accept: application/json
  disabled:X-Debug: 1
}
auth {
  bearer: tok123
}
body {
  json: {"name":"Fido"}
}
pre-request-script {
  console.log('before');
}
post-response-script {
  pm.test('ok');
}"#;

    #[test]
    fn imports_realistic_bru_fixture_with_expected_shape() {
        let preview = parse(BRU_FIXTURE).unwrap();
        assert_eq!(preview.root.requests.len(), 1);
        let req = &preview.root.requests[0];
        assert_eq!(req.name, "Get user");
        assert_eq!(req.method, "GET");
        assert_eq!(req.url, "https://api.example.com/users/1");
        assert_eq!(req.query, vec![
            KeyValue { key: "verbose".into(), value: "true".into(), enabled: true },
        ]);
        assert!(req.headers.iter().any(|h| h.name == "Accept"));
        let x_debug = req.headers.iter().find(|h| h.name == "X-Debug").unwrap();
        assert!(!x_debug.enabled);
        assert_eq!(req.auth, RequestAuth::Own(AuthConfig::Bearer { token: "tok123".into() }));
        assert_eq!(req.body, RequestBody::Json("{\"name\":\"Fido\"}".into()));
        assert!(req.pre_request_script.contains("before"));
        assert!(req.post_response_script.contains("ok"));
    }

    #[test]
    fn bruno_basic_auth_and_form_body_parse() {
        let bru = r#"meta {
  name: Login
  method: POST
}
url {
  https://api.test/login
}
auth {
  basic: alice:secret
}
body {
  form-urlencoded: user=alice & pass=secret
}"#;
        let preview = parse(bru).unwrap();
        let req = &preview.root.requests[0];
        assert_eq!(req.auth, RequestAuth::Own(AuthConfig::Basic { username: "alice".into(), password: "secret".into() }));
        match &req.body {
            RequestBody::UrlEncoded(kv) => assert_eq!(kv, &[
                KeyValue { key: "user".into(), value: "alice".into(), enabled: true },
                KeyValue { key: "pass".into(), value: "secret".into(), enabled: true },
            ]),
            other => panic!("expected UrlEncoded body, got {other:?}"),
        }
    }

    #[test]
    fn rejects_non_bru_content() {
        let err = parse("just some text, no braces").unwrap_err();
        assert!(err.to_string().contains("no `{ ... }` sections"));
    }
}
