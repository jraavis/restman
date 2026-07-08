//! cURL command import/export. A curl invocation maps to exactly one
//! request — curl has no collection/folder concept — so `parse` wraps the
//! single request in a synthetic root `ImportedNode` (name derived from the
//! request itself) to fit the shared IR's collection-shaped contract. Export
//! is the mirror: it walks the whole tree and emits one `curl` block per
//! request, so exporting a multi-request collection yields a multi-block
//! script. See `interop` module doc for the secret-handling contract this
//! respects (import goes through `apply_import`, which already strips
//! unrecoverable masks; export reads through `collect()`, which already
//! masks).
//!
//! Parsing is a small POSIX-ish shell tokenizer (quotes + backslash escapes)
//! followed by a flag walk over a fixed set of known curl options. Real
//! curl's grammar has many more flags than handled here; an unrecognized
//! flag degrades to a warning rather than a hard failure, matching the
//! partial-import philosophy `postman::parse` already follows.

use crate::error::{AppError, AppResult};
use crate::interop::{ImportPreview, ImportedNode, ImportedRequest};
use crate::model::auth::{ApiKeyLocation, AuthConfig, RequestAuth};
use crate::model::http::{FormField, HeaderEntry, KeyValue, RequestBody, RequestOptions};

pub fn parse(content: &str) -> AppResult<ImportPreview> {
    // Strip shell line-continuations (backslash immediately before a
    // newline) so a multi-line pasted command tokenizes like one line. Only
    // touches a `\` directly followed by a newline, so it doesn't disturb
    // in-token escapes elsewhere — except the rare case of a literal
    // trailing backslash inside a single-quoted payload that itself spans
    // a line break, which is indistinguishable from a continuation here.
    let joined = content.replace("\\\r\n", "").replace("\\\n", "");
    let tokens = tokenize(&joined);
    let mut tokens = tokens.into_iter();
    let first = tokens.next();
    let rest: Vec<String> = match first {
        Some(t) if t == "curl" => tokens.collect(),
        Some(t) => std::iter::once(t).chain(tokens).collect(),
        None => Vec::new(),
    };

    let mut warnings = Vec::new();
    let mut method: Option<String> = None;
    let mut url: Option<String> = None;
    let mut headers: Vec<HeaderEntry> = Vec::new();
    let mut data_pieces: Vec<String> = Vec::new();
    let mut form_fields: Vec<FormField> = Vec::new();
    let mut user: Option<String> = None;
    let mut insecure = false;
    // Real curl defaults to *not* following redirects without `-L`, but
    // modeling that faithfully would silently change replay behavior for
    // the common case of a browser/Postman "copy as cURL" snippet that
    // omits `-L` even though the original request did follow redirects.
    // Default to this app's own `RequestOptions::default()` (true) instead,
    // consistent with every other option field here — `-L` is honored if
    // present but its absence isn't treated as a meaningful negative.
    let mut follow_redirects = true;
    let mut max_time: Option<u64> = None;
    let mut max_redirects: Option<usize> = None;

    let mut i = 0;
    while i < rest.len() {
        let tok = rest[i].as_str();
        match tok {
            "-X" | "--request" => {
                i += 1;
                method = rest.get(i).cloned();
            }
            "-H" | "--header" => {
                i += 1;
                if let Some(h) = rest.get(i) {
                    push_header(&mut headers, h, &mut warnings);
                }
            }
            "-d" | "--data" | "--data-raw" | "--data-binary" | "--data-ascii" | "--data-urlencode" => {
                i += 1;
                if let Some(d) = rest.get(i) {
                    data_pieces.push(d.clone());
                }
            }
            "-F" | "--form" => {
                i += 1;
                if let Some(f) = rest.get(i) {
                    form_fields.push(parse_form_field(f));
                }
            }
            "-u" | "--user" => {
                i += 1;
                user = rest.get(i).cloned();
            }
            "-b" | "--cookie" => {
                i += 1;
                if let Some(c) = rest.get(i) {
                    headers.push(HeaderEntry { name: "Cookie".into(), value: c.clone(), enabled: true });
                }
            }
            "-A" | "--user-agent" => {
                i += 1;
                if let Some(ua) = rest.get(i) {
                    headers.push(HeaderEntry { name: "User-Agent".into(), value: ua.clone(), enabled: true });
                }
            }
            "-e" | "--referer" => {
                i += 1;
                if let Some(r) = rest.get(i) {
                    headers.push(HeaderEntry { name: "Referer".into(), value: r.clone(), enabled: true });
                }
            }
            "--url" => {
                i += 1;
                url = rest.get(i).cloned();
            }
            "-m" | "--max-time" => {
                i += 1;
                max_time = rest.get(i).and_then(|s| s.parse::<f64>().ok()).map(|v| v.ceil() as u64);
            }
            "--max-redirs" => {
                i += 1;
                max_redirects = rest.get(i).and_then(|s| s.parse().ok());
            }
            "-I" | "--head" => method = Some("HEAD".into()),
            "-G" | "--get" => method = method.or_else(|| Some("GET".into())),
            "-k" | "--insecure" => insecure = true,
            "-L" | "--location" => follow_redirects = true,
            "--compressed" | "-s" | "--silent" | "-v" | "--verbose" | "-i" | "--include" | "-#" | "--progress-bar"
            | "-f" | "--fail" | "-S" | "--show-error" | "-N" | "--no-buffer" => {}
            other if other.starts_with('-') && other.len() > 1 => {
                warnings.push(format!("unsupported curl option \"{other}\" — ignored"));
            }
            other => {
                if url.is_none() {
                    url = Some(other.to_string());
                }
            }
        }
        i += 1;
    }

    let Some(raw_url) = url else {
        return Err(AppError::Other("no URL found in curl command".into()));
    };
    let (base, query) = split_url(&raw_url);

    let mut auth = RequestAuth::Inherit;
    if let Some(creds) = user {
        let (username, password) =
            creds.split_once(':').map(|(u, p)| (u.to_string(), p.to_string())).unwrap_or((creds, String::new()));
        auth = RequestAuth::Own(AuthConfig::Basic { username, password });
    } else if let Some(idx) =
        headers.iter().position(|h| h.name.eq_ignore_ascii_case("authorization") && h.value.starts_with("Bearer "))
    {
        let token = headers.remove(idx).value.trim_start_matches("Bearer ").to_string();
        auth = RequestAuth::Own(AuthConfig::Bearer { token, prefix: crate::model::auth::default_bearer_prefix() });
    }

    if data_pieces.iter().any(|d| d.starts_with('@')) {
        warnings.push("curl @file data reference was not read — body contains the literal \"@…\" token".into());
    }
    let content_type = headers.iter().find(|h| h.name.eq_ignore_ascii_case("content-type")).map(|h| h.value.clone());
    let body = if !form_fields.is_empty() {
        RequestBody::FormData(form_fields)
    } else if !data_pieces.is_empty() {
        build_data_body(&data_pieces, content_type.as_deref())
    } else {
        RequestBody::None
    };

    let method = method
        .unwrap_or_else(|| if matches!(body, RequestBody::None) { "GET".into() } else { "POST".into() })
        .to_uppercase();

    let mut options =
        RequestOptions { verify_ssl: !insecure, follow_redirects, ..RequestOptions::default() };
    if let Some(m) = max_redirects {
        options.max_redirects = m;
    }
    if let Some(t) = max_time {
        options.timeout_secs = t;
    }

    let name = derive_name(&method, &base);
    let request = ImportedRequest {
        name: name.clone(),
        method,
        url: base,
        headers,
        query,
        body,
        options,
        auth,
        pre_request_script: String::new(),
        post_response_script: String::new(),
        ..Default::default()
    };

    let root = ImportedNode { name, description: None, auth: AuthConfig::None, requests: vec![request], children: vec![] };
    Ok(ImportPreview::new(root, warnings))
}

pub fn export(node: &ImportedNode) -> AppResult<String> {
    let mut blocks = Vec::new();
    collect_curl_blocks(node, &mut blocks);
    if blocks.is_empty() {
        return Err(AppError::Other("nothing to export: no requests in this collection".into()));
    }
    Ok(blocks.join("\n\n"))
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// Minimal POSIX-ish shell tokenizer: splits on whitespace outside quotes,
/// honors single quotes (literal, no escapes), double quotes (`\` escapes
/// `"`/`\`/`$` only), and a bare `\` escaping the next character.
fn tokenize(s: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_token = false;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            ' ' | '\t' | '\n' | '\r' => {
                if in_token {
                    tokens.push(std::mem::take(&mut current));
                    in_token = false;
                }
            }
            '\'' => {
                in_token = true;
                for c2 in chars.by_ref() {
                    if c2 == '\'' {
                        break;
                    }
                    current.push(c2);
                }
            }
            '"' => {
                in_token = true;
                while let Some(c2) = chars.next() {
                    if c2 == '"' {
                        break;
                    }
                    if c2 == '\\' && matches!(chars.peek(), Some('"') | Some('\\') | Some('$')) {
                        current.push(chars.next().unwrap());
                        continue;
                    }
                    current.push(c2);
                }
            }
            '\\' => {
                in_token = true;
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            _ => {
                in_token = true;
                current.push(c);
            }
        }
    }
    if in_token {
        tokens.push(current);
    }
    tokens
}

fn push_header(headers: &mut Vec<HeaderEntry>, raw: &str, warnings: &mut Vec<String>) {
    match raw.split_once(':') {
        Some((name, value)) => {
            headers.push(HeaderEntry { name: name.trim().to_string(), value: value.trim().to_string(), enabled: true })
        }
        None => warnings.push(format!("malformed header \"{raw}\" — ignored")),
    }
}

/// `key=value`, `key=@/path/to/file` (file upload), optionally with a
/// trailing `;type=mime` suffix on the value.
fn parse_form_field(raw: &str) -> FormField {
    let (key, rest) = raw.split_once('=').unwrap_or((raw, ""));
    let (value_part, content_type) = match rest.split_once(";type=") {
        Some((v, ct)) => (v, Some(ct.to_string())),
        None => (rest, None),
    };
    match value_part.strip_prefix('@') {
        Some(path) => FormField { key: key.to_string(), value: path.to_string(), enabled: true, is_file: true, content_type },
        None => FormField { key: key.to_string(), value: value_part.to_string(), enabled: true, is_file: false, content_type },
    }
}

fn split_url(raw: &str) -> (String, Vec<KeyValue>) {
    match raw.split_once('?') {
        Some((base, qs)) => {
            let query = qs
                .split('&')
                .filter(|s| !s.is_empty())
                .map(|pair| match pair.split_once('=') {
                    Some((k, v)) => KeyValue { key: k.to_string(), value: v.to_string(), enabled: true },
                    None => KeyValue { key: pair.to_string(), value: String::new(), enabled: true },
                })
                .collect();
            (base.to_string(), query)
        }
        None => (raw.to_string(), Vec::new()),
    }
}

/// Multiple `-d`/`--data*` occurrences are joined with `&`, matching curl's
/// own behavior of merging repeated data pieces — see `man curl`. Content
/// type then decides how the joined string is structured: explicit `json`
/// wins, urlencoded is curl's actual default when no `Content-Type` header
/// was set, anything else is treated as an opaque raw body.
fn build_data_body(pieces: &[String], content_type: Option<&str>) -> RequestBody {
    let joined = pieces.join("&");
    let ct = content_type.unwrap_or_default().to_ascii_lowercase();
    if ct.contains("json") {
        RequestBody::Json(joined)
    } else if ct.is_empty() || ct.contains("x-www-form-urlencoded") {
        RequestBody::UrlEncoded(
            joined
                .split('&')
                .filter(|s| !s.is_empty())
                .map(|pair| match pair.split_once('=') {
                    Some((k, v)) => KeyValue { key: k.to_string(), value: v.to_string(), enabled: true },
                    None => KeyValue { key: pair.to_string(), value: String::new(), enabled: true },
                })
                .collect(),
        )
    } else {
        let language = ct.split('/').nth(1).map(str::to_string);
        RequestBody::Raw { content: joined, language }
    }
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

fn collect_curl_blocks(node: &ImportedNode, blocks: &mut Vec<String>) {
    for req in &node.requests {
        blocks.push(build_curl_block(req));
    }
    for child in &node.children {
        collect_curl_blocks(child, blocks);
    }
}

fn build_curl_block(req: &ImportedRequest) -> String {
    let mut lines = vec![format!("curl -X {} {}", req.method, quote(&url_with_query(&req.url, &req.query)))];

    if !req.options.verify_ssl {
        lines.push("  -k".to_string());
    }
    if req.options.follow_redirects {
        lines.push("  -L".to_string());
    }
    lines.push(format!("  --max-time {}", req.options.timeout_secs));
    lines.push(format!("  --max-redirs {}", req.options.max_redirects));

    let mut headers = req.headers.clone();
    if let RequestAuth::Own(cfg) = &req.auth {
        match cfg {
            AuthConfig::Basic { username, password } => {
                lines.push(format!("  -u {}", quote(&format!("{username}:{password}"))));
            }
            AuthConfig::Bearer { token, prefix } => {
                headers.push(HeaderEntry { name: "Authorization".into(), value: crate::model::auth::bearer_header_value(prefix, token), enabled: true });
            }
            AuthConfig::ApiKey { key, value, location } if *location == ApiKeyLocation::Header => {
                headers.push(HeaderEntry { name: key.clone(), value: value.clone(), enabled: true });
            }
            AuthConfig::None | AuthConfig::ApiKey { .. } | AuthConfig::OAuth2(_) | AuthConfig::AwsSigV4(_) => {}
        }
    }

    for h in headers.iter().filter(|h| h.enabled) {
        lines.push(format!("  -H {}", quote(&format!("{}: {}", h.name, h.value))));
    }

    match &req.body {
        RequestBody::None => {}
        RequestBody::Json(content) => {
            if !has_content_type(&headers) {
                lines.push(format!("  -H {}", quote("Content-Type: application/json")));
            }
            lines.push(format!("  --data-raw {}", quote(content)));
        }
        RequestBody::Raw { content, .. } => {
            lines.push(format!("  --data-raw {}", quote(content)));
        }
        RequestBody::UrlEncoded(list) => {
            let joined =
                list.iter().filter(|kv| kv.enabled).map(|kv| format!("{}={}", kv.key, kv.value)).collect::<Vec<_>>().join("&");
            lines.push(format!("  --data-raw {}", quote(&joined)));
        }
        RequestBody::FormData(list) => {
            for f in list.iter().filter(|f| f.enabled) {
                let raw =
                    if f.is_file { format!("{}=@{}", f.key, f.value) } else { format!("{}={}", f.key, f.value) };
                lines.push(format!("  -F {}", quote(&raw)));
            }
        }
        RequestBody::Binary { path } => {
            lines.push(format!("  --data-binary {}", quote(&format!("@{path}"))));
        }
        RequestBody::Graphql { query, variables, operation_name } => {
            if !has_content_type(&headers) {
                lines.push(format!("  -H {}", quote("Content-Type: application/json")));
            }
            let mut parts = vec![format!("\"query\":{}", json_string(query))];
            if let Some(v) = variables {
                parts.push(format!("\"variables\":{v}"));
            }
            if let Some(name) = operation_name {
                if !name.trim().is_empty() {
                    parts.push(format!("\"operationName\":{}", json_string(name)));
                }
            }
            let body = format!("{{{}}}", parts.join(","));
            lines.push(format!("  --data-raw {}", quote(&body)));
        }
    }

    lines.join(" \\\n")
}

fn has_content_type(headers: &[HeaderEntry]) -> bool {
    headers.iter().any(|h| h.name.eq_ignore_ascii_case("content-type"))
}

/// Percent-encodes query params the same way `engine::http::build_url` does
/// (via `Url::query_pairs_mut`), mirroring `codegen::full_url` — so a value
/// with a space/`&`/`#` renders into the exported curl command exactly as it
/// would actually be sent, not as a string that silently turns into a
/// different request once it hits the wire.
fn url_with_query(base: &str, query: &[KeyValue]) -> String {
    let enabled: Vec<&KeyValue> = query.iter().filter(|q| q.enabled).collect();
    if enabled.is_empty() {
        return base.to_string();
    }
    if let Ok(mut url) = reqwest::Url::parse(base.trim()) {
        {
            let mut pairs = url.query_pairs_mut();
            for q in &enabled {
                pairs.append_pair(&q.key, &q.value);
            }
        }
        return url.to_string();
    }
    // Most likely an in-progress URL still containing an unresolved
    // `{{var}}` — fall back to a naive join so export still produces
    // *something* rather than erroring over a URL that isn't a real target
    // yet.
    let qs: String = enabled.iter().map(|q| format!("{}={}", q.key, q.value)).collect::<Vec<_>>().join("&");
    format!("{base}?{qs}")
}

/// Single-quotes `s` for safe inclusion in a shell command, using the
/// standard POSIX `'\''` trick to embed a literal single quote.
fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn json_string(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| format!("\"{s}\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_realistic_curl_command_with_expected_shape() {
        let cmd = r#"curl -X POST 'https://petstore.example.com/pets?verbose=true' \
  -H 'Accept: application/json' \
  -H 'Authorization: Bearer abc123' \
  -H 'Content-Type: application/json' \
  --data-raw '{"name":"Fido"}' \
  --max-time 15 \
  --max-redirs 3 \
  -L \
  -k"#;
        let preview = parse(cmd).unwrap();
        let req = &preview.root.requests[0];
        assert_eq!(req.method, "POST");
        assert_eq!(req.url, "https://petstore.example.com/pets");
        assert_eq!(req.query, vec![KeyValue { key: "verbose".into(), value: "true".into(), enabled: true }]);
        assert_eq!(
            req.headers,
            vec![
                HeaderEntry { name: "Accept".into(), value: "application/json".into(), enabled: true },
                HeaderEntry { name: "Content-Type".into(), value: "application/json".into(), enabled: true },
            ]
        );
        assert_eq!(req.auth, RequestAuth::Own(AuthConfig::Bearer { token: "abc123".into(), prefix: crate::model::auth::default_bearer_prefix() }));
        assert_eq!(req.body, RequestBody::Json("{\"name\":\"Fido\"}".into()));
        assert_eq!(req.options.timeout_secs, 15);
        assert_eq!(req.options.max_redirects, 3);
        assert!(req.options.follow_redirects);
        assert!(!req.options.verify_ssl);
        assert_eq!(preview.root.requests.len(), 1);
        assert!(preview.root.children.is_empty());
        assert_eq!(preview.stats.folders, 0);
        assert_eq!(preview.stats.requests, 1);
    }

    #[test]
    fn parses_basic_auth_and_multipart_form() {
        let cmd = "curl -u alice:secret -F 'caption=cute dog' -F 'photo=@/tmp/fido.png' 'https://petstore.example.com/pets/123/photo'";
        let preview = parse(cmd).unwrap();
        let req = &preview.root.requests[0];
        assert_eq!(req.method, "POST"); // no -X, but form data implies POST
        assert_eq!(req.auth, RequestAuth::Own(AuthConfig::Basic { username: "alice".into(), password: "secret".into() }));
        assert_eq!(
            req.body,
            RequestBody::FormData(vec![
                FormField { key: "caption".into(), value: "cute dog".into(), enabled: true, is_file: false, content_type: None },
                FormField { key: "photo".into(), value: "/tmp/fido.png".into(), enabled: true, is_file: true, content_type: None },
            ])
        );
    }

    #[test]
    fn unsupported_flag_warns_but_does_not_fail() {
        let preview = parse("curl --digest -u x:y https://a.test").unwrap();
        assert!(preview.warnings.iter().any(|w| w.contains("--digest")));
        assert_eq!(
            preview.root.requests[0].auth,
            RequestAuth::Own(AuthConfig::Basic { username: "x".into(), password: "y".into() })
        );
    }

    #[test]
    fn get_with_no_flags_defaults_method_and_options() {
        let preview = parse("curl https://a.test/items").unwrap();
        let req = &preview.root.requests[0];
        assert_eq!(req.method, "GET");
        assert_eq!(req.name, "GET /items");
        assert_eq!(req.body, RequestBody::None);
        assert_eq!(req.options, RequestOptions::default());
    }

    #[test]
    fn exports_request_to_well_formed_curl_command() {
        let req = ImportedRequest {
            name: "Get thing".into(),
            method: "GET".into(),
            url: "https://api.test/items".into(),
            headers: vec![HeaderEntry { name: "Accept".into(), value: "application/json".into(), enabled: true }],
            query: vec![KeyValue { key: "limit".into(), value: "5".into(), enabled: true }],
            body: RequestBody::None,
            options: RequestOptions::default(),
            auth: RequestAuth::Own(AuthConfig::Bearer { token: "tok".into(), prefix: crate::model::auth::default_bearer_prefix() }),
            pre_request_script: String::new(),
            post_response_script: String::new(),
            ..Default::default()
        };
        let node = ImportedNode { name: "x".into(), description: None, auth: AuthConfig::None, requests: vec![req], children: vec![] };
        let out = export(&node).unwrap();
        assert_eq!(
            out,
            "curl -X GET 'https://api.test/items?limit=5' \\\n  -L \\\n  --max-time 30 \\\n  --max-redirs 10 \\\n  -H 'Accept: application/json' \\\n  -H 'Authorization: Bearer tok'"
        );
    }

    #[test]
    fn url_with_query_percent_encodes_special_characters() {
        let query = vec![KeyValue { key: "q".into(), value: "a&b c".into(), enabled: true }];
        let out = url_with_query("https://api.example.com/items", &query);
        assert!(!out.contains("a&b c"), "{out}");
        let reparsed = reqwest::Url::parse(&out).unwrap();
        let pairs: Vec<(String, String)> = reparsed.query_pairs().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        assert_eq!(pairs, vec![("q".to_string(), "a&b c".to_string())]);
    }

    #[test]
    fn url_with_query_falls_back_to_naive_join_for_unparseable_base() {
        let query = vec![KeyValue { key: "limit".into(), value: "5".into(), enabled: true }];
        assert_eq!(url_with_query("{{base_url}}/items", &query), "{{base_url}}/items?limit=5");
    }

    #[test]
    fn curl_command_round_trips_through_parse_export_parse() {
        let cmd = r#"curl -X POST 'https://petstore.example.com/pets?verbose=true' \
  -H 'Accept: application/json' \
  -H 'Authorization: Bearer abc123' \
  -H 'Content-Type: application/json' \
  --data-raw '{"name":"Fido"}' \
  --max-time 15 \
  --max-redirs 3 \
  -L \
  -k"#;
        let preview_a = parse(cmd).unwrap();
        let exported = export(&preview_a.root).unwrap();
        let preview_b = parse(&exported).unwrap();
        assert_eq!(preview_a.root, preview_b.root);
    }

    #[test]
    fn multiline_curl_with_backslash_continuations_parses_same_as_one_line() {
        let multiline = "curl -X GET \\\n  'https://a.test/x' \\\n  -H 'Accept: text/plain'";
        let oneline = "curl -X GET 'https://a.test/x' -H 'Accept: text/plain'";
        assert_eq!(parse(multiline).unwrap().root, parse(oneline).unwrap().root);
    }
}
