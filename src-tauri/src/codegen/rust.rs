//! Rust (`reqwest`) target.

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

    let mut lines = vec![
        "#[tokio::main]".to_string(),
        "async fn main() -> Result<(), Box<dyn std::error::Error>> {".to_string(),
        "    let client = reqwest::Client::new();".to_string(),
    ];
    if let Some(note) = auth_note(req, options)? {
        lines.push(format!("    // {note}"));
    }

    let mut setup: Vec<String> = Vec::new();
    let mut chain = vec![format!(
        "    let response = client\n        .request(reqwest::Method::{}, {})",
        req.method.to_uppercase(),
        dquote(&url)
    )];

    match &body {
        BodyPlan::None => {}
        BodyPlan::Text { content, content_type } => {
            if let Some(ct) = content_type {
                if !has_header(&headers, "content-type") {
                    headers.push(("Content-Type".into(), ct.to_string()));
                }
            }
            chain.push(format!("        .body({})", dquote(content)));
        }
        BodyPlan::FormData(fields) => {
            setup.push(
                "    // requires reqwest = { features = [\"multipart\", \"stream\"] } in Cargo.toml"
                    .to_string(),
            );
            let mut form_expr = "reqwest::multipart::Form::new()".to_string();
            for (key, value, is_file) in fields {
                if !*is_file {
                    form_expr.push_str(&format!(".text({}, {})", dquote(key), dquote(value)));
                }
            }
            setup.push(format!("    let mut form = {form_expr};"));
            for (key, value, is_file) in fields {
                if *is_file {
                    setup.push(format!("    form = form.file({}, {}).await?;", dquote(key), dquote(value)));
                }
            }
            chain.push("        .multipart(form)".to_string());
        }
        BodyPlan::Binary(path) => {
            setup.push(format!("    let file_bytes = std::fs::read({})?;", dquote(path)));
            chain.push("        .body(file_bytes)".to_string());
        }
    }

    for (k, v) in &headers {
        chain.push(format!("        .header({}, {})", dquote(k), dquote(v)));
    }
    chain.push("        .send()".to_string());
    chain.push("        .await?;".to_string());

    lines.extend(setup);
    lines.push(chain.join("\n"));
    lines.push("    println!(\"{}\", response.status());".to_string());
    lines.push("    Ok(())".to_string());
    lines.push("}".to_string());

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
            "#[tokio::main]\nasync fn main() -> Result<(), Box<dyn std::error::Error>> {\n    let client = reqwest::Client::new();\n    let response = client\n        .request(reqwest::Method::GET, \"https://api.example.com/items?limit=5\")\n        .header(\"Accept\", \"application/json\")\n        .header(\"Authorization\", \"Bearer tok123\")\n        .send()\n        .await?;\n    println!(\"{}\", response.status());\n    Ok(())\n}"
        );
    }

    #[test]
    fn escapes_quote_backslash_and_newline_in_body() {
        let out = generate(&sample_json_post_request(), &CodegenOptions::default()).unwrap();
        let expected_body = dquote("a\"b\\c\nd");
        assert!(out.contains(&format!(".body({expected_body})")), "{out}");
        let expected_auth = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, "alice:secret");
        assert!(out.contains(&format!(".header(\"Authorization\", \"Basic {expected_auth}\")")), "{out}");
        assert!(out.contains(".header(\"Content-Type\", \"application/json\")"), "{out}");
    }

    #[test]
    fn aws_sigv4_emits_comment_and_no_auth_header() {
        let out = generate(&sample_sigv4_request(), &CodegenOptions::default()).unwrap();
        assert!(out.contains("// AWS SigV4"), "{out}");
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
    fn formdata_builds_a_multipart_form_with_text_then_awaits_the_file_part() {
        let out = generate(&sample_formdata_request(), &CodegenOptions::default()).unwrap();
        assert!(out.contains("reqwest::multipart::Form::new().text(\"caption\", \"cute dog\")"), "{out}");
        assert!(out.contains("form = form.file(\"photo\", \"/tmp/fido.png\").await?;"), "{out}");
        assert!(out.contains(".multipart(form)"), "{out}");
        assert!(out.contains("multipart\", \"stream"), "{out}");
    }

    #[test]
    fn binary_body_reads_the_file_into_bytes_before_sending() {
        let out = generate(&sample_binary_request(), &CodegenOptions::default()).unwrap();
        assert!(out.contains("std::fs::read(\"/tmp/payload.bin\")?"), "{out}");
        assert!(out.contains(".body(file_bytes)"), "{out}");
    }
}
