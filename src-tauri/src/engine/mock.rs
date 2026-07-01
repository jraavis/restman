//! Local mock-server hosting: binds a configured `MockRule` list to a real
//! loopback socket and answers matching requests with the configured canned
//! response. `serve()`/`RunningMockServer` are re-homed here from the 17c-
//! style feasibility spike (see the previous commit) now that the
//! rule-matching `Router` construction sits alongside them.

use crate::model::MockRule;
use axum::extract::State;
use axum::http::{Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use axum::Router;
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

/// True if the pattern's segments align with the path's — `:name` segments
/// match any single path segment, everything else must match literally.
/// Segment counts must match exactly (no wildcard-tail matching).
fn path_matches(pattern: &str, path: &str) -> bool {
    let pattern_segs: Vec<&str> = pattern.trim_matches('/').split('/').collect();
    let path_segs: Vec<&str> = path.trim_matches('/').split('/').collect();
    if pattern_segs.len() != path_segs.len() {
        return false;
    }
    pattern_segs.iter().zip(path_segs.iter()).all(|(p, s)| p.starts_with(':') || p == s)
}

/// First rule (in the caller's ordering — callers pass rules pre-sorted by
/// `sort_order`) whose method (or `None`, meaning any) and path pattern both
/// match. Pure and independently testable without a server.
pub fn match_rule<'a>(rules: &'a [MockRule], method: &str, path: &str) -> Option<&'a MockRule> {
    rules.iter().find(|r| {
        let method_matches = r.method.as_deref().is_none_or(|m| m.eq_ignore_ascii_case(method));
        method_matches && path_matches(&r.path_pattern, path)
    })
}

async fn handle_any(State(rules): State<Arc<Vec<MockRule>>>, method: Method, uri: Uri) -> Response {
    match match_rule(&rules, method.as_str(), uri.path()) {
        Some(rule) => {
            if rule.delay_ms > 0 {
                tokio::time::sleep(Duration::from_millis(rule.delay_ms)).await;
            }
            let mut builder = Response::builder().status(rule.status);
            for h in rule.headers.iter().filter(|h| h.enabled) {
                builder = builder.header(&h.name, &h.value);
            }
            builder.body(axum::body::Body::from(rule.body.clone())).unwrap_or_else(|_| {
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
        }
    }

    #[test]
    fn path_matches_literal_segments() {
        assert!(path_matches("/users", "/users"));
        assert!(!path_matches("/users", "/users/1"));
    }

    #[test]
    fn path_matches_colon_params_against_any_segment() {
        assert!(path_matches("/users/:id", "/users/42"));
        assert!(path_matches("/users/:id/posts/:postId", "/users/42/posts/7"));
        assert!(!path_matches("/users/:id", "/users/42/extra"));
    }

    #[test]
    fn match_rule_respects_method_and_none_means_any() {
        let rules = vec![rule(Some("GET"), "/x", 200, "get"), rule(None, "/x", 201, "any")];
        assert_eq!(match_rule(&rules, "GET", "/x").unwrap().body, "get");
        assert_eq!(match_rule(&rules, "POST", "/x").unwrap().body, "any");
    }

    #[test]
    fn match_rule_returns_none_when_nothing_matches() {
        let rules = vec![rule(Some("GET"), "/x", 200, "get")];
        assert!(match_rule(&rules, "GET", "/y").is_none());
    }

    #[test]
    fn match_rule_first_match_wins() {
        let rules = vec![rule(None, "/x", 200, "first"), rule(None, "/x", 201, "second")];
        assert_eq!(match_rule(&rules, "GET", "/x").unwrap().body, "first");
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
}
