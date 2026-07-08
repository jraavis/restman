//! Plugin-backed code generation. Same shape as the native per-language
//! `generate(req, options) -> String` dispatch, but `req`/`options` are
//! JSON-serialized and handed to a sandboxed JS plugin (`plugins::runtime`)
//! instead of a compiled Rust function — see `super::generate` for the
//! native counterpart.
//!
//! Unlike the native generators, which already see a fully resolved
//! `HttpRequest` (real OAuth2 token or `OAUTH2_TOKEN_PLACEHOLDER`, per
//! `commands::codegen::generate_code`'s doc comment), a plugin is
//! third-party JS, not trusted Rust in this binary. Every non-empty secret
//! field on `req.auth` is replaced with `PLUGIN_SECRET_PLACEHOLDER` before
//! the request ever reaches the sandbox.

use crate::error::AppResult;
use crate::model::auth::AuthConfig;
use crate::model::http::HttpRequest;

use super::CodegenOptions;

/// Substituted for every non-empty secret field on `req.auth` before a
/// request crosses into a sandboxed plugin. Distinct from
/// `super::OAUTH2_TOKEN_PLACEHOLDER` (that one means "no fresh token cached
/// yet"; this one means "a real secret existed but plugins never see real
/// secrets, full stop").
pub const PLUGIN_SECRET_PLACEHOLDER: &str = "<PLUGIN_SECRET_REDACTED>";

fn mask_auth_for_plugin(auth: AuthConfig) -> AuthConfig {
    let non_empty_slots: Vec<&'static str> = auth
        .secret_fields()
        .into_iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(slot, _)| slot)
        .collect();
    let mut auth = auth;
    for slot in non_empty_slots {
        auth = auth.with_secret_field(slot, PLUGIN_SECRET_PLACEHOLDER.to_string());
    }
    auth
}

/// Run a codegen plugin's `generate(request, options)` against `req`,
/// masking secrets first. Mirrors `super::generate`'s signature minus the
/// `CodeLanguage` selector — the plugin's own source is the dispatch target.
pub fn generate(source: &str, req: &HttpRequest, options: &CodegenOptions) -> AppResult<String> {
    let mut masked = req.clone();
    masked.auth = mask_auth_for_plugin(masked.auth);

    let args = [serde_json::to_value(&masked)?, serde_json::to_value(options)?];
    crate::plugins::call_returning_string(source, "generate", &args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::auth::AuthConfig;

    #[test]
    fn plugin_never_sees_a_real_bearer_token() {
        let req = HttpRequest {
            method: "GET".into(),
            url: "https://api.example.com/items".into(),
            auth: AuthConfig::Bearer { token: "real-secret-token".into(), prefix: crate::model::auth::default_bearer_prefix() },
            ..Default::default()
        };
        let options = CodegenOptions::default();
        let out = generate(
            r#"function generate(req, options) { return JSON.stringify(req.auth); }"#,
            &req,
            &options,
        )
        .unwrap();
        assert!(!out.contains("real-secret-token"), "{out}");
        assert!(out.contains(PLUGIN_SECRET_PLACEHOLDER), "{out}");
    }

    #[test]
    fn plugin_with_no_auth_is_unaffected() {
        let req = HttpRequest { method: "GET".into(), url: "https://api.example.com".into(), ..Default::default() };
        let options = CodegenOptions::default();
        let out = generate(r#"function generate(req) { return req.method + " " + req.url; }"#, &req, &options)
            .unwrap();
        assert_eq!(out, "GET https://api.example.com");
    }

    #[test]
    fn options_are_passed_through() {
        let req = HttpRequest::default();
        let options = CodegenOptions { include_auth: false, include_headers: true };
        let out = generate(
            r#"function generate(req, options) { return String(options.includeAuth) + "/" + String(options.includeHeaders); }"#,
            &req,
            &options,
        )
        .unwrap();
        assert_eq!(out, "false/true");
    }
}
