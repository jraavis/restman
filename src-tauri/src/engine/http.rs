//! HTTP engine: turns an `HttpRequest` spec into a reqwest call and captures
//! the response plus timing.
//!
//! Timing note: this first cut measures total / TTFB / download from the
//! client side. Per-phase DNS/TCP/TLS timing needs an instrumented connector
//! (reqwest exposes only total) and is a tracked Phase-1 follow-up; those
//! `Timing` fields stay `None` until then.

use crate::error::{AppError, AppResult};
use crate::model::http::*;
use base64::Engine as _;
use reqwest::header::CONTENT_TYPE;
use reqwest::{Client, Method, RequestBuilder, Url};
use std::time::{Duration, Instant};

pub async fn send(req: HttpRequest) -> AppResult<HttpResponse> {
    let client = build_client(&req.options)?;

    let method = Method::from_bytes(req.method.trim().as_bytes())
        .map_err(|_| AppError::Other(format!("invalid HTTP method: {}", req.method)))?;

    let url = build_url(&req.url, &req.query)?;

    let mut builder = client.request(method, url);

    // Apply enabled headers; note whether the user set Content-Type so body
    // modes don't clobber it.
    let mut user_has_content_type = false;
    for h in req.headers.iter().filter(|h| h.enabled && !h.name.trim().is_empty()) {
        if h.name.eq_ignore_ascii_case("content-type") {
            user_has_content_type = true;
        }
        builder = builder.header(&h.name, &h.value);
    }

    builder = apply_body(builder, &req.body, user_has_content_type)?;

    // ---- send + measure ----
    let start = Instant::now();
    let resp = builder.send().await?;
    let ttfb = start.elapsed();

    let status = resp.status();
    let status_text = status.canonical_reason().unwrap_or("").to_string();
    let final_url = resp.url().to_string();
    let http_version = format!("{:?}", resp.version());
    let headers: Vec<HeaderEntry> = resp
        .headers()
        .iter()
        .map(|(k, v)| HeaderEntry {
            name: k.to_string(),
            value: v.to_str().unwrap_or_default().to_string(),
            enabled: true,
        })
        .collect();

    let bytes = resp.bytes().await?;
    let total = start.elapsed();

    Ok(HttpResponse {
        status: status.as_u16(),
        status_text,
        headers,
        size_bytes: bytes.len() as u64,
        body_base64: base64::engine::general_purpose::STANDARD.encode(&bytes),
        timing: Timing {
            total_ms: ms(total),
            dns_ms: None,
            connect_ms: None,
            tls_ms: None,
            ttfb_ms: Some(ms(ttfb)),
            download_ms: Some(ms(total.saturating_sub(ttfb))),
        },
        final_url,
        http_version,
    })
}

fn build_client(opts: &RequestOptions) -> AppResult<Client> {
    let redirect = if opts.follow_redirects {
        reqwest::redirect::Policy::limited(opts.max_redirects)
    } else {
        reqwest::redirect::Policy::none()
    };
    Client::builder()
        .danger_accept_invalid_certs(!opts.verify_ssl)
        .redirect(redirect)
        .timeout(Duration::from_secs(opts.timeout_secs))
        .build()
        .map_err(Into::into)
}

fn build_url(raw: &str, query: &[KeyValue]) -> AppResult<Url> {
    let mut url =
        Url::parse(raw.trim()).map_err(|e| AppError::Other(format!("invalid URL: {e}")))?;
    {
        let mut pairs = url.query_pairs_mut();
        for p in query.iter().filter(|p| p.enabled && !p.key.is_empty()) {
            pairs.append_pair(&p.key, &p.value);
        }
    }
    // Drop a trailing "?" if no params were added.
    if url.query() == Some("") {
        url.set_query(None);
    }
    Ok(url)
}

fn apply_body(
    builder: RequestBuilder,
    body: &RequestBody,
    user_has_content_type: bool,
) -> AppResult<RequestBuilder> {
    let set_ct = |b: RequestBuilder, ct: &str| {
        if user_has_content_type {
            b
        } else {
            b.header(CONTENT_TYPE, ct)
        }
    };

    Ok(match body {
        RequestBody::None => builder,
        RequestBody::Json(content) => set_ct(builder, "application/json").body(content.clone()),
        RequestBody::Raw { content, .. } => builder.body(content.clone()),
        RequestBody::UrlEncoded(fields) => {
            let pairs: Vec<(&str, &str)> = fields
                .iter()
                .filter(|f| f.enabled && !f.key.is_empty())
                .map(|f| (f.key.as_str(), f.value.as_str()))
                .collect();
            builder.form(&pairs)
        }
        RequestBody::FormData(fields) => {
            let mut form = reqwest::multipart::Form::new();
            for f in fields.iter().filter(|f| f.enabled && !f.key.is_empty()) {
                if f.is_file {
                    let data = std::fs::read(&f.value)?;
                    let mut part = reqwest::multipart::Part::bytes(data)
                        .file_name(file_name_of(&f.value));
                    if let Some(ct) = &f.content_type {
                        part = part
                            .mime_str(ct)
                            .map_err(|e| AppError::Other(format!("bad content-type: {e}")))?;
                    }
                    form = form.part(f.key.clone(), part);
                } else {
                    form = form.text(f.key.clone(), f.value.clone());
                }
            }
            builder.multipart(form)
        }
        RequestBody::Binary { path } => {
            let data = std::fs::read(path)?;
            set_ct(builder, "application/octet-stream").body(data)
        }
        RequestBody::Graphql { query, variables } => {
            let vars: serde_json::Value = match variables {
                Some(v) if !v.trim().is_empty() => {
                    serde_json::from_str(v).map_err(|e| AppError::Other(format!("invalid GraphQL variables JSON: {e}")))?
                }
                _ => serde_json::Value::Null,
            };
            let payload = serde_json::json!({ "query": query, "variables": vars });
            set_ct(builder, "application/json").body(payload.to_string())
        }
    })
}

fn file_name_of(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("file")
        .to_string()
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kv(key: &str, value: &str, enabled: bool) -> KeyValue {
        KeyValue {
            key: key.into(),
            value: value.into(),
            enabled,
        }
    }

    #[test]
    fn build_url_merges_enabled_query_only() {
        let q = vec![kv("a", "1", true), kv("b", "2", false)];
        let url = build_url("https://example.com/path?x=0", &q).unwrap();
        let s = url.as_str();
        assert!(s.contains("x=0"));
        assert!(s.contains("a=1"));
        assert!(!s.contains("b=2"));
    }

    #[test]
    fn build_url_rejects_invalid() {
        assert!(build_url("not a url", &[]).is_err());
    }

    #[test]
    fn file_name_of_extracts_basename() {
        assert_eq!(file_name_of("/a/b/c.png"), "c.png");
        assert_eq!(file_name_of("plain"), "plain");
    }

    fn decode(resp: &HttpResponse) -> String {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&resp.body_base64)
            .unwrap();
        String::from_utf8_lossy(&bytes).into_owned()
    }

    /// End-to-end engine test against an in-process TCP server on 127.0.0.1.
    /// No DNS or external network, so it's deterministic in any sandbox while
    /// still exercising the real socket/parse/timing/base64 path.
    #[tokio::test]
    async fn sends_over_socket_and_parses_response() {
        use std::io::{Read, Write};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf); // consume request line/headers/body
            let body = b"{\"ok\":true}";
            let head = format!(
                "HTTP/1.1 201 Created\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(head.as_bytes()).unwrap();
            stream.write_all(body).unwrap();
            stream.flush().unwrap();
        });

        let req = HttpRequest {
            method: "POST".into(),
            url: format!("http://{addr}/echo"),
            headers: vec![HeaderEntry {
                name: "X-Test".into(),
                value: "1".into(),
                enabled: true,
            }],
            query: vec![kv("q", "1", true)],
            body: RequestBody::Json("{\"hello\":\"world\"}".into()),
            options: RequestOptions::default(),
        };
        let resp = send(req).await.unwrap();
        server.join().unwrap();

        assert_eq!(resp.status, 201);
        assert_eq!(resp.status_text, "Created");
        assert_eq!(decode(&resp), "{\"ok\":true}");
        assert_eq!(resp.size_bytes, 11);
        assert!(resp
            .headers
            .iter()
            .any(|h| h.name.eq_ignore_ascii_case("content-type")));
        assert!(resp.timing.ttfb_ms.is_some());
    }

    // Live network tests — run with `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore = "network"]
    async fn live_get_with_query() {
        let req = HttpRequest {
            method: "GET".into(),
            url: "https://httpbin.org/get".into(),
            headers: vec![],
            query: vec![kv("foo", "bar", true)],
            body: RequestBody::None,
            options: RequestOptions::default(),
        };
        let resp = send(req).await.unwrap();
        assert_eq!(resp.status, 200);
        assert!(resp.size_bytes > 0);
        assert!(resp.timing.ttfb_ms.is_some());
        assert!(decode(&resp).contains("\"foo\": \"bar\""));
    }

    #[tokio::test]
    #[ignore = "network"]
    async fn live_post_json() {
        let req = HttpRequest {
            method: "POST".into(),
            url: "https://httpbin.org/post".into(),
            headers: vec![],
            query: vec![],
            body: RequestBody::Json("{\"hello\":\"restman\"}".into()),
            options: RequestOptions::default(),
        };
        let resp = send(req).await.unwrap();
        assert_eq!(resp.status, 200);
        let body = decode(&resp);
        assert!(body.contains("restman"));
        assert!(body.contains("application/json"));
    }
}
