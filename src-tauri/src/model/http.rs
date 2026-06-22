//! HTTP request/response types crossing the IPC boundary.

use super::auth::AuthConfig;
use serde::{Deserialize, Serialize};

pub(crate) fn default_true() -> bool {
    true
}
fn default_timeout() -> u64 {
    30
}
fn default_max_redirects() -> usize {
    10
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeaderEntry {
    pub name: String,
    pub value: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyValue {
    pub key: String,
    pub value: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormField {
    pub key: String,
    /// For text fields, the value. For file fields, the absolute file path.
    pub value: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub is_file: bool,
    #[serde(default)]
    pub content_type: Option<String>,
}

/// Request body, tagged by `mode` with payload under `data`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(tag = "mode", content = "data", rename_all = "camelCase")]
pub enum RequestBody {
    #[default]
    None,
    Json(String),
    Raw {
        content: String,
        #[serde(default)]
        language: Option<String>,
    },
    UrlEncoded(Vec<KeyValue>),
    FormData(Vec<FormField>),
    Binary {
        path: String,
    },
    Graphql {
        query: String,
        #[serde(default)]
        variables: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestOptions {
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    #[serde(default = "default_true")]
    pub follow_redirects: bool,
    #[serde(default = "default_true")]
    pub verify_ssl: bool,
    #[serde(default = "default_max_redirects")]
    pub max_redirects: usize,
    /// When true, the engine uses a shared cookie jar: Set-Cookie responses
    /// are stored and Cookie headers are replayed on subsequent sends.
    #[serde(default)]
    pub send_cookies: bool,
}

impl Default for RequestOptions {
    fn default() -> Self {
        Self {
            timeout_secs: default_timeout(),
            follow_redirects: true,
            verify_ssl: true,
            max_redirects: default_max_redirects(),
            send_cookies: false,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub headers: Vec<HeaderEntry>,
    #[serde(default)]
    pub query: Vec<KeyValue>,
    #[serde(default)]
    pub body: RequestBody,
    #[serde(default)]
    pub options: RequestOptions,
    /// Already resolved — no `Inherit` at this level. Set by
    /// `crate::auth::resolve` + `hydrate` right before the request is sent,
    /// never read back out over IPC.
    #[serde(default)]
    pub auth: AuthConfig,
}

/// Per-phase timing. Fields are optional: the current engine fills
/// total/ttfb/download; DNS/connect/TLS require an instrumented connector
/// (tracked Phase-1 follow-up) and are `None` until then.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Timing {
    pub total_ms: f64,
    pub dns_ms: Option<f64>,
    pub connect_ms: Option<f64>,
    pub tls_ms: Option<f64>,
    pub ttfb_ms: Option<f64>,
    pub download_ms: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpResponse {
    pub status: u16,
    pub status_text: String,
    pub headers: Vec<HeaderEntry>,
    /// Raw response bytes, base64-encoded (safe for binary; frontend decodes).
    pub body_base64: String,
    pub size_bytes: u64,
    pub timing: Timing,
    pub final_url: String,
    pub http_version: String,
}
