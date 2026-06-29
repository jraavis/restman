//! User-authored JS plugins: custom code-generators and custom import/export
//! formats, sandbox-executed rather than compiled into the Rust binary.
//! Storage only — execution lives in a separate sandbox module.
//!
//! Not yet referenced outside `store::plugins` and its tests: the
//! `commands::plugins` layer that exposes these over IPC is a later
//! sequential task. Suppress dead-code warnings until that lands.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};

/// What a plugin does. `Codegen` plugins render a saved request as source
/// code in some language; `Import`/`Export` plugins translate collections to
/// and from some external format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginKind {
    Codegen,
    Import,
    Export,
}

/// A stored JS plugin, scoped to one workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plugin {
    pub id: String,
    pub workspace_id: String,
    pub name: String,
    pub kind: PluginKind,
    /// Display label: the language name for `Codegen`, the format name for
    /// `Import`/`Export` (e.g. "Python (requests)", "Insomnia v4").
    pub language_label: String,
    pub source: String,
    pub enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Fields accepted when creating or updating a plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginInput {
    pub name: String,
    pub kind: PluginKind,
    pub language_label: String,
    pub source: String,
    #[serde(default = "super::http::default_true")]
    pub enabled: bool,
}
