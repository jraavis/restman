//! Color-coded tags attached to requests.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tag {
    pub id: String,
    pub workspace_id: String,
    pub name: String,
    pub color: String,
}
