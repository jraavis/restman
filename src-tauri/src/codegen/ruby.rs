//! Ruby (`Net::HTTP`) target.

use super::{auth_note, effective_headers, effective_query, full_url, has_header, plan_body, squote, BodyPlan, CodegenOptions};
use crate::error::AppResult;
use crate::model::http::HttpRequest;

fn request_new(method: &str) -> String {
    match method.to_uppercase().as_str() {
        "GET" => "Net::HTTP::Get.new(uri)".to_string(),
        "POST" => "Net::HTTP::Post.new(uri)".to_string(),
        "PUT" => "Net::HTTP::Put.new(uri)".to_string(),
        "DELETE" => "Net::HTTP::Delete.new(uri)".to_string(),
        "HEAD" => "Net::HTTP::Head.new(uri)".to_string(),
        "OPTIONS" => "Net::HTTP::Options.new(uri)".to_string(),
        "PATCH" => "Net::HTTP::Patch.new(uri)".to_string(),
        other => format!("Net::HTTPGenericRequest.new({}, true, true, uri)", squote(other)),
    }
}

pub fn generate(req: &HttpRequest, options: &CodegenOptions) -> AppResult<String> {
    let query = effective_query(req, options)?;
    let mut headers = effective_headers(req, options)?;
    let body = plan_body(&req.body);
    let url = full_url(req, &query);

    if let BodyPlan::Text { content_type: Some(ct), .. } = &body {
        if !has_header(&headers, "content-type") {
            headers.push(("Content-Type".into(), ct.to_string()));
        }
    }

    let mut lines = vec![
        "require 'net/http'".to_string(),
        "require 'uri'".to_string(),
        String::new(),
        format!("uri = URI({})", squote(&url)),
        "http = Net::HTTP.new(uri.host, uri.port)".to_string(),
        "http.use_ssl = (uri.scheme == 'https')".to_string(),
        String::new(),
    ];
    if let Some(note) = auth_note(req, options)? {
        lines.push(format!("# {note}"));
    }
    lines.push(format!("request = {}", request_new(&req.method)));

    for (k, v) in &headers {
        lines.push(format!("request[{}] = {}", squote(k), squote(v)));
    }

    match body {
        BodyPlan::None => {}
        BodyPlan::Text { content, .. } => lines.push(format!("request.body = {}", squote(&content))),
        BodyPlan::FormData(fields) => {
            lines.push("request.set_form([".to_string());
            for (key, value, is_file) in fields {
                if is_file {
                    lines.push(format!("  [{}, File.open({})],", squote(&key), squote(&value)));
                } else {
                    lines.push(format!("  [{}, {}],", squote(&key), squote(&value)));
                }
            }
            lines.push("], 'multipart/form-data')".to_string());
        }
        BodyPlan::Binary(path) => lines.push(format!("request.body = File.read({})", squote(&path))),
    }

    lines.push(String::new());
    lines.push("response = http.request(request)".to_string());
    lines.push("puts response.body".to_string());

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
            "require 'net/http'\nrequire 'uri'\n\nuri = URI('https://api.example.com/items?limit=5')\nhttp = Net::HTTP.new(uri.host, uri.port)\nhttp.use_ssl = (uri.scheme == 'https')\n\nrequest = Net::HTTP::Get.new(uri)\nrequest['Accept'] = 'application/json'\nrequest['Authorization'] = 'Bearer tok123'\n\nresponse = http.request(request)\nputs response.body"
        );
    }

    #[test]
    fn escapes_quote_backslash_and_newline_in_body() {
        let out = generate(&sample_json_post_request(), &CodegenOptions::default()).unwrap();
        let expected_body = squote("a\"b\\c\nd");
        assert!(out.contains(&format!("request.body = {expected_body}")), "{out}");
        let expected_auth = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, "alice:secret");
        assert!(out.contains(&format!("request['Authorization'] = 'Basic {expected_auth}'")), "{out}");
        assert!(out.contains("request['Content-Type'] = 'application/json'"), "{out}");
    }

    #[test]
    fn aws_sigv4_emits_comment_and_no_auth_header() {
        let out = generate(&sample_sigv4_request(), &CodegenOptions::default()).unwrap();
        assert!(out.contains("# AWS SigV4"), "{out}");
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
    fn formdata_passes_set_form_a_plain_pair_and_an_open_file_pair() {
        let out = generate(&sample_formdata_request(), &CodegenOptions::default()).unwrap();
        assert!(out.contains("['caption', 'cute dog'],"), "{out}");
        assert!(out.contains("['photo', File.open('/tmp/fido.png')],"), "{out}");
        assert!(out.contains("request.set_form(["), "{out}");
        assert!(out.contains("'multipart/form-data')"), "{out}");
    }

    #[test]
    fn binary_body_reads_the_whole_file_into_the_request_body() {
        let out = generate(&sample_binary_request(), &CodegenOptions::default()).unwrap();
        assert!(out.contains("request.body = File.read('/tmp/payload.bin')"), "{out}");
    }
}
