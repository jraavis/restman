//! Plugin execution subsystem — sandboxed QuickJS harness for calling a
//! named top-level function defined by user-authored plugin source.
//!
//! Unlike `scripting::engine` (which runs *side-effecting* pre/post-request
//! scripts that mutate shared state via an injected `pm` object), a plugin
//! is a **pure function call**: the plugin source defines a top-level JS
//! function, this harness evaluates the source, looks up the named
//! function, calls it with JSON-serializable arguments, and marshals the
//! return value back into Rust. Same sandbox guarantees as the scripting
//! engine (fresh `Runtime`+`Context` per call, no fs/network access, 8s
//! timeout, 512KB max stack) — see `runtime::apply_runtime_limits` reuse.

pub mod runtime;

pub use runtime::{call_returning_json, call_returning_string};
