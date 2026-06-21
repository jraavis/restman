//! Auto-saved request/response snapshots.

use super::http::{HttpRequest, HttpResponse};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub id: String,
    pub workspace_id: String,
    pub request_id: Option<String>,
    pub name: String,
    pub method: String,
    pub url: String,
    pub status: Option<u16>,
    pub duration_ms: Option<f64>,
    pub request: HttpRequest,
    pub response: Option<HttpResponse>,
    pub error: Option<String>,
    pub created_at: i64,
}

/// Filters accepted by `list` — every field optional/empty means "no filter".
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryFilter {
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub status_min: Option<u16>,
    #[serde(default)]
    pub status_max: Option<u16>,
    #[serde(default)]
    pub date_min: Option<i64>,
    #[serde(default)]
    pub date_max: Option<i64>,
    #[serde(default)]
    pub limit: Option<i64>,
}
