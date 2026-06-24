//! Tauri IPC command handlers. Thin wrappers over the store/engine layers,
//! grouped one module per domain and re-exported flat for `generate_handler!`.

pub mod codegen;
pub mod collections;
pub mod environments;
pub mod files;
pub mod history;
pub mod http;
pub mod interop;
pub mod oauth;
pub mod requests;
pub mod scripting;
pub mod tabs;
pub mod tags;
pub mod variables;
pub mod workspaces;

pub use codegen::*;
pub use collections::*;
pub use environments::*;
pub use files::*;
pub use history::*;
pub use http::*;
pub use interop::*;
pub use oauth::*;
pub use requests::*;
pub use scripting::*;
pub use tabs::*;
pub use tags::*;
pub use variables::*;
pub use workspaces::*;
