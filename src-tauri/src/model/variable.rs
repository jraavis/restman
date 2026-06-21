//! Variables, scoped to exactly one of global / workspace / collection /
//! environment. Resolution priority across scopes lives in `crate::vars`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VarType {
    String,
    Number,
    Boolean,
    Json,
}

impl VarType {
    pub fn as_str(self) -> &'static str {
        match self {
            VarType::String => "string",
            VarType::Number => "number",
            VarType::Boolean => "boolean",
            VarType::Json => "json",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "number" => VarType::Number,
            "boolean" => VarType::Boolean,
            "json" => VarType::Json,
            _ => VarType::String,
        }
    }
}

/// Which single scope a variable belongs to. Carries the parent id so callers
/// don't need a separate enum-plus-id pair. Tagged the same way as
/// `RequestBody` (`{"kind":"workspace","id":"..."}`) for a consistent IPC shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "camelCase")]
pub enum VarScope {
    Global,
    Workspace(String),
    Collection(String),
    Environment(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Variable {
    pub id: String,
    pub scope: VarScope,
    pub key: String,
    pub value: String,
    pub var_type: VarType,
    pub is_secret: bool,
    pub enabled: bool,
    pub sort_order: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

impl Variable {
    /// Blank a secret's value before it crosses the IPC boundary. Interpolation
    /// happens entirely in Rust (`crate::vars::resolve`), so the frontend never
    /// needs the real value — only the store layer should ever see plaintext.
    pub fn mask_secret(mut self) -> Self {
        if self.is_secret {
            self.value = SECRET_MASK.to_string();
        }
        self
    }
}

pub const SECRET_MASK: &str = "••••••••";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VariableInput {
    pub key: String,
    pub value: String,
    #[serde(default = "default_var_type")]
    pub var_type: VarType,
    #[serde(default)]
    pub is_secret: bool,
    #[serde(default = "super::http::default_true")]
    pub enabled: bool,
}

fn default_var_type() -> VarType {
    VarType::String
}
