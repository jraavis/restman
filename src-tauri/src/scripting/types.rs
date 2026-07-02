//! Shared scripting types that cross the IPC boundary.

use serde::{Deserialize, Serialize};

/// Outcome of a single `pm.test(name, fn)` call.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestResult {
    /// The label passed to `pm.test(name, ...)`.
    pub name: String,
    pub passed: bool,
    /// Assertion failure message when `passed` is false.
    pub error: Option<String>,
}

/// Aggregate result returned from running a pre- or post-request script.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ScriptResult {
    /// All `pm.test` calls in execution order.
    pub tests: Vec<TestResult>,
    /// Any uncaught JS exception or runtime error (not a pm.test failure).
    pub error: Option<String>,
    /// Variables that the script set via `pm.environment.set` (net effect).
    pub env_mutations: Vec<(String, String)>,
    /// Keys removed via `pm.environment.unset` (net effect — not re-set later).
    pub env_unsets: Vec<String>,
    /// True if `pm.abort()` was called — the send should be cancelled.
    pub aborted: bool,
}

impl ScriptResult {
    pub fn passed(&self) -> usize {
        self.tests.iter().filter(|t| t.passed).count()
    }

    pub fn failed(&self) -> usize {
        self.tests.iter().filter(|t| !t.passed).count()
    }
}

/// The data exposed to a pre-request script (read-only view of the request,
/// mutable env).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreScriptContext {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub query: Vec<(String, String)>,
    /// Resolved env vars available for reading.
    pub env: std::collections::HashMap<String, String>,
}

/// The data exposed to a post-response script (full response + env).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PostScriptContext {
    pub method: String,
    pub url: String,
    pub request_headers: Vec<(String, String)>,
    pub status: u16,
    pub status_text: String,
    pub response_headers: Vec<(String, String)>,
    /// Raw response body as UTF-8 string (best-effort; binary responses may
    /// have replacement characters).
    pub body: String,
    pub duration_ms: f64,
    pub env: std::collections::HashMap<String, String>,
}
