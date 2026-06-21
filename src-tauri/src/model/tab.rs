//! Open editor tabs. `draft` holds the live (possibly unsaved) request body
//! so edits survive an app restart even before the user explicitly saves.

use super::http::HttpRequest;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tab {
    pub id: String,
    pub workspace_id: String,
    pub request_id: Option<String>,
    pub title: String,
    pub draft: HttpRequest,
    pub sort_order: i64,
    pub is_active: bool,
    pub created_at: i64,
    pub updated_at: i64,
}
