//! cURL command target. Distinct from `interop::curl::export`: that module
//! renders the import/export IR (`ImportedRequest`/`RequestAuth`, auth
//! masked at rest); this one renders an already-resolved `HttpRequest`/
//! `AuthConfig` and bakes real values in, per `codegen`'s module doc.

use super::{
    auth_note, effective_headers, effective_query, full_url, has_header, plan_body, shquote, BodyPlan, CodegenOptions,
};
use crate::error::AppResult;
use crate::model::http::HttpRequest;

pub fn generate(req: &HttpRequest, options: &CodegenOptions) -> AppResult<String> {
    let query = effective_query(req, options)?;
    let mut headers = effective_headers(req, options)?;
    let body = plan_body(&req.body);

    let mut lines = Vec::new();
    if let Some(note) = auth_note(req, options)? {
        lines.push(format!("# {note}"));
    }
    lines.push(format!("curl -X {} {}", req.method, shquote(&full_url(req, &query))));

    if !req.options.verify_ssl {
        lines.push("  -k".to_string());
    }
    if req.options.follow_redirects {
        lines.push("  -L".to_string());
    }
    lines.push(format!("  --max-time {}", req.options.timeout_secs));
    lines.push(format!("  --max-redirs {}", req.options.max_redirects));

    if let BodyPlan::Text { content_type: Some(ct), .. } = &body {
        if !has_header(&headers, "content-type") {
            headers.push(("Content-Type".into(), ct.to_string()));
        }
    }
    for (name, value) in &headers {
        lines.push(format!("  -H {}", shquote(&format!("{name}: {value}"))));
    }

    match body {
        BodyPlan::None => {}
        BodyPlan::Text { content, .. } => lines.push(format!("  --data-raw {}", shquote(&content))),
        BodyPlan::FormData(fields) => {
            for (key, value, is_file) in fields {
                let raw = if is_file { format!("{key}=@{value}") } else { format!("{key}={value}") };
                lines.push(format!("  -F {}", shquote(&raw)));
            }
        }
        BodyPlan::Binary(path) => lines.push(format!("  --data-binary {}", shquote(&format!("@{path}")))),
    }

    Ok(lines.join(" \\\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::{
        sample_binary_request, sample_formdata_request, sample_get_request, sample_json_post_request,
        sample_sigv4_request, shquote,
    };

    #[test]
    fn renders_canonical_get_with_bearer_auth() {
        let out = generate(&sample_get_request(), &CodegenOptions::default()).unwrap();
        assert_eq!(
            out,
            "curl -X GET 'https://api.example.com/items?limit=5' \\\n  -L \\\n  --max-time 30 \\\n  --max-redirs 10 \\\n  -H 'Accept: application/json' \\\n  -H 'Authorization: Bearer tok123'"
        );
    }

    #[test]
    fn escapes_quote_backslash_and_newline_in_body() {
        let out = generate(&sample_json_post_request(), &CodegenOptions::default()).unwrap();
        let expected_body = shquote("a\"b\\c\nd");
        assert!(out.contains(&format!("--data-raw {expected_body}")), "{out}");
        let expected_auth =
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, "alice:secret");
        assert!(out.contains(&format!("Authorization: Basic {expected_auth}")), "{out}");
    }

    #[test]
    fn aws_sigv4_emits_comment_and_no_auth_header() {
        let out = generate(&sample_sigv4_request(), &CodegenOptions::default()).unwrap();
        assert!(out.starts_with("# AWS SigV4"), "{out}");
        assert!(!out.contains("Authorization"), "{out}");
    }

    #[test]
    fn include_auth_false_omits_header_and_comment() {
        let options = CodegenOptions { include_auth: false, include_headers: true };
        let out = generate(&sample_sigv4_request(), &options).unwrap();
        assert!(!out.contains("AWS SigV4"), "{out}");
        let out = generate(&sample_get_request(), &options).unwrap();
        assert!(!out.contains("Authorization"), "{out}");
    }

    #[test]
    fn formdata_renders_text_field_and_file_field() {
        let out = generate(&sample_formdata_request(), &CodegenOptions::default()).unwrap();
        assert!(out.contains("-F 'caption=cute dog'"), "{out}");
        assert!(out.contains("-F 'photo=@/tmp/fido.png'"), "{out}");
    }

    #[test]
    fn binary_body_uses_data_binary_with_at_path() {
        let out = generate(&sample_binary_request(), &CodegenOptions::default()).unwrap();
        assert!(out.contains("--data-binary '@/tmp/payload.bin'"), "{out}");
    }
}
