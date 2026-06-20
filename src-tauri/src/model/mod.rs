//! Shared data structures serialized across the IPC boundary.
//! Field names are camelCased to match idiomatic TypeScript on the frontend.

pub mod http;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub is_active: bool,
}
