//! Per-workspace HTTP transport settings: outbound proxy, default headers
//! applied to every request in the workspace, and an optional mTLS client
//! certificate. Stored as a separate row from `Workspace` so workspace-row
//! mutations (rename, active toggle) never touch transport config and vice
//! versa — same separation rationale as `oauth_tokens` vs `auth_json`.
//!
//! Secrets (pasted PEM cert/key bytes, and a passphrase) never touch the DB
//! column in cleartext: only the keychain slot names round-trip through the
//! settings JSON, and the real bytes are hydrated from `crate::secrets` at
//! send time. Path mode stores only filesystem paths (the cert/key live on
//! disk, not in the app's keychain), which is what Bruno/Postman do — the
//! paths themselves cross IPC but the bytes never do.

use serde::{Deserialize, Serialize};
use crate::model::http::HeaderEntry;

/// mTLS client certificate. Two storage modes — `Paste` keeps the PEM bytes
/// in the OS keychain; `Path` references on-disk files. Both may carry an
/// optional passphrase for encrypted PEM keys.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", content = "data", rename_all = "camelCase")]
pub enum ClientCertConfig {
    /// No client certificate configured.
    None,
    /// PEM cert + key bytes pasted into the UI; real bytes live in the
    /// keychain under `wscert:{workspace_id}:{cert|key|pass}`. The fields on
    /// this variant are masked/empty whenever they cross the IPC boundary for
    /// display — hydrated only at send time on the Rust side.
    Paste {
        cert_pem: String,
        key_pem: String,
        #[serde(default)]
        passphrase: Option<String>,
    },
    /// PEM cert + key at filesystem paths; the app reads them at send time and
    /// never copies the bytes into its own storage.
    Path {
        cert_path: String,
        key_path: String,
        #[serde(default)]
        passphrase: Option<String>,
    },
}

impl Default for ClientCertConfig {
    fn default() -> Self { Self::None }
}

impl ClientCertConfig {
    pub fn is_set(&self) -> bool {
        !matches!(self, Self::None)
    }
}

/// How a workspace's `.restman/` folder relates to the DB — see
/// `crate::sync` module doc for what each mode actually does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SyncMode {
    /// No sync folder configured; `sync_folder_path` is ignored even if set.
    #[default]
    Off,
    /// The user triggers export/import explicitly (buttons in the UI).
    Manual,
    /// Same as `Manual`, plus the app automatically re-exports to
    /// `sync_folder_path` after every collection/environment mutation.
    /// Import is still always manual — see the migration's module doc.
    Live,
}

impl SyncMode {
    pub fn as_str(self) -> &'static str {
        match self {
            SyncMode::Off => "off",
            SyncMode::Manual => "manual",
            SyncMode::Live => "live",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "manual" => SyncMode::Manual,
            "live" => SyncMode::Live,
            _ => SyncMode::Off,
        }
    }
}

/// File format `crate::sync` writes collections/environments in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SyncFormat {
    #[default]
    Json,
    /// Git-friendly YAML — same content, easier to diff/review in a PR.
    Yaml,
}

impl SyncFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            SyncFormat::Json => "json",
            SyncFormat::Yaml => "yaml",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "yaml" => SyncFormat::Yaml,
            _ => SyncFormat::Json,
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            SyncFormat::Json => "json",
            SyncFormat::Yaml => "yaml",
        }
    }
}

/// One workspace's transport settings. `default_headers` are plain strings
/// (no secret treatment) — they're applied to every request unless the
/// request already carries a same-named header (user value wins).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSettings {
    pub workspace_id: String,
    #[serde(default)]
    pub proxy_url: Option<String>,
    /// Comma-separated host list (Postman-style) bypassed by the proxy.
    #[serde(default)]
    pub proxy_bypass: Option<String>,
    #[serde(default)]
    pub default_headers: Vec<HeaderEntry>,
    #[serde(default)]
    pub client_cert: ClientCertConfig,
    #[serde(default)]
    pub sync_folder_path: Option<String>,
    #[serde(default)]
    pub sync_mode: SyncMode,
    #[serde(default)]
    pub sync_format: SyncFormat,
}

impl WorkspaceSettings {
    pub fn empty(workspace_id: &str) -> Self {
        Self {
            workspace_id: workspace_id.to_string(),
            proxy_url: None,
            proxy_bypass: None,
            default_headers: Vec::new(),
            client_cert: ClientCertConfig::None,
            sync_folder_path: None,
            sync_mode: SyncMode::Off,
            sync_format: SyncFormat::Json,
        }
    }
}