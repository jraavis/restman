//! C# (`HttpClient`) target.

use super::{auth_note, dquote, effective_headers, effective_query, full_url, plan_body, BodyPlan, CodegenOptions};
use crate::error::AppResult;
use crate::model::http::HttpRequest;

fn method_enum(method: &str) -> String {
    match method.to_uppercase().as_str() {
        "GET" => "HttpMethod.Get".to_string(),
        "POST" => "HttpMethod.Post".to_string(),
        "PUT" => "HttpMethod.Put".to_string(),
        "DELETE" => "HttpMethod.Delete".to_string(),
        "HEAD" => "HttpMethod.Head".to_string(),
        "OPTIONS" => "HttpMethod.Options".to_string(),
        "PATCH" => "HttpMethod.Patch".to_string(),
        other => format!("new HttpMethod({})", dquote(other)),
    }
}

pub fn generate(req: &HttpRequest, options: &CodegenOptions) -> AppResult<String> {
    let query = effective_query(req, options)?;
    let headers = effective_headers(req, options)?;
    let body = plan_body(&req.body);
    let url = full_url(req, &query);

    let mut usings = vec!["using System.Net.Http;"];
    match &body {
        BodyPlan::Text { .. } => usings.push("using System.Text;"),
        BodyPlan::FormData(_) | BodyPlan::Binary(_) => usings.push("using System.IO;"),
        BodyPlan::None => {}
    }

    let mut lines: Vec<String> = usings.into_iter().map(String::from).collect();
    lines.push(String::new());
    lines.push("var client = new HttpClient();".to_string());
    lines.push(format!("var request = new HttpRequestMessage({}, {});", method_enum(&req.method), dquote(&url)));
    if let Some(note) = auth_note(req, options)? {
        lines.push(format!("// {note}"));
    }
    for (k, v) in &headers {
        // Content-related headers (Content-Type, Content-Length, ...) belong on
        // HttpContent.Headers in .NET — HttpRequestMessage.Headers.Add throws if
        // given one. The Text branch below sets Content-Type via StringContent's
        // constructor instead, so this is a deliberate drop, not an oversight.
        if k.eq_ignore_ascii_case("content-type") {
            continue;
        }
        lines.push(format!("request.Headers.Add({}, {});", dquote(k), dquote(v)));
    }

    match &body {
        BodyPlan::None => {}
        BodyPlan::Text { content, content_type } => {
            let ct = content_type.unwrap_or("text/plain");
            lines.push(format!(
                "request.Content = new StringContent({}, Encoding.UTF8, {});",
                dquote(content),
                dquote(ct)
            ));
        }
        BodyPlan::FormData(fields) => {
            lines.push("var formContent = new MultipartFormDataContent();".to_string());
            for (key, value, is_file) in fields {
                if *is_file {
                    lines.push(format!(
                        "formContent.Add(new StreamContent(File.OpenRead({0})), {1}, Path.GetFileName({0}));",
                        dquote(value),
                        dquote(key)
                    ));
                } else {
                    lines.push(format!("formContent.Add(new StringContent({}), {});", dquote(value), dquote(key)));
                }
            }
            lines.push("request.Content = formContent;".to_string());
        }
        BodyPlan::Binary(path) => {
            lines.push(format!("request.Content = new StreamContent(File.OpenRead({}));", dquote(path)));
        }
    }

    lines.push(String::new());
    lines.push("var response = await client.SendAsync(request);".to_string());
    lines.push("Console.WriteLine(await response.Content.ReadAsStringAsync());".to_string());

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
            "using System.Net.Http;\n\nvar client = new HttpClient();\nvar request = new HttpRequestMessage(HttpMethod.Get, \"https://api.example.com/items?limit=5\");\nrequest.Headers.Add(\"Accept\", \"application/json\");\nrequest.Headers.Add(\"Authorization\", \"Bearer tok123\");\n\nvar response = await client.SendAsync(request);\nConsole.WriteLine(await response.Content.ReadAsStringAsync());"
        );
    }

    #[test]
    fn escapes_quote_backslash_and_newline_in_body() {
        let out = generate(&sample_json_post_request(), &CodegenOptions::default()).unwrap();
        let expected_body = dquote("a\"b\\c\nd");
        assert!(
            out.contains(&format!("request.Content = new StringContent({expected_body}, Encoding.UTF8, \"application/json\");")),
            "{out}"
        );
        let expected_auth = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, "alice:secret");
        assert!(out.contains(&format!("request.Headers.Add(\"Authorization\", \"Basic {expected_auth}\");")), "{out}");
        assert!(!out.contains("Headers.Add(\"Content-Type\""), "{out}");
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
    fn formdata_adds_a_string_part_and_a_stream_part_with_its_filename() {
        let out = generate(&sample_formdata_request(), &CodegenOptions::default()).unwrap();
        assert!(out.contains("formContent.Add(new StringContent(\"cute dog\"), \"caption\");"), "{out}");
        assert!(
            out.contains(
                "formContent.Add(new StreamContent(File.OpenRead(\"/tmp/fido.png\")), \"photo\", Path.GetFileName(\"/tmp/fido.png\"));"
            ),
            "{out}"
        );
        assert!(out.contains("request.Content = formContent;"), "{out}");
    }

    #[test]
    fn binary_body_streams_the_file_directly() {
        let out = generate(&sample_binary_request(), &CodegenOptions::default()).unwrap();
        assert!(out.contains("request.Content = new StreamContent(File.OpenRead(\"/tmp/payload.bin\"));"), "{out}");
    }
}
