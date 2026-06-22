//! Collection (and folder) nodes. A folder is just a collection with a parent.

use crate::model::auth::AuthConfig;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Collection {
    pub id: String,
    pub workspace_id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub description: Option<String>,
    /// Default auth for requests in this collection. Resolution does not
    /// walk up nested parent collections — only a request's own direct
    /// collection is consulted, mirroring `VarScope::Collection`.
    #[serde(default)]
    pub auth: AuthConfig,
    pub sort_order: i64,
    pub created_at: i64,
    pub updated_at: i64,
}
