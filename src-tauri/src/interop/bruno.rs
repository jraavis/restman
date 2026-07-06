//! Bruno `.bru` request-file import, in two flavors:
//!
//! - `parse(content)` — a single pasted/uploaded `.bru` request, wrapped in a
//!   synthetic root `ImportedNode` (like `curl`'s importer).
//! - `parse_directory(path)` — a real Bruno collection directory on disk:
//!   `bruno.json` names the root collection, each subdirectory becomes a
//!   folder (optionally named/authed by its own `folder.bru`), and every
//!   `.bru` file inside becomes a request. `environments/` is skipped (Bruno
//!   environment files use a different `vars {}` grammar this app's
//!   dedicated environment importer already handles), as are dotfiles/dirs.
//!   Ordering follows each file's `seq` meta field where present, falling
//!   back to filename. A file that fails to parse is skipped with a warning
//!   rather than failing the whole import.
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
    let mut warnings = Vec::new();
    let request = parse_request(content, &mut warnings)?;
    let name = request.name.clone();
    let root = ImportedNode { name, description: None, auth: AuthConfig::None, requests: vec![request], children: vec![] };
    Ok(ImportPreview::new(root, warnings))
}

/// Parse one `.bru` request file's content into an `ImportedRequest`. Shared
/// by the single-file paste path (`parse`, above) and the directory-walk
/// path (`parse_directory`, in the `dir` submodule) — a `.bru` request file
/// has the same grammar whether it arrived as a paste or as one file among
/// many in a real Bruno collection folder.
fn parse_request(content: &str, warnings: &mut Vec<String>) -> AppResult<ImportedRequest> {
    let sections = parse_sections(content);
    if sections.is_empty() {
        return Err(AppError::Other("not a Bruno .bru file: no `{ ... }` sections found".into()));
    }

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
    let auth = parse_auth_section(sections.iter().find(|(n, _)| *n == "auth").map(|(_, b)| b.as_str()).unwrap_or(""), warnings);
    let body = parse_body_section(sections.iter().find(|(n, _)| *n == "body").map(|(_, b)| b.as_str()).unwrap_or(""), &headers, warnings);

    let pre = sections.iter().find(|(n, _)| *n == "pre-request-script").map(|(_, b)| b.trim_end_matches('\n').to_string()).unwrap_or_default();
    let post = sections.iter().find(|(n, _)| *n == "post-response-script").map(|(_, b)| b.trim_end_matches('\n').to_string()).unwrap_or_default();

    Ok(ImportedRequest {
        name,
        method,
        url: base_url,
        headers,
        query,
        body,
        options: RequestOptions::default(),
        auth: RequestAuth::Own(auth),
        pre_request_script: pre,
        post_response_script: post,
        ..Default::default()
    })
}

// ---------------------------------------------------------------------------
// Directory import: a real Bruno collection on disk, one `.bru` file per
// request plus `bruno.json`/`folder.bru` manifests. See module doc for the
// on-disk shape this maps onto `ImportedNode`.
// ---------------------------------------------------------------------------

/// Import an entire Bruno collection directory, as opposed to `parse`, which
/// only understands one pasted/uploaded `.bru` request.
pub fn parse_directory(root: &std::path::Path) -> AppResult<ImportPreview> {
    if !root.is_dir() {
        return Err(AppError::Other(format!("not a directory: {}", root.display())));
    }
    let mut warnings = Vec::new();
    let name = read_bruno_json_name(root).unwrap_or_else(|| {
        root.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Imported Bruno collection".to_string())
    });
    let auth = read_manifest_auth(root, "collection.bru", &mut warnings).unwrap_or(AuthConfig::None);
    let (requests, children) = read_dir_contents(root, &mut warnings)?;
    let node = ImportedNode { name, description: None, auth, requests, children };
    // A folder picker has no format filter — the paste path errors on junk
    // ("no `{ ... }` sections"), so a wrong-folder pick here should surface
    // the same kind of signal rather than silently previewing an empty
    // collection.
    if total_requests(&node) == 0 {
        warnings.push(format!(
            "no `.bru` request files found under \"{}\" — check this is a Bruno collection folder",
            root.display()
        ));
    }
    Ok(ImportPreview::new(node, warnings))
}

fn total_requests(node: &ImportedNode) -> usize {
    node.requests.len() + node.children.iter().map(total_requests).sum::<usize>()
}

fn read_bruno_json_name(root: &std::path::Path) -> Option<String> {
    let content = std::fs::read_to_string(root.join("bruno.json")).ok()?;
    let value: serde_json::Value = serde_json::from_str(&content).ok()?;
    value.get("name")?.as_str().map(|s| s.to_string())
}

/// `folder.bru` (per-subfolder) and `collection.bru` (root) share their
/// `auth {}` block's grammar with request `.bru` files — reuse
/// `parse_auth_section` rather than a second parser. Collection/folder-level
/// headers/vars/scripts aren't mapped onto `ImportedNode` today, matching
/// this module's existing request-shaped scope.
fn read_manifest_auth(dir: &std::path::Path, file_name: &str, warnings: &mut Vec<String>) -> Option<AuthConfig> {
    let content = std::fs::read_to_string(dir.join(file_name)).ok()?;
    let auth_section = parse_sections(&content).into_iter().find(|(n, _)| *n == "auth")?.1;
    Some(parse_auth_section(&auth_section, warnings))
}

fn folder_bru_name(dir: &std::path::Path) -> Option<String> {
    let content = std::fs::read_to_string(dir.join("folder.bru")).ok()?;
    let meta = parse_sections(&content).into_iter().find(|(n, _)| *n == "meta")?.1;
    kv_get(&meta, "name")
}

fn extract_seq(content: &str) -> Option<i64> {
    let meta = parse_sections(content).into_iter().find(|(n, _)| *n == "meta")?.1;
    kv_get(&meta, "seq")?.parse::<i64>().ok()
}

/// One directory's direct contents, split into leaf requests (each `.bru`
/// file) and nested folders (each subdirectory). Both are sorted by Bruno's
/// `seq` meta field where present, falling back to filename so the ordering
/// is at least deterministic without one.
fn read_dir_contents(
    dir: &std::path::Path,
    warnings: &mut Vec<String>,
) -> AppResult<(Vec<ImportedRequest>, Vec<ImportedNode>)> {
    let mut requests: Vec<(Option<i64>, String, ImportedRequest)> = Vec::new();
    let mut children: Vec<(Option<i64>, String, ImportedNode)> = Vec::new();

    let entries = std::fs::read_dir(dir).map_err(|e| AppError::Other(format!("reading {}: {e}", dir.display())))?;
    for entry in entries {
        let entry = entry.map_err(|e| AppError::Other(format!("reading {}: {e}", dir.display())))?;
        let path = entry.path();
        let file_name = entry.file_name().to_string_lossy().to_string();
        if file_name.starts_with('.') {
            continue;
        }

        if path.is_dir() {
            // Bruno stores environment definitions here under a different
            // grammar (`vars {}` blocks, no `meta`/`url`/`headers`); this app
            // already has a dedicated environment importer, so this
            // directory is intentionally not walked as a folder of requests.
            if file_name.eq_ignore_ascii_case("environments") {
                continue;
            }
            let name = folder_bru_name(&path).unwrap_or_else(|| file_name.clone());
            let auth = read_manifest_auth(&path, "folder.bru", warnings).unwrap_or(AuthConfig::None);
            let seq = std::fs::read_to_string(path.join("folder.bru")).ok().and_then(|c| extract_seq(&c));
            let (sub_requests, sub_children) = read_dir_contents(&path, warnings)?;
            children.push((seq, file_name, ImportedNode { name, description: None, auth, requests: sub_requests, children: sub_children }));
            continue;
        }

        if file_name == "bruno.json" || file_name == "collection.bru" || file_name == "folder.bru" {
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("bru") {
            continue;
        }

        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                warnings.push(format!("\"{file_name}\": could not read file ({e}) — skipped"));
                continue;
            }
        };
        match parse_request(&content, warnings) {
            Ok(req) => requests.push((extract_seq(&content), file_name, req)),
            Err(e) => warnings.push(format!("\"{file_name}\": {e} — skipped")),
        }
    }

    requests.sort_by_key(|(seq, name, _)| (seq.unwrap_or(i64::MAX), name.clone()));
    children.sort_by_key(|(seq, name, _)| (seq.unwrap_or(i64::MAX), name.clone()));
    Ok((requests.into_iter().map(|(_, _, r)| r).collect(), children.into_iter().map(|(_, _, c)| c).collect()))
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

    // -----------------------------------------------------------------
    // Directory import
    // -----------------------------------------------------------------

    fn tmp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("restman_test_bruno_dir_{name}_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn parse_directory_walks_nested_collection_in_seq_order() {
        let root = tmp_dir("nested");
        std::fs::write(root.join("bruno.json"), r#"{"name": "My Collection", "version": "1"}"#).unwrap();
        std::fs::write(
            root.join("Get user.bru"),
            "meta {\n  name: Get user\n  seq: 2\n}\nurl {\n  https://api.test/users/1\n}\n",
        )
        .unwrap();
        std::fs::write(
            root.join("Create user.bru"),
            "meta {\n  name: Create user\n  seq: 1\n}\nurl {\n  https://api.test/users\n}\n",
        )
        .unwrap();

        let sub = root.join("Sub");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("folder.bru"), "meta {\n  name: Sub folder\n  seq: 1\n}\n").unwrap();
        std::fs::write(sub.join("Nested.bru"), "meta {\n  name: Nested\n}\nurl {\n  https://api.test/nested\n}\n").unwrap();

        let envs = root.join("environments");
        std::fs::create_dir_all(&envs).unwrap();
        std::fs::write(envs.join("Prod.bru"), "vars {\n  base_url: https://prod.test\n}\n").unwrap();

        let preview = parse_directory(&root).unwrap();
        assert_eq!(preview.root.name, "My Collection");
        assert_eq!(
            preview.root.requests.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            vec!["Create user", "Get user"],
            "seq: 1 must sort before seq: 2"
        );
        assert_eq!(preview.root.children.len(), 1, "environments/ must be skipped, only Sub/ counted");
        let sub_node = &preview.root.children[0];
        assert_eq!(sub_node.name, "Sub folder");
        assert_eq!(sub_node.requests.len(), 1);
        assert_eq!(sub_node.requests[0].name, "Nested");

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn parse_directory_falls_back_to_dirname_without_bruno_json() {
        let root = tmp_dir("no_manifest");
        std::fs::write(root.join("Ping.bru"), "meta {\n  name: Ping\n}\nurl {\n  https://api.test/ping\n}\n").unwrap();

        let preview = parse_directory(&root).unwrap();
        assert_eq!(preview.root.name, root.file_name().unwrap().to_string_lossy());
        assert_eq!(preview.root.requests.len(), 1);

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn parse_directory_skips_unparseable_file_and_warns_instead_of_failing() {
        let root = tmp_dir("bad_file");
        std::fs::write(root.join("Good.bru"), "meta {\n  name: Good\n}\nurl {\n  https://api.test/good\n}\n").unwrap();
        std::fs::write(root.join("Bad.bru"), "not a bru file, no braces at all").unwrap();

        let preview = parse_directory(&root).unwrap();
        assert_eq!(preview.root.requests.len(), 1);
        assert_eq!(preview.root.requests[0].name, "Good");
        assert!(preview.warnings.iter().any(|w| w.contains("Bad.bru")), "{:?}", preview.warnings);

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn parse_directory_rejects_a_plain_file_path() {
        let root = tmp_dir("not_a_dir_parent");
        let file = root.join("just_a_file.bru");
        std::fs::write(&file, "meta {\n  name: X\n}\n").unwrap();

        let err = parse_directory(&file).unwrap_err();
        assert!(err.to_string().contains("not a directory"), "{err}");

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn parse_directory_warns_when_no_bru_files_found_anywhere() {
        let root = tmp_dir("empty");
        let sub = root.join("Sub");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("readme.txt"), "not a bru file").unwrap();

        let preview = parse_directory(&root).unwrap();
        assert_eq!(preview.root.requests.len(), 0);
        assert!(
            preview.warnings.iter().any(|w| w.contains("no `.bru` request files found")),
            "{:?}",
            preview.warnings
        );

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn parse_directory_reads_folder_and_collection_level_auth() {
        let root = tmp_dir("auth");
        std::fs::write(root.join("collection.bru"), "auth {\n  bearer: root-tok\n}\n").unwrap();
        let sub = root.join("Sub");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("folder.bru"), "auth {\n  basic: alice:secret\n}\n").unwrap();
        std::fs::write(sub.join("Req.bru"), "meta {\n  name: Req\n}\nurl {\n  https://api.test/x\n}\n").unwrap();

        let preview = parse_directory(&root).unwrap();
        assert_eq!(preview.root.auth, AuthConfig::Bearer { token: "root-tok".into() });
        assert_eq!(preview.root.children[0].auth, AuthConfig::Basic { username: "alice".into(), password: "secret".into() });

        std::fs::remove_dir_all(&root).unwrap();
    }
}
