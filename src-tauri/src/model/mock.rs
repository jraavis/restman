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

/// One method+path -> canned-response rule. `method: None` matches any
/// method. `path_pattern` supports `:name` segments matching any single path
/// segment (e.g. `/users/:id` matches `/users/42`, not `/users/42/posts`).
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
}
