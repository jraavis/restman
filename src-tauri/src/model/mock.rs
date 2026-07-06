//! Local mock servers: workspace-scoped configs (a server + its ordered
//! rules), route stand-ins served by `engine::mock` while running. Storage/
//! config only — which configs are actually running right now lives in
//! `AppState.mock_servers`, not here.

use super::http::HeaderEntry;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MockServer {
    pub id: String,
    pub workspace_id: String,
    pub name: String,
    pub port: u16,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MockServerInput {
    pub name: String,
    pub port: u16,
}

/// A request-matching constraint checked against an incoming query
/// parameter or header (by `name`), in addition to the rule's method+path.
/// Disabled matchers are skipped, same enable/disable convention as
/// `HeaderEntry`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MockMatcher {
    pub name: String,
    pub value: String,
    #[serde(default = "super::http::default_true")]
    pub enabled: bool,
}

/// How `BodyMatcher::value` is checked against the incoming request body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BodyMatchMode {
    /// The raw body text contains `value` as a substring.
    Contains,
    /// The body parses as JSON and the value at `json_path` (dot-separated,
    /// e.g. `user.id`) stringifies to exactly `value`.
    JsonEquals,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BodyMatcher {
    pub mode: BodyMatchMode,
    #[serde(default)]
    pub json_path: String,
    pub value: String,
}

/// One method+path -> canned-response rule. `method: None` matches any
/// method. `path_pattern` supports `:name` segments matching any single path
/// segment (e.g. `/users/:id` matches `/users/42`, not `/users/42/posts`) —
/// the captured segment values are available for response templating (see
/// `engine::mock`) as `{{name}}` in `body`/`headers`. `query_matchers`/
/// `header_matchers`/`body_matcher` are additional constraints on top of
/// method+path, letting two rules share the same path and be disambiguated
/// by request content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MockRule {
    pub id: String,
    pub mock_server_id: String,
    #[serde(default)]
    pub method: Option<String>,
    pub path_pattern: String,
    pub status: u16,
    #[serde(default)]
    pub headers: Vec<HeaderEntry>,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub delay_ms: u64,
    #[serde(default)]
    pub sort_order: i64,
    #[serde(default)]
    pub query_matchers: Vec<MockMatcher>,
    #[serde(default)]
    pub header_matchers: Vec<MockMatcher>,
    #[serde(default)]
    pub body_matcher: Option<BodyMatcher>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MockRuleInput {
    #[serde(default)]
    pub method: Option<String>,
    pub path_pattern: String,
    pub status: u16,
    #[serde(default)]
    pub headers: Vec<HeaderEntry>,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub delay_ms: u64,
    #[serde(default)]
    pub sort_order: i64,
    #[serde(default)]
    pub query_matchers: Vec<MockMatcher>,
    #[serde(default)]
    pub header_matchers: Vec<MockMatcher>,
    #[serde(default)]
    pub body_matcher: Option<BodyMatcher>,
}
