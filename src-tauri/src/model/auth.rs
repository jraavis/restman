//! Auth config shared by collections, saved requests, and the live
//! `HttpRequest` sent to the engine.
//!
//! Secret-bearing fields (`token`, `password`, `value`, `client_secret`,
//! `refresh_token`, `secret_key`, `session_token`) follow the same
//! mask-on-write contract as `Variable`/`SECRET_MASK`: the JSON stored in
//! `collections.auth_json` / `requests.auth_json` never holds a real secret,
//! only `SECRET_MASK` (a secret is set, lives in the keychain) or `""` (no
//! secret set). The real value is recovered from the keychain only in
//! memory, right before a request is sent — see `crate::auth::hydrate`.

use crate::model::variable::SECRET_MASK;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiKeyLocation {
    Header,
    Query,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OAuth2GrantType {
    AuthorizationCode,
    ClientCredentials,
    Password,
    RefreshToken,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PkceMethod {
    #[default]
    S256,
    Plain,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuth2Config {
    pub grant_type: OAuth2GrantType,
    #[serde(default)]
    pub auth_url: String,
    #[serde(default)]
    pub token_url: String,
    #[serde(default)]
    pub client_id: String,
    #[serde(default)]
    pub client_secret: String,
    #[serde(default)]
    pub scope: String,
    #[serde(default)]
    pub redirect_uri: String,
    #[serde(default)]
    pub pkce: PkceMethod,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    /// Manually-seeded refresh token for the `RefreshToken` grant. For the
    /// `AuthorizationCode` grant, the refresh token obtained from the
    /// browser flow lives in the token store (`crate::auth::token_store`),
    /// not here.
    #[serde(default)]
    pub refresh_token: String,
}

impl Default for OAuth2GrantType {
    fn default() -> Self {
        OAuth2GrantType::ClientCredentials
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AwsSigV4Config {
    #[serde(default)]
    pub access_key: String,
    #[serde(default)]
    pub secret_key: String,
    #[serde(default)]
    pub region: String,
    #[serde(default)]
    pub service: String,
    #[serde(default)]
    pub session_token: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuthConfig {
    None,
    Bearer {
        #[serde(default)]
        token: String,
        /// Authorization scheme word before the token (`"Bearer"` unless the
        /// API wants e.g. `"Token"` or `"JWT"`; empty sends the bare token).
        #[serde(default = "default_bearer_prefix")]
        prefix: String,
    },
    Basic {
        #[serde(default)]
        username: String,
        #[serde(default)]
        password: String,
    },
    ApiKey {
        #[serde(default)]
        key: String,
        #[serde(default)]
        value: String,
        location: ApiKeyLocation,
    },
    OAuth2(OAuth2Config),
    AwsSigV4(AwsSigV4Config),
}

impl Default for AuthConfig {
    fn default() -> Self {
        AuthConfig::None
    }
}

pub fn default_bearer_prefix() -> String {
    "Bearer".into()
}

/// `"{prefix} {token}"`, or the bare token when the prefix is empty — the
/// one place the Authorization value shape for bearer auth is defined, shared
/// by the engine, codegen, and the curl exporter.
pub fn bearer_header_value(prefix: &str, token: &str) -> String {
    if prefix.is_empty() {
        token.to_string()
    } else {
        format!("{prefix} {token}")
    }
}

impl AuthConfig {
    /// Slot names this config variant stores secrets under. Stable across
    /// edits so `crate::auth::persist`/`hydrate` can address the keychain.
    /// Returning the *full known set* (not just this variant's) lets callers
    /// sweep stale entries left behind by a since-changed auth type.
    pub fn secret_slots() -> &'static [&'static str] {
        &[
            "bearer-token",
            "basic-password",
            "apikey-value",
            "oauth-client-secret",
            "oauth-password",
            "oauth-refresh-token",
            "aws-secret-key",
            "aws-session-token",
        ]
    }

    /// `(slot, value)` pairs for the secret fields this variant actually
    /// has, in storage order. Empty list for variants with no secrets.
    pub fn secret_fields(&self) -> Vec<(&'static str, &str)> {
        match self {
            AuthConfig::None => vec![],
            AuthConfig::Bearer { token, .. } => vec![("bearer-token", token.as_str())],
            AuthConfig::Basic { password, .. } => vec![("basic-password", password.as_str())],
            AuthConfig::ApiKey { value, .. } => vec![("apikey-value", value.as_str())],
            AuthConfig::OAuth2(c) => vec![
                ("oauth-client-secret", c.client_secret.as_str()),
                ("oauth-password", c.password.as_str()),
                ("oauth-refresh-token", c.refresh_token.as_str()),
            ],
            AuthConfig::AwsSigV4(c) => vec![
                ("aws-secret-key", c.secret_key.as_str()),
                ("aws-session-token", c.session_token.as_str()),
            ],
        }
    }

    /// Replace this variant's secret fields in place, by slot name.
    fn set_secret_field(&mut self, slot: &str, value: String) {
        match self {
            AuthConfig::Bearer { token, .. } if slot == "bearer-token" => *token = value,
            AuthConfig::Basic { password, .. } if slot == "basic-password" => *password = value,
            AuthConfig::ApiKey { value: v, .. } if slot == "apikey-value" => *v = value,
            AuthConfig::OAuth2(c) => match slot {
                "oauth-client-secret" => c.client_secret = value,
                "oauth-password" => c.password = value,
                "oauth-refresh-token" => c.refresh_token = value,
                _ => {}
            },
            AuthConfig::AwsSigV4(c) => match slot {
                "aws-secret-key" => c.secret_key = value,
                "aws-session-token" => c.session_token = value,
                _ => {}
            },
            _ => {}
        }
    }

    pub(crate) fn with_secret_field(mut self, slot: &str, value: String) -> Self {
        self.set_secret_field(slot, value);
        self
    }

    /// True if every secret field is either empty or already masked — i.e.
    /// safe to cross IPC. Used by tests/debug assertions, not on the hot path.
    pub fn is_masked(&self) -> bool {
        self.secret_fields().iter().all(|(_, v)| v.is_empty() || *v == SECRET_MASK)
    }
}

/// Request-level auth: either fall through to the owning collection's
/// `AuthConfig`, or override it. Collections don't get this wrapper — a
/// collection's auth is always a plain `AuthConfig` (default `None`),
/// mirroring `VarScope::Collection` as a terminal, non-chaining scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum RequestAuth {
    Inherit,
    Own(AuthConfig),
}

impl Default for RequestAuth {
    fn default() -> Self {
        RequestAuth::Inherit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_request_auth_is_inherit() {
        assert_eq!(RequestAuth::default(), RequestAuth::Inherit);
    }

    #[test]
    fn bearer_round_trips_through_json() {
        let cfg = AuthConfig::Bearer { token: "abc".into(), prefix: crate::model::auth::default_bearer_prefix() };
        let json = serde_json::to_string(&cfg).unwrap();
        assert_eq!(json, r#"{"type":"bearer","token":"abc","prefix":"Bearer"}"#);
        let back: AuthConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cfg);
    }

    /// Bearer configs stored before the `prefix` field existed deserialize
    /// with the `"Bearer"` default rather than failing or going empty.
    #[test]
    fn bearer_without_prefix_defaults_on_deserialize() {
        let back: AuthConfig = serde_json::from_str(r#"{"type":"bearer","token":"abc"}"#).unwrap();
        assert_eq!(back, AuthConfig::Bearer { token: "abc".into(), prefix: "Bearer".into() });
    }

    #[test]
    fn bearer_header_value_handles_custom_and_empty_prefix() {
        assert_eq!(bearer_header_value("Bearer", "tok"), "Bearer tok");
        assert_eq!(bearer_header_value("Token", "tok"), "Token tok");
        assert_eq!(bearer_header_value("", "tok"), "tok");
    }

    /// Pins down the exact `"type"` tag string serde's snake_case rule
    /// produces for each variant — these are not obvious from the variant
    /// names alone (`OAuth2` → `o_auth2`, `AwsSigV4` → `aws_sig_v4`) and the
    /// frontend's TypeScript mirror must match them exactly.
    #[test]
    fn variant_tags_match_frontend_contract() {
        assert_eq!(serde_json::to_string(&AuthConfig::None).unwrap(), r#"{"type":"none"}"#);
        assert_eq!(
            serde_json::to_string(&AuthConfig::ApiKey { key: "k".into(), value: "v".into(), location: ApiKeyLocation::Header }).unwrap(),
            r#"{"type":"api_key","key":"k","value":"v","location":"header"}"#
        );
        assert_eq!(
            serde_json::to_string(&AuthConfig::OAuth2(OAuth2Config::default())).unwrap(),
            r#"{"type":"o_auth2","grantType":"client_credentials","authUrl":"","tokenUrl":"","clientId":"","clientSecret":"","scope":"","redirectUri":"","pkce":"s256","username":"","password":"","refreshToken":""}"#
        );
        assert_eq!(
            serde_json::to_string(&AuthConfig::AwsSigV4(AwsSigV4Config::default())).unwrap(),
            r#"{"type":"aws_sig_v4","accessKey":"","secretKey":"","region":"","service":"","sessionToken":""}"#
        );
    }

    /// `RequestAuth::Own` wraps another internally-tagged enum (`AuthConfig`)
    /// as its newtype payload — confirms serde flattens both tags as sibling
    /// keys on one object (`mode` + `type`) rather than nesting, since this
    /// shape is exactly what the frontend's discriminated union must match.
    #[test]
    fn request_auth_own_flattens_both_tags_into_one_object() {
        let auth = RequestAuth::Own(AuthConfig::Bearer { token: "tok".into(), prefix: crate::model::auth::default_bearer_prefix() });
        let json = serde_json::to_string(&auth).unwrap();
        assert_eq!(json, r#"{"mode":"own","type":"bearer","token":"tok","prefix":"Bearer"}"#);
        assert_eq!(serde_json::from_str::<RequestAuth>(&json).unwrap(), auth);

        assert_eq!(serde_json::to_string(&RequestAuth::Inherit).unwrap(), r#"{"mode":"inherit"}"#);
    }

    #[test]
    fn with_secret_field_only_touches_matching_variant() {
        let cfg = AuthConfig::Bearer { token: String::new(), prefix: crate::model::auth::default_bearer_prefix() }.with_secret_field("bearer-token", "tok".into());
        assert_eq!(cfg, AuthConfig::Bearer { token: "tok".into(), prefix: crate::model::auth::default_bearer_prefix() });

        // Wrong slot for this variant is a no-op, not a panic.
        let cfg = AuthConfig::Bearer { token: "tok".into(), prefix: crate::model::auth::default_bearer_prefix() }.with_secret_field("aws-secret-key", "nope".into());
        assert_eq!(cfg, AuthConfig::Bearer { token: "tok".into(), prefix: crate::model::auth::default_bearer_prefix() });
    }

    #[test]
    fn is_masked_true_for_empty_and_mask_false_for_real_value() {
        assert!(AuthConfig::Bearer { token: String::new(), prefix: crate::model::auth::default_bearer_prefix() }.is_masked());
        assert!(AuthConfig::Bearer { token: SECRET_MASK.into(), prefix: crate::model::auth::default_bearer_prefix() }.is_masked());
        assert!(!AuthConfig::Bearer { token: "real".into(), prefix: crate::model::auth::default_bearer_prefix() }.is_masked());
    }
}
