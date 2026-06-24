//! Python (`requests`) target.

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

    let mut lines = vec!["import requests".to_string(), String::new()];
    if let Some(note) = auth_note(req, options)? {
        lines.push(format!("# {note}"));
    }

    let mut kwargs: Vec<String> = Vec::new();

    match &body {
        BodyPlan::None => {}
        BodyPlan::Text { content, content_type } => {
            if let Some(ct) = content_type {
                if !has_header(&headers, "content-type") {
                    headers.push(("Content-Type".into(), ct.to_string()));
                }
            }
            kwargs.push(format!("    data={},", dquote(content)));
        }
        BodyPlan::FormData(fields) => {
            lines.push("files = {".to_string());
            for (key, value, is_file) in fields {
                if *is_file {
                    lines.push(format!("    {}: open({}, \"rb\"),", dquote(key), dquote(value)));
                } else {
                    lines.push(format!("    {}: (None, {}),", dquote(key), dquote(value)));
                }
            }
            lines.push("}".to_string());
            lines.push(String::new());
            kwargs.push("    files=files,".to_string());
        }
        BodyPlan::Binary(path) => {
            kwargs.push(format!("    data=open({}, \"rb\"),", dquote(path)));
        }
    }

    if !headers.is_empty() {
        lines.push("headers = {".to_string());
        for (k, v) in &headers {
            lines.push(format!("    {}: {},", dquote(k), dquote(v)));
        }
        lines.push("}".to_string());
        lines.push(String::new());
        kwargs.push("    headers=headers,".to_string());
    }

    let kwargs_str = if kwargs.is_empty() { String::new() } else { format!("\n{}", kwargs.join("\n")) };
    lines.push(format!(
        "response = requests.request(\n    {},\n    {},{}\n)",
        dquote(&req.method),
        dquote(&url),
        kwargs_str
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
            "import requests\n\nheaders = {\n    \"Accept\": \"application/json\",\n    \"Authorization\": \"Bearer tok123\",\n}\n\nresponse = requests.request(\n    \"GET\",\n    \"https://api.example.com/items?limit=5\",\n    headers=headers,\n)"
        );
    }

    #[test]
    fn escapes_quote_backslash_and_newline_in_body() {
        let out = generate(&sample_json_post_request(), &CodegenOptions::default()).unwrap();
        let expected_body = dquote("a\"b\\c\nd");
        assert!(out.contains(&format!("data={expected_body},")), "{out}");
        let expected_auth = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, "alice:secret");
        assert!(out.contains(&format!("\"Authorization\": \"Basic {expected_auth}\"")), "{out}");
        assert!(out.contains("\"Content-Type\": \"application/json\""), "{out}");
    }

    #[test]
    fn aws_sigv4_emits_comment_and_no_auth_header() {
        let out = generate(&sample_sigv4_request(), &CodegenOptions::default()).unwrap();
        assert!(out.starts_with("import requests\n\n# AWS SigV4"), "{out}");
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
    fn formdata_renders_text_field_as_tuple_and_file_field_as_open() {
        let out = generate(&sample_formdata_request(), &CodegenOptions::default()).unwrap();
        assert!(out.contains("\"caption\": (None, \"cute dog\"),"), "{out}");
        assert!(out.contains("\"photo\": open(\"/tmp/fido.png\", \"rb\"),"), "{out}");
        assert!(out.contains("files=files,"), "{out}");
    }

    #[test]
    fn binary_body_opens_file_in_binary_mode() {
        let out = generate(&sample_binary_request(), &CodegenOptions::default()).unwrap();
        assert!(out.contains("data=open(\"/tmp/payload.bin\", \"rb\"),"), "{out}");
    }
}
