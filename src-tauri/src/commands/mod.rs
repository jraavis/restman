//! Tauri IPC command handlers. Thin wrappers over the store/engine layers,
//! grouped one module per domain and re-exported flat for `generate_handler!`.

pub mod collections;
pub mod environments;
pub mod history;
pub mod http;
pub mod oauth;
pub mod requests;
pub mod tabs;
pub mod tags;
pub mod variables;
pub mod workspaces;

pub use collections::*;
pub use environments::*;
pub use history::*;
pub use http::*;
pub use oauth::*;
pub use requests::*;
pub use tabs::*;
pub use tags::*;
pub use variables::*;
pub use workspaces::*;
