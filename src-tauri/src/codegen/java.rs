//! Java (OkHttp) target.

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

    let mut lines = vec!["OkHttpClient client = new OkHttpClient();".to_string()];
    if let Some(note) = auth_note(req, options)? {
        lines.push(format!("// {note}"));
    }

    let mut setup: Vec<String> = Vec::new();
    let body_expr = match &body {
        BodyPlan::None => "null".to_string(),
        BodyPlan::Text { content, content_type } => {
            if let Some(ct) = content_type {
                if !has_header(&headers, "content-type") {
                    headers.push(("Content-Type".into(), ct.to_string()));
                }
            }
            let ct = content_type.unwrap_or("text/plain");
            // OkHttp's `RequestBody.create` takes the MediaType first, then
            // the content — easy to get backwards since it reads like the
            // opposite order in plenty of stale tutorials.
            format!("RequestBody.create(MediaType.parse({}), {})", dquote(ct), dquote(content))
        }
        BodyPlan::FormData(fields) => {
            setup.push("MultipartBody requestBody = new MultipartBody.Builder()".to_string());
            setup.push("    .setType(MultipartBody.FORM)".to_string());
            for (key, value, is_file) in fields {
                if *is_file {
                    setup.push(format!(
                        "    .addFormDataPart({}, new File({}).getName(), RequestBody.create(MediaType.parse(\"application/octet-stream\"), new File({})))",
                        dquote(key), dquote(value), dquote(value)
                    ));
                } else {
                    setup.push(format!("    .addFormDataPart({}, {})", dquote(key), dquote(value)));
                }
            }
            setup.push("    .build();".to_string());
            "requestBody".to_string()
        }
        BodyPlan::Binary(path) => {
            format!(
                "RequestBody.create(MediaType.parse(\"application/octet-stream\"), new File({}))",
                dquote(path)
            )
        }
    };

    if !setup.is_empty() {
        lines.push(String::new());
        lines.extend(setup);
    }

    let mut builder_lines = vec![
        "Request request = new Request.Builder()".to_string(),
        format!("    .url({})", dquote(&url)),
        format!("    .method({}, {body_expr})", dquote(&req.method)),
    ];
    for (k, v) in &headers {
        builder_lines.push(format!("    .addHeader({}, {})", dquote(k), dquote(v)));
    }
    builder_lines.push("    .build();".to_string());

    lines.push(String::new());
    lines.push(builder_lines.join("\n"));
    lines.push(String::new());
    lines.push("Response response = client.newCall(request).execute();".to_string());
    lines.push("System.out.println(response.body().string());".to_string());

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
            "OkHttpClient client = new OkHttpClient();\n\nRequest request = new Request.Builder()\n    .url(\"https://api.example.com/items?limit=5\")\n    .method(\"GET\", null)\n    .addHeader(\"Accept\", \"application/json\")\n    .addHeader(\"Authorization\", \"Bearer tok123\")\n    .build();\n\nResponse response = client.newCall(request).execute();\nSystem.out.println(response.body().string());"
        );
    }

    #[test]
    fn escapes_quote_backslash_and_newline_in_body() {
        let out = generate(&sample_json_post_request(), &CodegenOptions::default()).unwrap();
        let expected_body = dquote("a\"b\\c\nd");
        assert!(out.contains(&format!("RequestBody.create(MediaType.parse(\"application/json\"), {expected_body})")), "{out}");
        let expected_auth = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, "alice:secret");
        assert!(out.contains(&format!(".addHeader(\"Authorization\", \"Basic {expected_auth}\")")), "{out}");
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
    fn formdata_builds_a_multipart_body_with_a_text_part_and_a_file_part() {
        let out = generate(&sample_formdata_request(), &CodegenOptions::default()).unwrap();
        assert!(out.contains(".addFormDataPart(\"caption\", \"cute dog\")"), "{out}");
        assert!(
            out.contains(
                ".addFormDataPart(\"photo\", new File(\"/tmp/fido.png\").getName(), RequestBody.create(MediaType.parse(\"application/octet-stream\"), new File(\"/tmp/fido.png\")))"
            ),
            "{out}"
        );
        assert!(out.contains(".method(\"POST\", requestBody)"), "{out}");
    }

    #[test]
    fn binary_body_wraps_the_file_in_a_request_body() {
        let out = generate(&sample_binary_request(), &CodegenOptions::default()).unwrap();
        assert!(
            out.contains(
                "RequestBody.create(MediaType.parse(\"application/octet-stream\"), new File(\"/tmp/payload.bin\"))"
            ),
            "{out}"
        );
    }
}
