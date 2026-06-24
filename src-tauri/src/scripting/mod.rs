//! Scripting subsystem — QuickJS sandbox with pm.* API.
//!
//! The public surface is two functions:
//! - `run_pre_script` — runs before a request is sent; can mutate env vars,
//!   abort the send.
//! - `run_post_script` — runs after the response arrives; same powers plus
//!   read access to the response.
//!
//! All JS execution is synchronous and single-threaded per call (each
//! invocation creates its own Runtime + Context).  Scripts have no access
//! to the host filesystem, network, or OS keychain.

pub mod engine;
pub mod types;

pub use engine::{run_post_script, run_pre_script};
pub use types::{PostScriptContext, PreScriptContext, ScriptResult, TestResult};
