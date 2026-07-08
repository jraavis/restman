//! Source-code generation: turn a resolved `HttpRequest` into a runnable
//! snippet in one of several target languages. Mirrors `interop`'s
//! per-format sibling-module dispatch, but the direction only goes one way
//! (request -> code) and the input is a single `HttpRequest`, not an IR tree.
//!
//! This module is deliberately pure: no DB, no keychain, no network, no
//! clock — same inputs always give the same output, which is what makes the
//! per-language known-answer tests possible. `commands::codegen` is the
//! thin, stateful wrapper that resolves variables and auth (mirroring
//! `commands::http::send_request`'s non-network steps) before calling in
//! here. That boundary is why `OAuth2` reaching `plan_auth` is treated as a
//! programming error rather than a case to handle (the caller must collapse
//! it to `Bearer` first, real token or placeholder) and why AWS SigV4 never
//! gets a live signature baked into generated code — `sign_headers` needs
//! `SystemTime::now()`, which would make output that's stale within minutes
//! and break purity besides.

pub mod csharp;
pub mod curl;
pub mod go;
pub mod java;
pub mod javascript;
pub mod php;
pub mod plugin;
pub mod python;
pub mod ruby;
pub mod rust;

use crate::error::{AppError, AppResult};
use crate::model::auth::{ApiKeyLocation, AuthConfig};
use crate::model::http::{HttpRequest, RequestBody};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodeLanguage {
    Curl,
    JavascriptFetch,
    Python,
    Go,
    Rust,
    Php,
    Java,
    Csharp,
    Ruby,
}

/// What `commands::codegen::generate_code` dispatches to: one of the 9
/// native compiled-Rust languages, or a workspace's stored codegen plugin
/// by id. A tagged enum rather than two `Option` siblings so "exactly one
/// of these" is enforced by the type itself rather than a runtime check —
/// also keeps `generate_code`'s argument count under clippy's
/// `too_many_arguments` threshold.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CodegenTarget {
    Native { language: CodeLanguage },
    /// `pluginId` on the wire — the frontend is camelCase throughout; Tauri
    /// only auto-converts top-level command args, not nested enum fields.
    Plugin { #[serde(rename = "pluginId")] plugin_id: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodegenOptions {
    #[serde(default = "crate::model::http::default_true")]
    pub include_auth: bool,
    #[serde(default = "crate::model::http::default_true")]
    pub include_headers: bool,
}

impl Default for CodegenOptions {
    fn default() -> Self {
        Self { include_auth: true, include_headers: true }
    }
}

/// Generate `language`'s source for `req`. Pure: same inputs always give the
/// same output regardless of DB/keychain/clock state — callers that want
/// "what would actually be sent" must resolve vars/auth into `req` before
/// calling (see `commands::codegen::generate_code`).
pub fn generate(language: CodeLanguage, req: &HttpRequest, options: &CodegenOptions) -> AppResult<String> {
    match language {
        CodeLanguage::Curl => curl::generate(req, options),
        CodeLanguage::JavascriptFetch => javascript::generate(req, options),
        CodeLanguage::Python => python::generate(req, options),
        CodeLanguage::Go => go::generate(req, options),
        CodeLanguage::Rust => rust::generate(req, options),
        CodeLanguage::Php => php::generate(req, options),
        CodeLanguage::Java => java::generate(req, options),
        CodeLanguage::Csharp => csharp::generate(req, options),
        CodeLanguage::Ruby => ruby::generate(req, options),
    }
}

/// Placeholder baked into an OAuth2-derived Bearer token when no fresh
/// cached token is available — `commands::codegen` substitutes a real token
/// when one exists. Visually distinct from a real token so a user pasting
/// the generated code notices it needs filling in.
pub const OAUTH2_TOKEN_PLACEHOLDER: &str = "<OAUTH2_ACCESS_TOKEN>";

/// What a resolved `AuthConfig` contributes to generated code: a header, a
/// query param, nothing (no auth), or a case that can't be reproduced as a
/// static value (AWS SigV4) and gets an explanatory comment instead.
#[derive(Debug)]
pub(crate) enum AuthPlan {
    None,
    Header(String, String),
    Query(String, String),
    Unsigned(String),
}

/// `OAuth2` must already be collapsed to `Bearer` (real token or
/// `OAUTH2_TOKEN_PLACEHOLDER`) by the caller before `req` reaches here —
/// mirrors `engine::http::apply_auth`'s contract that OAuth2 reaching a
/// DB-free layer is a programming error, not a runtime case to handle.
pub(crate) fn plan_auth(auth: &AuthConfig) -> AppResult<AuthPlan> {
    Ok(match auth {
        AuthConfig::None => AuthPlan::None,
        AuthConfig::Bearer { token, prefix } => AuthPlan::Header("Authorization".into(), crate::model::auth::bearer_header_value(prefix, token)),
        AuthConfig::Basic { username, password } => {
            let encoded = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, format!("{username}:{password}"));
            AuthPlan::Header("Authorization".into(), format!("Basic {encoded}"))
        }
        AuthConfig::ApiKey { key, value, location } => match location {
            ApiKeyLocation::Header => AuthPlan::Header(key.clone(), value.clone()),
            ApiKeyLocation::Query => AuthPlan::Query(key.clone(), value.clone()),
        },
        AuthConfig::AwsSigV4(cfg) => AuthPlan::Unsigned(format!(
            "AWS SigV4 auth is configured (access key {}, region {}, service {}) but a signature is time-limited and can't be baked into static code — sign this request at send time with your SDK's SigV4 support.",
            display_or(&cfg.access_key, "<unset>"),
            display_or(&cfg.region, "<unset>"),
            display_or(&cfg.service, "<unset>"),
        )),
        AuthConfig::OAuth2(_) => {
            return Err(AppError::Other("codegen: OAuth2 must be collapsed to Bearer before code generation".into()));
        }
    })
}

fn display_or<'a>(s: &'a str, fallback: &'a str) -> &'a str {
    if s.is_empty() {
        fallback
    } else {
        s
    }
}

pub(crate) fn effective_headers(req: &HttpRequest, options: &CodegenOptions) -> AppResult<Vec<(String, String)>> {
    let mut headers: Vec<(String, String)> = if options.include_headers {
        req.headers
            .iter()
            .filter(|h| h.enabled && !h.name.trim().is_empty())
            .map(|h| (h.name.clone(), h.value.clone()))
            .collect()
    } else {
        Vec::new()
    };
    if options.include_auth {
        if let AuthPlan::Header(k, v) = plan_auth(&req.auth)? {
            headers.push((k, v));
        }
    }
    Ok(headers)
}

pub(crate) fn effective_query(req: &HttpRequest, options: &CodegenOptions) -> AppResult<Vec<(String, String)>> {
    let mut query: Vec<(String, String)> = req
        .query
        .iter()
        .filter(|q| q.enabled && !q.key.is_empty())
        .map(|q| (q.key.clone(), q.value.clone()))
        .collect();
    if options.include_auth {
        if let AuthPlan::Query(k, v) = plan_auth(&req.auth)? {
            query.push((k, v));
        }
    }
    Ok(query)
}

/// A leading comment line when auth can't be reproduced as a static
/// header/query value (currently: AWS SigV4 only). `None` when there's
/// nothing to flag, including whenever `include_auth` is off.
pub(crate) fn auth_note(req: &HttpRequest, options: &CodegenOptions) -> AppResult<Option<String>> {
    if !options.include_auth {
        return Ok(None);
    }
    Ok(match plan_auth(&req.auth)? {
        AuthPlan::Unsigned(note) => Some(note),
        _ => None,
    })
}

/// Percent-encodes query params the same way `engine::http::build_url` does
/// (via `Url::query_pairs_mut`), so a value with a space/`&`/`#` renders into
/// generated code exactly as it would actually be sent — not as a string
/// that silently turns into a different request once it hits the wire.
pub(crate) fn full_url(req: &HttpRequest, query: &[(String, String)]) -> String {
    if query.is_empty() {
        return req.url.clone();
    }
    match reqwest::Url::parse(req.url.trim()) {
        Ok(mut url) => {
            {
                let mut pairs = url.query_pairs_mut();
                for (k, v) in query {
                    pairs.append_pair(k, v);
                }
            }
            url.to_string()
        }
        // Most likely an in-progress URL still containing an unresolved
        // `{{var}}` (live preview re-generates on every keystroke) — fall
        // back to a naive join so the preview still shows *something*
        // rather than erroring the whole panel over a URL that isn't a real
        // target yet.
        Err(_) => {
            let qs = query.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("&");
            if req.url.contains('?') {
                format!("{}&{}", req.url, qs)
            } else {
                format!("{}?{}", req.url, qs)
            }
        }
    }
}

pub(crate) fn has_header(headers: &[(String, String)], name: &str) -> bool {
    headers.iter().any(|(k, _)| k.eq_ignore_ascii_case(name))
}

/// A body collapsed to the shape every language actually needs to render:
/// full `RequestBody` variants minus the format-specific details that don't
/// change how the *code* is generated (`Json` vs `UrlEncoded` are both just
/// "send this text with this content-type").
pub(crate) enum BodyPlan {
    None,
    Text { content: String, content_type: Option<&'static str> },
    FormData(Vec<(String, String, bool)>),
    Binary(String),
}

pub(crate) fn plan_body(body: &RequestBody) -> BodyPlan {
    match body {
        RequestBody::None => BodyPlan::None,
        RequestBody::Json(s) => BodyPlan::Text { content: s.clone(), content_type: Some("application/json") },
        RequestBody::Raw { content, .. } => BodyPlan::Text { content: content.clone(), content_type: None },
        RequestBody::UrlEncoded(list) => {
            let joined = list
                .iter()
                .filter(|kv| kv.enabled)
                .map(|kv| format!("{}={}", kv.key, kv.value))
                .collect::<Vec<_>>()
                .join("&");
            BodyPlan::Text { content: joined, content_type: Some("application/x-www-form-urlencoded") }
        }
        RequestBody::FormData(fields) => BodyPlan::FormData(
            fields.iter().filter(|f| f.enabled).map(|f| (f.key.clone(), f.value.clone(), f.is_file)).collect(),
        ),
        RequestBody::Binary { path } => BodyPlan::Binary(path.clone()),
        RequestBody::Graphql { query, variables, operation_name } => {
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
            BodyPlan::Text { content: body, content_type: Some("application/json") }
        }
    }
}

fn json_string(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_default()
}

/// Valid escaped literal (with surrounding quotes) for any language whose
/// double-quoted strings follow JSON's own backslash-escape rules with no
/// interpolation character — JS, Python, Go, Rust, Java, and C# all qualify.
pub(crate) fn dquote(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| format!("\"{s}\""))
}

/// Single-quoted literal for languages where `'...'` performs no
/// interpolation (so a literal `$`/`#{` is safe) but still treats `\` and
/// `'` as meaningful — PHP and Ruby.
pub(crate) fn squote(s: &str) -> String {
    format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// POSIX shell single-quoting — backslash is literal inside single quotes,
/// so only the quote character itself needs escaping (the standard `'\''`
/// trick).
pub(crate) fn shquote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[cfg(test)]
pub(crate) fn sample_get_request() -> HttpRequest {
    HttpRequest {
        method: "GET".into(),
        url: "https://api.example.com/items".into(),
        headers: vec![crate::model::http::HeaderEntry {
            name: "Accept".into(),
            value: "application/json".into(),
            enabled: true,
        }],
        query: vec![crate::model::http::KeyValue { key: "limit".into(), value: "5".into(), enabled: true }],
        auth: AuthConfig::Bearer { token: "tok123".into(), prefix: crate::model::auth::default_bearer_prefix() },
        ..Default::default()
    }
}

/// Body deliberately contains a quote, a backslash, and a real newline —
/// the three characters every per-language escaper must handle — kept to
/// otherwise-trivial filler (`a`/`b`/`c`/`d`) so expected test output is
/// hand-verifiable instead of requiring a JSON document to be re-derived by
/// eye.
#[cfg(test)]
pub(crate) fn sample_json_post_request() -> HttpRequest {
    HttpRequest {
        method: "POST".into(),
        url: "https://api.example.com/items".into(),
        body: RequestBody::Json("a\"b\\c\nd".into()),
        auth: AuthConfig::Basic { username: "alice".into(), password: "secret".into() },
        ..Default::default()
    }
}

#[cfg(test)]
pub(crate) fn sample_sigv4_request() -> HttpRequest {
    HttpRequest {
        method: "GET".into(),
        url: "https://example.execute-api.us-east-1.amazonaws.com/prod/items".into(),
        auth: AuthConfig::AwsSigV4(crate::model::auth::AwsSigV4Config {
            access_key: "AKIA_TEST".into(),
            secret_key: "shh".into(),
            region: "us-east-1".into(),
            service: "execute-api".into(),
            session_token: String::new(),
        }),
        ..Default::default()
    }
}

/// Mixed text + file field — the multipart path every per-language generator
/// has to special-case for the file branch (filename handling, stream/file
/// APIs) in a way the other fixtures never exercise.
#[cfg(test)]
pub(crate) fn sample_formdata_request() -> HttpRequest {
    HttpRequest {
        method: "POST".into(),
        url: "https://api.example.com/upload".into(),
        body: RequestBody::FormData(vec![
            crate::model::http::FormField {
                key: "caption".into(),
                value: "cute dog".into(),
                enabled: true,
                is_file: false,
                content_type: None,
            },
            crate::model::http::FormField {
                key: "photo".into(),
                value: "/tmp/fido.png".into(),
                enabled: true,
                is_file: true,
                content_type: None,
            },
        ]),
        ..Default::default()
    }
}

#[cfg(test)]
pub(crate) fn sample_binary_request() -> HttpRequest {
    HttpRequest {
        method: "POST".into(),
        url: "https://api.example.com/upload".into(),
        body: RequestBody::Binary { path: "/tmp/payload.bin".into() },
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oauth2_reaching_plan_auth_is_a_programming_error() {
        let err = plan_auth(&AuthConfig::OAuth2(Default::default())).unwrap_err();
        assert!(err.to_string().contains("collapsed"));
    }

    #[test]
    fn aws_sigv4_never_yields_a_header_or_query_value() {
        let plan = plan_auth(&sample_sigv4_request().auth).unwrap();
        assert!(matches!(plan, AuthPlan::Unsigned(_)));
    }

    #[test]
    fn include_auth_false_drops_auth_header() {
        let req = sample_get_request();
        let options = CodegenOptions { include_auth: false, include_headers: true };
        let headers = effective_headers(&req, &options).unwrap();
        assert!(!headers.iter().any(|(k, _)| k == "Authorization"));
    }

    #[test]
    fn include_headers_false_drops_user_headers_but_keeps_auth() {
        let req = sample_get_request();
        let options = CodegenOptions { include_auth: true, include_headers: false };
        let headers = effective_headers(&req, &options).unwrap();
        assert_eq!(headers, vec![("Authorization".to_string(), "Bearer tok123".to_string())]);
    }

    #[test]
    fn full_url_percent_encodes_query_values_with_special_characters() {
        let req = HttpRequest { url: "https://api.example.com/items".into(), ..Default::default() };
        let query = vec![("q".to_string(), "a&b c".to_string())];
        let out = full_url(&req, &query);
        assert!(!out.contains("a&b c"), "{out}");
        let reparsed = reqwest::Url::parse(&out).unwrap();
        let pairs: Vec<(String, String)> = reparsed.query_pairs().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        assert_eq!(pairs, vec![("q".to_string(), "a&b c".to_string())]);
    }

    #[test]
    fn full_url_falls_back_to_naive_join_for_an_unparseable_url() {
        let req = HttpRequest { url: "{{base_url}}/items".into(), ..Default::default() };
        let query = vec![("limit".to_string(), "5".to_string())];
        assert_eq!(full_url(&req, &query), "{{base_url}}/items?limit=5");
    }

    // Per-language generators trust these three primitives as oracles
    // rather than re-deriving escaped output by hand in every test module —
    // pinned precisely here so that trust is warranted.
    #[test]
    fn dquote_matches_json_string_escaping() {
        assert_eq!(dquote("a\"b\\c\nd"), serde_json::to_string("a\"b\\c\nd").unwrap());
        assert_eq!(dquote("hello"), "\"hello\"");
    }

    #[test]
    fn squote_escapes_backslash_and_quote_only() {
        assert_eq!(squote("a\"b\\c"), "'a\"b\\\\c'");
    }

    #[test]
    fn shquote_only_escapes_single_quote() {
        assert_eq!(shquote("a'b\\c"), "'a'\\''b\\c'");
    }

    #[test]
    fn codegen_target_deserializes_frontend_camel_case_plugin_id() {
        let target: CodegenTarget =
            serde_json::from_str(r#"{"kind":"plugin","pluginId":"plug-1"}"#).unwrap();
        assert!(matches!(target, CodegenTarget::Plugin { plugin_id } if plugin_id == "plug-1"));
    }
}
