//! Go (`net/http`) target.

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

    let mut imports: Vec<&str> = vec!["\"fmt\"", "\"io\"", "\"net/http\""];
    let mut setup: Vec<String> = Vec::new();
    let mut extra_header_lines: Vec<String> = Vec::new();

    let body_arg = match &body {
        BodyPlan::None => "nil".to_string(),
        BodyPlan::Text { content, content_type } => {
            imports.push("\"strings\"");
            if let Some(ct) = content_type {
                if !has_header(&headers, "content-type") {
                    headers.push(("Content-Type".into(), ct.to_string()));
                }
            }
            format!("strings.NewReader({})", dquote(content))
        }
        BodyPlan::FormData(fields) => {
            imports.push("\"bytes\"");
            imports.push("\"mime/multipart\"");
            setup.push("\tvar buf bytes.Buffer".to_string());
            setup.push("\tmw := multipart.NewWriter(&buf)".to_string());
            let mut file_idx = 0;
            for (key, value, is_file) in fields {
                if *is_file {
                    file_idx += 1;
                    imports.push("\"os\"");
                    imports.push("\"path/filepath\"");
                    setup.push(format!(
                        "\tfw{file_idx}, _ := mw.CreateFormFile({}, filepath.Base({}))",
                        dquote(key),
                        dquote(value)
                    ));
                    setup.push(format!("\tff{file_idx}, _ := os.Open({})", dquote(value)));
                    setup.push(format!("\tio.Copy(fw{file_idx}, ff{file_idx})"));
                    setup.push(format!("\tff{file_idx}.Close()"));
                } else {
                    setup.push(format!("\tmw.WriteField({}, {})", dquote(key), dquote(value)));
                }
            }
            setup.push("\tmw.Close()".to_string());
            extra_header_lines.push("\treq.Header.Set(\"Content-Type\", mw.FormDataContentType())".to_string());
            "&buf".to_string()
        }
        BodyPlan::Binary(path) => {
            imports.push("\"os\"");
            setup.push(format!("\tf, ferr := os.Open({})", dquote(path)));
            setup.push("\tif ferr != nil {\n\t\tpanic(ferr)\n\t}".to_string());
            "f".to_string()
        }
    };

    imports.sort_unstable();
    imports.dedup();

    let mut lines = vec!["package main".to_string(), String::new(), "import (".to_string()];
    for imp in &imports {
        lines.push(format!("\t{imp}"));
    }
    lines.push(")".to_string());
    lines.push(String::new());
    if let Some(note) = auth_note(req, options)? {
        lines.push(format!("// {note}"));
    }
    lines.push("func main() {".to_string());
    lines.extend(setup);
    lines.push(format!("\treq, err := http.NewRequest({}, {}, {body_arg})", dquote(&req.method), dquote(&url)));
    lines.push("\tif err != nil {\n\t\tpanic(err)\n\t}".to_string());
    for (k, v) in &headers {
        lines.push(format!("\treq.Header.Set({}, {})", dquote(k), dquote(v)));
    }
    lines.extend(extra_header_lines);
    lines.push(String::new());
    lines.push("\tclient := &http.Client{}".to_string());
    lines.push("\tresp, err := client.Do(req)".to_string());
    lines.push("\tif err != nil {\n\t\tpanic(err)\n\t}".to_string());
    lines.push("\tdefer resp.Body.Close()".to_string());
    lines.push("\trespBody, _ := io.ReadAll(resp.Body)".to_string());
    lines.push("\tfmt.Println(resp.Status, string(respBody))".to_string());
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
            "package main\n\nimport (\n\t\"fmt\"\n\t\"io\"\n\t\"net/http\"\n)\n\nfunc main() {\n\treq, err := http.NewRequest(\"GET\", \"https://api.example.com/items?limit=5\", nil)\n\tif err != nil {\n\t\tpanic(err)\n\t}\n\treq.Header.Set(\"Accept\", \"application/json\")\n\treq.Header.Set(\"Authorization\", \"Bearer tok123\")\n\n\tclient := &http.Client{}\n\tresp, err := client.Do(req)\n\tif err != nil {\n\t\tpanic(err)\n\t}\n\tdefer resp.Body.Close()\n\trespBody, _ := io.ReadAll(resp.Body)\n\tfmt.Println(resp.Status, string(respBody))\n}"
        );
    }

    #[test]
    fn escapes_quote_backslash_and_newline_in_body() {
        let out = generate(&sample_json_post_request(), &CodegenOptions::default()).unwrap();
        let expected_body = dquote("a\"b\\c\nd");
        assert!(out.contains(&format!("strings.NewReader({expected_body})")), "{out}");
        let expected_auth = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, "alice:secret");
        assert!(out.contains(&format!("req.Header.Set(\"Authorization\", \"Basic {expected_auth}\")")), "{out}");
        assert!(out.contains("req.Header.Set(\"Content-Type\", \"application/json\")"), "{out}");
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
    fn formdata_writes_text_field_and_streams_file_field_into_a_multipart_writer() {
        let out = generate(&sample_formdata_request(), &CodegenOptions::default()).unwrap();
        assert!(out.contains("mw.WriteField(\"caption\", \"cute dog\")"), "{out}");
        assert!(out.contains("mw.CreateFormFile(\"photo\", filepath.Base(\"/tmp/fido.png\"))"), "{out}");
        assert!(out.contains("os.Open(\"/tmp/fido.png\")"), "{out}");
        assert!(out.contains("req.Header.Set(\"Content-Type\", mw.FormDataContentType())"), "{out}");
    }

    #[test]
    fn binary_body_opens_file_as_the_request_reader() {
        let out = generate(&sample_binary_request(), &CodegenOptions::default()).unwrap();
        assert!(out.contains("os.Open(\"/tmp/payload.bin\")"), "{out}");
        assert!(out.contains("http.NewRequest(\"POST\", \"https://api.example.com/upload\", f)"), "{out}");
    }
}
