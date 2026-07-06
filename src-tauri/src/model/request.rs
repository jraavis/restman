//! A saved request: a named, persisted `HttpRequest` living in a collection.

use super::auth::RequestAuth;
use super::http::{HeaderEntry, KeyValue, RequestBody, RequestOptions};
use serde::{Deserialize, Serialize};

/// Discriminates what a saved request actually is. `Http` is the original,
/// tab-backed shape (`headers`/`query`/`body`/`options` etc). The streaming
/// kinds instead carry their protocol-specific connect config in
/// `SavedRequest::stream_config`/`SavedRequestInput::stream_config` — an
/// opaque JSON blob the frontend owns the shape of, since SSE/WS/gRPC each
/// need different fields (see `streaming::SsePanel`/`WsPanel`/`GrpcPanel`).
/// The HTTP-shaped fields on a streaming-kind row are unused placeholders
/// (kept because the columns are `NOT NULL`), not a second source of truth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestKind {
    #[default]
    Http,
    Sse,
    Ws,
    Grpc,
}

impl RequestKind {
    pub fn as_db_str(self) -> &'static str {
        match self {
            RequestKind::Http => "http",
            RequestKind::Sse => "sse",
            RequestKind::Ws => "ws",
            RequestKind::Grpc => "grpc",
        }
    }

    pub fn from_db_str(s: &str) -> Self {
        match s {
            "sse" => RequestKind::Sse,
            "ws" => RequestKind::Ws,
            "grpc" => RequestKind::Grpc,
            _ => RequestKind::Http,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedRequest {
    pub id: String,
    pub collection_id: String,
    pub name: String,
    pub method: String,
    pub url: String,
    pub headers: Vec<HeaderEntry>,
    pub query: Vec<KeyValue>,
    pub body: RequestBody,
    pub options: RequestOptions,
    #[serde(default)]
    pub auth: RequestAuth,
    pub tags: Vec<super::tag::Tag>,
    /// JavaScript run before the request is sent. Empty string = no script.
    #[serde(default)]
    pub pre_request_script: String,
    /// JavaScript run after the response arrives. Empty string = no script.
    #[serde(default)]
    pub post_response_script: String,
    #[serde(default)]
    pub kind: RequestKind,
    /// Opaque per-kind connect config for streaming kinds; `None` for `Http`.
    #[serde(default)]
    pub stream_config: Option<serde_json::Value>,
    pub sort_order: i64,
    pub created_at: i64,
    pub updated_at: i64,
    pub last_used_at: Option<i64>,
}

/// Fields accepted when creating or updating a saved request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedRequestInput {
    pub name: String,
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
    #[serde(default)]
    pub auth: RequestAuth,
    #[serde(default)]
    pub pre_request_script: String,
    #[serde(default)]
    pub post_response_script: String,
    #[serde(default)]
    pub kind: RequestKind,
    #[serde(default)]
    pub stream_config: Option<serde_json::Value>,
}
