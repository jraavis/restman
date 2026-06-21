//! Shared data structures serialized across the IPC boundary.
//! Field names are camelCased to match idiomatic TypeScript on the frontend.

pub mod collection;
pub mod environment;
pub mod history;
pub mod http;
pub mod request;
pub mod tab;
pub mod tag;
pub mod variable;

use serde::{Deserialize, Serialize};

pub use collection::Collection;
pub use environment::Environment;
pub use history::{HistoryEntry, HistoryFilter};
pub use request::{SavedRequest, SavedRequestInput};
pub use tab::Tab;
pub use tag::Tag;
pub use variable::{VarScope, VarType, Variable, VariableInput, SECRET_MASK};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub is_active: bool,
}
