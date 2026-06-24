//! JavaScript (`fetch`) target.

use super::{
    auth_note, dquote, effective_headers, effective_query, full_url, has_header, plan_body, BodyPlan, CodegenOptions,
};
use crate::error::AppResult;
use crate::model::http::HttpRequest;

pub fn generate(req: &HttpRequest, options: &CodegenOptions) -> AppResult<String> {
    let query = effective_query(req, options)?;
    let mut headers = effective_headers(req, options)?;
    let body = plan_body(&req.body);
    let url = full_url(req, &query);

    let mut lines = Vec::new();
    if let Some(note) = auth_note(req, options)? {
        lines.push(format!("// {note}"));
    }

    let mut setup: Vec<String> = Vec::new();
    let body_expr: Option<String> = match &body {
        BodyPlan::None => None,
        BodyPlan::Text { content, content_type } => {
            if let Some(ct) = content_type {
                if !has_header(&headers, "content-type") {
                    headers.push(("Content-Type".into(), ct.to_string()));
                }
            }
            Some(dquote(content))
        }
        BodyPlan::FormData(fields) => {
            setup.push("const formData = new FormData();".to_string());
            for (key, value, is_file) in fields {
                if *is_file {
                    setup.push(format!(
                        "formData.append({}, /* TODO: read this path into a File/Blob */ {});",
                        dquote(key),
                        dquote(value)
                    ));
                } else {
                    setup.push(format!("formData.append({}, {});", dquote(key), dquote(value)));
                }
            }
            Some("formData".to_string())
        }
        BodyPlan::Binary(path) => {
            setup.push(format!("// binary body — read the file at {} into bytes before sending", dquote(path)));
            Some(format!("/* TODO: bytes of {} */ undefined", dquote(path)))
        }
    };
    lines.extend(setup);

    let mut opts_lines = vec![format!("  method: {}", dquote(&req.method))];
    if !headers.is_empty() {
        let header_entries: Vec<String> =
            headers.iter().map(|(k, v)| format!("    {}: {}", dquote(k), dquote(v))).collect();
        opts_lines.push(format!("  headers: {{\n{}\n  }}", header_entries.join(",\n")));
    }
    if let Some(expr) = &body_expr {
        opts_lines.push(format!("  body: {expr}"));
    }

    lines.push(format!(
        "const response = await fetch({}, {{\n{}\n}});",
        dquote(&url),
        opts_lines.join(",\n")
    ));

    Ok(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::{
        dquote, sample_binary_request, sample_formdata_request, sample_get_request, sample_json_post_request,
        sample_sigv4_request,
    };

    #[test]
    fn renders_canonical_get_with_bearer_auth() {
        let out = generate(&sample_get_request(), &CodegenOptions::default()).unwrap();
        assert_eq!(
            out,
            "const response = await fetch(\"https://api.example.com/items?limit=5\", {\n  method: \"GET\",\n  headers: {\n    \"Accept\": \"application/json\",\n    \"Authorization\": \"Bearer tok123\"\n  }\n});"
        );
    }

    #[test]
    fn escapes_quote_backslash_and_newline_in_body() {
        let out = generate(&sample_json_post_request(), &CodegenOptions::default()).unwrap();
        let expected_body = dquote("a\"b\\c\nd");
        assert!(out.contains(&format!("body: {expected_body}")), "{out}");
        let expected_auth = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, "alice:secret");
        assert!(out.contains(&format!("\"Authorization\": \"Basic {expected_auth}\"")), "{out}");
        assert!(out.contains("\"Content-Type\": \"application/json\""), "{out}");
    }

    #[test]
    fn aws_sigv4_emits_comment_and_no_auth_header() {
        let out = generate(&sample_sigv4_request(), &CodegenOptions::default()).unwrap();
        assert!(out.starts_with("// AWS SigV4"), "{out}");
        assert!(!out.contains("Authorization"), "{out}");
    }

    #[test]
    fn formdata_renders_text_field_and_flags_file_field_for_manual_completion() {
        let out = generate(&sample_formdata_request(), &CodegenOptions::default()).unwrap();
        assert!(out.contains("formData.append(\"caption\", \"cute dog\");"), "{out}");
        assert!(out.contains("formData.append(\"photo\", /* TODO"), "{out}");
        assert!(out.contains("body: formData"), "{out}");
    }

    #[test]
    fn binary_body_is_flagged_for_manual_completion() {
        let out = generate(&sample_binary_request(), &CodegenOptions::default()).unwrap();
        assert!(out.contains("/* TODO: bytes of \"/tmp/payload.bin\" */"), "{out}");
    }
}
