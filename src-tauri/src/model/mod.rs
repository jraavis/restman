//! Shared data structures serialized across the IPC boundary.
//! Field names are camelCased to match idiomatic TypeScript on the frontend.

pub mod auth;
pub mod collection;
pub mod environment;
pub mod grpc;
pub mod history;
pub mod http;
pub mod mock;
pub mod plugin;
pub mod request;
pub mod streaming;
pub mod tab;
pub mod tag;
pub mod variable;
pub mod workspace_settings;

use serde::{Deserialize, Serialize};

pub use auth::AuthConfig;
pub use collection::Collection;
pub use environment::Environment;
pub use history::{HistoryEntry, HistoryFilter};
pub use mock::{BodyMatchMode, BodyMatcher, MockMatcher, MockRule, MockRuleInput, MockServer, MockServerInput};
pub use plugin::{Plugin, PluginInput, PluginKind};
pub use request::{RequestKind, SavedRequest, SavedRequestInput};
pub use tab::Tab;
pub use tag::Tag;
pub use variable::{VarScope, VarType, Variable, VariableInput, SECRET_MASK};
pub use workspace_settings::{ClientCertConfig, SyncFormat, SyncMode, WorkspaceSettings};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub is_active: bool,
}
