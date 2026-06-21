//! Environments group variables; scoped to a workspace, optionally narrowed
//! to one collection within it. Exactly one environment can be active per
//! workspace at a time (mirrors `Workspace::is_active`).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Environment {
    pub id: String,
    pub workspace_id: String,
    pub collection_id: Option<String>,
    pub name: String,
    pub group_name: Option<String>,
    pub is_active: bool,
    pub created_at: i64,
    pub updated_at: i64,
}
