//! Local mock-server hosting: binds a configured `MockRule` list to a real
//! loopback socket and answers matching requests with the configured canned
//! response. `serve()`/`RunningMockServer` are re-homed here from the 17c-
//! style feasibility spike (see the previous commit) now that the
//! rule-matching `Router` construction sits alongside them.

use crate::model::{BodyMatchMode, BodyMatcher, MockMatcher, MockRule};
use crate::vars::interpolate;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use axum::Router;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

/// A running mock server. `addr` is whatever the bind actually resolved to
/// (the caller-requested port, or an OS-assigned one if the caller asked for
/// port 0 — tests do this to avoid colliding with each other).
pub struct RunningMockServer {
    pub addr: std::net::SocketAddr,
    handle: JoinHandle<()>,
}

impl RunningMockServer {
    pub fn abort(&self) {
        self.handle.abort();
    }
}

/// Binds `router` to `127.0.0.1:<port>` and starts serving in a spawned
/// task. Never `0.0.0.0` — this is a privacy-first app, a mock server is a
/// local dev tool, not something meant to be reachable from the LAN.
pub async fn serve(router: Router, port: u16) -> std::io::Result<RunningMockServer> {
    let listener = TcpListener::bind(("127.0.0.1", port)).await?;
    let addr = listener.local_addr()?;
    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    Ok(RunningMockServer { addr, handle })
}

/// If the pattern's segments align with the path's, returns the `:name` ->
/// segment-value captures (empty map if the pattern has no `:name`
/// segments); `None` if they don't align. Segment counts must match exactly
/// (no wildcard-tail matching).
fn path_matches(pattern: &str, path: &str) -> Option<HashMap<String, String>> {
    let pattern_segs: Vec<&str> = pattern.trim_matches('/').split('/').collect();
    let path_segs: Vec<&str> = path.trim_matches('/').split('/').collect();
    if pattern_segs.len() != path_segs.len() {
        return None;
    }
    let mut captures = HashMap::new();
    for (p, s) in pattern_segs.iter().zip(path_segs.iter()) {
        match p.strip_prefix(':') {
            Some(name) => {
                captures.insert(name.to_string(), s.to_string());
            }
            None if p == s => {}
            None => return None,
        }
    }
    Some(captures)
}

/// Decodes a raw (percent-encoded) query string into name->value pairs.
/// Repeated names: last occurrence wins (`HashMap` insert overwrites) — good
/// enough for rule matching, not a general multi-value query API.
fn parse_query(query: &str) -> HashMap<String, String> {
    form_urlencoded::parse(query.as_bytes()).into_owned().collect()
}

/// A matcher with an empty `name` is ignored, not treated as "must match an
/// empty-name param/header" — the UI's "Add row" persists a blank row the
/// instant it's clicked, before the user types a name, and that transient
/// state must not make an otherwise-fine rule stop matching everything.
fn matches_query(matchers: &[MockMatcher], query: &HashMap<String, String>) -> bool {
    matchers
        .iter()
        .filter(|m| m.enabled && !m.name.is_empty())
        .all(|m| query.get(&m.name).is_some_and(|v| v == &m.value))
}

fn matches_headers(matchers: &[MockMatcher], headers: &HeaderMap) -> bool {
    matchers.iter().filter(|m| m.enabled && !m.name.is_empty()).all(|m| {
        headers.get(m.name.as_str()).and_then(|v| v.to_str().ok()).is_some_and(|v| v == m.value)
    })
}

/// String rendering of a JSON scalar for `BodyMatchMode::JsonEquals`
/// comparison; `Null`/arrays/objects have no unambiguous string form, so
/// they never match.
fn json_scalar_as_str(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn matches_body(matcher: &Option<BodyMatcher>, body: &str) -> bool {
    let Some(m) = matcher else { return true };
    match m.mode {
        BodyMatchMode::Contains => body.contains(&m.value),
        BodyMatchMode::JsonEquals => {
            let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else { return false };
            let mut current = &value;
            for key in m.json_path.split('.').filter(|k| !k.is_empty()) {
                match current.get(key) {
                    Some(next) => current = next,
                    None => return false,
                }
            }
            json_scalar_as_str(current).is_some_and(|s| s == m.value)
        }
    }
}

/// First rule (in the caller's ordering — callers pass rules pre-sorted by
/// `sort_order`) whose method, path pattern, and every enabled query/header/
/// body matcher all match. Returns the rule plus its path pattern's `:name`
/// captures, for response templating. Pure and independently testable
/// without a server.
pub fn match_rule<'a>(
    rules: &'a [MockRule],
    method: &str,
    path: &str,
    query: &HashMap<String, String>,
    headers: &HeaderMap,
    body: &str,
) -> Option<(&'a MockRule, HashMap<String, String>)> {
    rules.iter().find_map(|r| {
        let method_matches = r.method.as_deref().is_none_or(|m| m.eq_ignore_ascii_case(method));
        if !method_matches {
            return None;
        }
        let captures = path_matches(&r.path_pattern, path)?;
        if !matches_query(&r.query_matchers, query) {
            return None;
        }
        if !matches_headers(&r.header_matchers, headers) {
            return None;
        }
        if !matches_body(&r.body_matcher, body) {
            return None;
        }
        Some((r, captures))
    })
}

async fn handle_any(
    State(rules): State<Arc<Vec<MockRule>>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let query = uri.query().map(parse_query).unwrap_or_default();
    let body_text = String::from_utf8_lossy(&body);
    match match_rule(&rules, method.as_str(), uri.path(), &query, &headers, &body_text) {
        Some((rule, captures)) => {
            if rule.delay_ms > 0 {
                tokio::time::sleep(Duration::from_millis(rule.delay_ms)).await;
            }
            let mut builder = Response::builder().status(rule.status);
            for h in rule.headers.iter().filter(|h| h.enabled) {
                builder = builder.header(&h.name, interpolate(&h.value, &captures));
            }
            let response_body = interpolate(&rule.body, &captures);
            builder.body(axum::body::Body::from(response_body)).unwrap_or_else(|_| {
                (StatusCode::INTERNAL_SERVER_ERROR, "invalid mock rule headers").into_response()
            })
        }
        None => (StatusCode::NOT_FOUND, "no mock rule matched").into_response(),
    }
}

/// Builds the router a mock server actually serves: one fallback route that
/// matches every incoming method+path against `rules` (pre-sorted by the
/// caller — `store::mock_servers::list_rules` already orders by
/// `sort_order`), since the rule set is per-workspace/runtime data, not
/// something expressible as compile-time axum routes.
pub fn build_router(rules: Vec<MockRule>) -> Router {
    Router::new().fallback(any(handle_any)).with_state(Arc::new(rules))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::http::HeaderEntry;

    fn rule(method: Option<&str>, path: &str, status: u16, body: &str) -> MockRule {
        MockRule {
            id: "r1".into(),
            mock_server_id: "s1".into(),
            method: method.map(str::to_string),
            path_pattern: path.into(),
            status,
            headers: Vec::new(),
            body: body.into(),
            delay_ms: 0,
            sort_order: 0,
            query_matchers: Vec::new(),
            header_matchers: Vec::new(),
            body_matcher: None,
        }
    }

    fn no_query() -> HashMap<String, String> {
        HashMap::new()
    }

    fn no_headers() -> HeaderMap {
        HeaderMap::new()
    }

    #[test]
    fn path_matches_literal_segments() {
        assert!(path_matches("/users", "/users").is_some());
        assert!(path_matches("/users", "/users/1").is_none());
    }

    #[test]
    fn path_matches_colon_params_against_any_segment() {
        assert_eq!(path_matches("/users/:id", "/users/42"), Some(HashMap::from([("id".to_string(), "42".to_string())])));
        assert_eq!(
            path_matches("/users/:id/posts/:postId", "/users/42/posts/7"),
            Some(HashMap::from([("id".to_string(), "42".to_string()), ("postId".to_string(), "7".to_string())]))
        );
        assert!(path_matches("/users/:id", "/users/42/extra").is_none());
    }

    #[test]
    fn match_rule_respects_method_and_none_means_any() {
        let rules = vec![rule(Some("GET"), "/x", 200, "get"), rule(None, "/x", 201, "any")];
        assert_eq!(match_rule(&rules, "GET", "/x", &no_query(), &no_headers(), "").unwrap().0.body, "get");
        assert_eq!(match_rule(&rules, "POST", "/x", &no_query(), &no_headers(), "").unwrap().0.body, "any");
    }

    #[test]
    fn match_rule_returns_none_when_nothing_matches() {
        let rules = vec![rule(Some("GET"), "/x", 200, "get")];
        assert!(match_rule(&rules, "GET", "/y", &no_query(), &no_headers(), "").is_none());
    }

    #[test]
    fn match_rule_first_match_wins() {
        let rules = vec![rule(None, "/x", 200, "first"), rule(None, "/x", 201, "second")];
        assert_eq!(match_rule(&rules, "GET", "/x", &no_query(), &no_headers(), "").unwrap().0.body, "first");
    }

    #[test]
    fn match_rule_returns_path_captures() {
        let rules = vec![rule(Some("GET"), "/users/:id", 200, "")];
        let (_, captures) = match_rule(&rules, "GET", "/users/42", &no_query(), &no_headers(), "").unwrap();
        assert_eq!(captures.get("id"), Some(&"42".to_string()));
    }

    #[test]
    fn matches_query_requires_every_enabled_matcher() {
        let matchers = vec![MockMatcher { name: "env".into(), value: "prod".into(), enabled: true }];
        let mut query = HashMap::new();
        assert!(!matches_query(&matchers, &query));
        query.insert("env".into(), "prod".into());
        assert!(matches_query(&matchers, &query));
        query.insert("env".into(), "staging".into());
        assert!(!matches_query(&matchers, &query));
    }

    #[test]
    fn matches_query_ignores_disabled_matchers() {
        let matchers = vec![MockMatcher { name: "env".into(), value: "prod".into(), enabled: false }];
        assert!(matches_query(&matchers, &HashMap::new()));
    }

    /// The UI's "Add row" persists a blank `{name:"",value:"",enabled:true}`
    /// row the instant it's clicked, before the user types a name. Without
    /// the empty-name skip, this transient state would make the rule stop
    /// matching every request (`query.get("")`/`headers.get("")` is never
    /// present) until the name is filled in.
    #[test]
    fn matches_query_and_headers_ignore_empty_name_matchers() {
        let matchers = vec![MockMatcher { name: String::new(), value: String::new(), enabled: true }];
        assert!(matches_query(&matchers, &HashMap::new()));
        assert!(matches_headers(&matchers, &HeaderMap::new()));
    }

    #[test]
    fn matches_body_contains_checks_substring() {
        let matcher = Some(BodyMatcher { mode: BodyMatchMode::Contains, json_path: String::new(), value: "hello".into() });
        assert!(matches_body(&matcher, "say hello world"));
        assert!(!matches_body(&matcher, "say goodbye"));
    }

    #[test]
    fn matches_body_json_equals_checks_nested_path() {
        let matcher =
            Some(BodyMatcher { mode: BodyMatchMode::JsonEquals, json_path: "user.id".into(), value: "42".into() });
        assert!(matches_body(&matcher, r#"{"user":{"id":42}}"#));
        assert!(!matches_body(&matcher, r#"{"user":{"id":7}}"#));
        assert!(!matches_body(&matcher, "not json"));
    }

    #[test]
    fn matches_body_none_matcher_always_passes() {
        assert!(matches_body(&None, "anything"));
    }

    /// Loopback isn't "the internet" — this sandbox's cargo test can reach
    /// 127.0.0.1 even though it can't reach a real external host, so this is
    /// a genuine end-to-end proof of the whole path: real socket, real
    /// router, real rule match, real response bytes.
    #[tokio::test]
    async fn serves_a_configured_rule_over_a_real_loopback_socket() {
        let rules = vec![MockRule {
            id: "r1".into(),
            mock_server_id: "s1".into(),
            method: Some("GET".into()),
            path_pattern: "/users/:id".into(),
            status: 201,
            headers: vec![HeaderEntry { name: "X-Mock".into(), value: "yes".into(), enabled: true }],
            body: "{\"ok\":true}".into(),
            delay_ms: 0,
            sort_order: 0,
            query_matchers: Vec::new(),
            header_matchers: Vec::new(),
            body_matcher: None,
        }];
        let server = serve(build_router(rules), 0).await.unwrap();

        let resp = reqwest::get(format!("http://{}/users/42", server.addr)).await.unwrap();
        assert_eq!(resp.status(), 201);
        assert_eq!(resp.headers().get("x-mock").unwrap(), "yes");
        assert_eq!(resp.text().await.unwrap(), "{\"ok\":true}");

        server.abort();
    }

    #[tokio::test]
    async fn returns_404_when_no_rule_matches() {
        let server = serve(build_router(Vec::new()), 0).await.unwrap();
        let resp = reqwest::get(format!("http://{}/nope", server.addr)).await.unwrap();
        assert_eq!(resp.status(), 404);
        server.abort();
    }

    #[tokio::test]
    async fn only_ever_binds_loopback_not_all_interfaces() {
        let server = serve(build_router(Vec::new()), 0).await.unwrap();
        assert!(server.addr.ip().is_loopback());
        server.abort();
    }

    /// Real proof `:name` path captures reach the served response, not just
    /// that `match_rule` returns a captures map in isolation.
    #[tokio::test]
    async fn path_capture_is_interpolated_into_response_body_and_headers() {
        let rules = vec![MockRule {
            id: "r1".into(),
            mock_server_id: "s1".into(),
            method: Some("GET".into()),
            path_pattern: "/users/:id".into(),
            status: 200,
            headers: vec![HeaderEntry { name: "X-User-Id".into(), value: "{{id}}".into(), enabled: true }],
            body: "{\"id\":\"{{id}}\"}".into(),
            delay_ms: 0,
            sort_order: 0,
            query_matchers: Vec::new(),
            header_matchers: Vec::new(),
            body_matcher: None,
        }];
        let server = serve(build_router(rules), 0).await.unwrap();

        let resp = reqwest::get(format!("http://{}/users/42", server.addr)).await.unwrap();
        assert_eq!(resp.headers().get("x-user-id").unwrap(), "42");
        assert_eq!(resp.text().await.unwrap(), "{\"id\":\"42\"}");

        server.abort();
    }

    /// A real percent-encoded query value must be decoded before comparison
    /// — a naive raw-string parser would fail this against `%20`.
    #[tokio::test]
    async fn query_matcher_selects_correct_rule_with_percent_encoded_value() {
        let mut plain = rule(Some("GET"), "/search", 200, "no match");
        plain.query_matchers = vec![MockMatcher { name: "q".into(), value: "wrong".into(), enabled: true }];
        let mut matching = rule(Some("GET"), "/search", 200, "matched");
        matching.query_matchers = vec![MockMatcher { name: "q".into(), value: "hello world".into(), enabled: true }];
        let server = serve(build_router(vec![plain, matching]), 0).await.unwrap();

        let resp = reqwest::get(format!("http://{}/search?q=hello%20world", server.addr)).await.unwrap();
        assert_eq!(resp.text().await.unwrap(), "matched");

        server.abort();
    }

    #[tokio::test]
    async fn header_matcher_selects_correct_rule() {
        let mut plain = rule(Some("GET"), "/x", 200, "no match");
        plain.header_matchers = vec![MockMatcher { name: "x-env".into(), value: "prod".into(), enabled: true }];
        let mut matching = rule(Some("GET"), "/x", 200, "matched");
        matching.header_matchers = vec![MockMatcher { name: "x-env".into(), value: "staging".into(), enabled: true }];
        let server = serve(build_router(vec![plain, matching]), 0).await.unwrap();

        let client = reqwest::Client::new();
        let resp = client.get(format!("http://{}/x", server.addr)).header("x-env", "staging").send().await.unwrap();
        assert_eq!(resp.text().await.unwrap(), "matched");

        server.abort();
    }

    /// The actual disambiguation use case: two rules sharing method+path,
    /// selected by request body content — proves first-match-wins doesn't
    /// silently defeat the body matcher.
    #[tokio::test]
    async fn body_matcher_disambiguates_rules_sharing_method_and_path() {
        let mut created = rule(Some("POST"), "/orders", 201, "created");
        created.body_matcher =
            Some(BodyMatcher { mode: BodyMatchMode::JsonEquals, json_path: "action".into(), value: "create".into() });
        let mut cancelled = rule(Some("POST"), "/orders", 200, "cancelled");
        cancelled.body_matcher =
            Some(BodyMatcher { mode: BodyMatchMode::JsonEquals, json_path: "action".into(), value: "cancel".into() });
        let server = serve(build_router(vec![created, cancelled]), 0).await.unwrap();

        let client = reqwest::Client::new();
        let resp = client
            .post(format!("http://{}/orders", server.addr))
            .body(r#"{"action":"cancel"}"#)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        assert_eq!(resp.text().await.unwrap(), "cancelled");

        server.abort();
    }
}
