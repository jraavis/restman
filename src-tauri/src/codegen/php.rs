//! PHP (Guzzle) target.

use super::{
    auth_note, effective_headers, effective_query, full_url, has_header, plan_body, squote, BodyPlan, CodegenOptions,
};
use crate::error::AppResult;
use crate::model::http::HttpRequest;

pub fn generate(req: &HttpRequest, options: &CodegenOptions) -> AppResult<String> {
    let query = effective_query(req, options)?;
    let mut headers = effective_headers(req, options)?;
    let body = plan_body(&req.body);
    let url = full_url(req, &query);

    let mut lines = vec![
        "<?php".to_string(),
        String::new(),
        "require 'vendor/autoload.php';".to_string(),
        String::new(),
        "use GuzzleHttp\\Client;".to_string(),
        String::new(),
    ];
    if let Some(note) = auth_note(req, options)? {
        lines.push(format!("// {note}"));
    }
    lines.push("$client = new Client();".to_string());

    let mut opts: Vec<String> = Vec::new();

    match &body {
        BodyPlan::None => {}
        BodyPlan::Text { content, content_type } => {
            if let Some(ct) = content_type {
                if !has_header(&headers, "content-type") {
                    headers.push(("Content-Type".into(), ct.to_string()));
                }
            }
            opts.push(format!("    'body' => {},", squote(content)));
        }
        BodyPlan::FormData(fields) => {
            opts.push("    'multipart' => [".to_string());
            for (key, value, is_file) in fields {
                if *is_file {
                    opts.push(format!(
                        "        ['name' => {}, 'contents' => fopen({}, 'r')],",
                        squote(key),
                        squote(value)
                    ));
                } else {
                    opts.push(format!("        ['name' => {}, 'contents' => {}],", squote(key), squote(value)));
                }
            }
            opts.push("    ],".to_string());
        }
        BodyPlan::Binary(path) => {
            opts.push(format!("    'body' => fopen({}, 'r'),", squote(path)));
        }
    }

    if !headers.is_empty() {
        opts.push("    'headers' => [".to_string());
        for (k, v) in &headers {
            opts.push(format!("        {} => {},", squote(k), squote(v)));
        }
        opts.push("    ],".to_string());
    }

    let opts_block = if opts.is_empty() { String::new() } else { format!(", [\n{}\n]", opts.join("\n")) };
    lines.push(format!("$response = $client->request({}, {}{});", squote(&req.method), squote(&url), opts_block));
    lines.push(String::new());
    lines.push("echo $response->getBody();".to_string());

    Ok(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::{
        sample_binary_request, sample_formdata_request, sample_get_request, sample_json_post_request,
        sample_sigv4_request, squote,
    };

    #[test]
    fn renders_canonical_get_with_bearer_auth() {
        let out = generate(&sample_get_request(), &CodegenOptions::default()).unwrap();
        assert_eq!(
            out,
            "<?php\n\nrequire 'vendor/autoload.php';\n\nuse GuzzleHttp\\Client;\n\n$client = new Client();\n$response = $client->request('GET', 'https://api.example.com/items?limit=5', [\n    'headers' => [\n        'Accept' => 'application/json',\n        'Authorization' => 'Bearer tok123',\n    ],\n]);\n\necho $response->getBody();"
        );
    }

    #[test]
    fn escapes_quote_backslash_and_newline_in_body() {
        let out = generate(&sample_json_post_request(), &CodegenOptions::default()).unwrap();
        let expected_body = squote("a\"b\\c\nd");
        assert!(out.contains(&format!("'body' => {expected_body},")), "{out}");
        let expected_auth = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, "alice:secret");
        assert!(out.contains(&format!("'Authorization' => 'Basic {expected_auth}',")), "{out}");
        assert!(out.contains("'Content-Type' => 'application/json',"), "{out}");
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
    fn formdata_renders_guzzle_multipart_array_with_a_file_stream() {
        let out = generate(&sample_formdata_request(), &CodegenOptions::default()).unwrap();
        assert!(out.contains("['name' => 'caption', 'contents' => 'cute dog'],"), "{out}");
        assert!(out.contains("['name' => 'photo', 'contents' => fopen('/tmp/fido.png', 'r')],"), "{out}");
        assert!(out.contains("'multipart' => ["), "{out}");
    }

    #[test]
    fn binary_body_passes_a_file_stream_as_the_body_option() {
        let out = generate(&sample_binary_request(), &CodegenOptions::default()).unwrap();
        assert!(out.contains("'body' => fopen('/tmp/payload.bin', 'r'),"), "{out}");
    }
}
