//! HTTP engine: turns an `HttpRequest` spec into a reqwest call and captures
//! the response plus timing.
//!
//! Timing note: this first cut measures total / TTFB / download from the
//! client side. Per-phase DNS/TCP/TLS timing needs an instrumented connector
//! (reqwest exposes only total) and is a tracked Phase-1 follow-up; those
//! `Timing` fields stay `None` until then.

use crate::error::{AppError, AppResult};
use crate::model::auth::{ApiKeyLocation, AuthConfig};
use crate::model::http::*;
use base64::Engine as _;
use reqwest::header::CONTENT_TYPE;
use reqwest::{Client, Method, RequestBuilder, Url};
use reqwest_cookie_store::CookieStoreMutex;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

pub async fn send(
    req: HttpRequest,
    cookie_jar: Option<Arc<CookieStoreMutex>>,
    transport: Option<&TransportOverrides>,
) -> AppResult<HttpResponse> {
    let client = build_client(&req.options, cookie_jar, transport)?;

    let method = Method::from_bytes(req.method.trim().as_bytes())
        .map_err(|_| AppError::Other(format!("invalid HTTP method: {}", req.method)))?;

    let mut url = build_url(&req.url, &req.query)?;

    // Collected as an owned, mutable list (rather than chained straight onto
    // the builder) because auth needs to inspect and extend it — adding an
    // `Authorization` header, an API-key query param, or a full SigV4
    // signature — before the request is actually built.
    let mut user_has_content_type = false;
    let mut header_pairs: Vec<(String, String)> = Vec::new();
    for h in req.headers.iter().filter(|h| h.enabled && !h.name.trim().is_empty()) {
        if h.name.eq_ignore_ascii_case("content-type") {
            user_has_content_type = true;
        }
        header_pairs.push((h.name.clone(), h.value.clone()));
    }

    apply_auth(&method, &mut url, &mut header_pairs, &req.body, &req.auth)?;

    let mut builder = client.request(method, url);
    for (name, value) in &header_pairs {
        builder = builder.header(name, value);
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

pub(crate) fn build_client(
    opts: &RequestOptions,
    cookie_jar: Option<Arc<CookieStoreMutex>>,
    transport: Option<&TransportOverrides>,
) -> AppResult<Client> {
    let redirect = if opts.follow_redirects {
        reqwest::redirect::Policy::limited(opts.max_redirects)
    } else {
        reqwest::redirect::Policy::none()
    };
    let mut builder = Client::builder()
        .danger_accept_invalid_certs(!opts.verify_ssl)
        .redirect(redirect)
        .timeout(Duration::from_secs(opts.timeout_secs));
    if opts.send_cookies {
        if let Some(jar) = cookie_jar {
            builder = builder.cookie_provider(jar);
        }
    }
    if let Some(t) = transport {
        if let Some(proxy_url) = t.proxy_url.as_deref() {
            if !proxy_url.trim().is_empty() {
                let mut proxy = reqwest::Proxy::all(proxy_url)
                    .map_err(|e| AppError::Other(format!("invalid proxy \"{proxy_url}\": {e}")))?;
                if let Some(bypass) = t.proxy_bypass.as_deref() {
                    if !bypass.trim().is_empty() {
                        proxy = proxy.no_proxy(reqwest::NoProxy::from_string(bypass));
                    }
                }
                builder = builder.proxy(proxy);
            }
        }
        if let Some(identity) = t.client_identity.as_ref() {
            builder = builder.identity(identity.clone());
        }
    }
    builder.build().map_err(Into::into)
}

/// Send-time transport overrides derived from a workspace's
/// `WorkspaceSettings`, after secret hydration (PEM bytes, passphrase) has
/// happened on the Rust side. Kept separate from `WorkspaceSettings` itself
/// so (a) the engine never depends on DB/keychain types and stays
/// unit-testable with pure inputs, and (b) the masked `WorkspaceSettings`
/// columns can be serialized safely across IPC.
#[derive(Debug, Clone, Default)]
pub struct TransportOverrides {
    pub proxy_url: Option<String>,
    pub proxy_bypass: Option<String>,
    pub client_identity: Option<reqwest::Identity>,
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

/// Applies a resolved `AuthConfig` to the outgoing request: Bearer/Basic add
/// an `Authorization` header, `ApiKey` adds either a header or a query param,
/// `AwsSigV4` signs and appends whatever headers `sign_headers` returns.
/// `OAuth2` reaching here is a programming error — `commands::http::send_request`
/// must collapse it to a bearer token first, since fetching/refreshing a
/// token needs DB + network access this DB-free engine layer doesn't have.
fn apply_auth(
    method: &Method,
    url: &mut Url,
    header_pairs: &mut Vec<(String, String)>,
    body: &RequestBody,
    auth: &AuthConfig,
) -> AppResult<()> {
    match auth {
        AuthConfig::None => {}
        AuthConfig::Bearer { token } => {
            header_pairs.push(("Authorization".to_string(), format!("Bearer {token}")));
        }
        AuthConfig::Basic { username, password } => {
            let encoded = base64::engine::general_purpose::STANDARD.encode(format!("{username}:{password}"));
            header_pairs.push(("Authorization".to_string(), format!("Basic {encoded}")));
        }
        AuthConfig::ApiKey { key, value, location } => match location {
            ApiKeyLocation::Header => header_pairs.push((key.clone(), value.clone())),
            ApiKeyLocation::Query => {
                url.query_pairs_mut().append_pair(key, value);
            }
        },
        AuthConfig::OAuth2(_) => {
            return Err(AppError::Other(
                "OAuth2 auth must be resolved to a bearer token before reaching the HTTP engine".into(),
            ));
        }
        AuthConfig::AwsSigV4(config) => {
            // `host` must be in the *signed* header set for the canonical
            // request to validate, but doesn't need to land in the real
            // outgoing headers — reqwest/hyper derive it from the URL the
            // same way. Signed separately so it doesn't pollute `header_pairs`.
            let host = url
                .host_str()
                .ok_or_else(|| AppError::Other("AWS SigV4: URL has no host".to_string()))?
                .to_string();
            let mut sign_input = header_pairs.clone();
            sign_input.push(("host".to_string(), host));
            let body_bytes = body_bytes_for_signing(body)?;
            let signed = crate::auth::aws_sigv4::sign_headers(
                config,
                method.as_str(),
                url.as_str(),
                &sign_input,
                Some(body_bytes.as_slice()),
                SystemTime::now(),
            )?;
            header_pairs.extend(signed);
        }
    }
    Ok(())
}

/// Best-effort byte reproduction of `body`, for SigV4's canonical request
/// (which hashes the body) — must match what `apply_body` actually sends.
/// `FormData` has no accessible static byte representation before reqwest
/// streams it, so it signs as empty: AWS SigV4 + multipart isn't a realistic
/// combination in practice (AWS REST/JSON APIs are the common case).
/// `UrlEncoded` reuses `Url::query_pairs_mut` (same `form_urlencoded` crate
/// reqwest's own `.form()` uses internally) instead of hand-rolling
/// percent-encoding, so the signed bytes are guaranteed to match the sent ones.
fn body_bytes_for_signing(body: &RequestBody) -> AppResult<Vec<u8>> {
    Ok(match body {
        RequestBody::None => Vec::new(),
        RequestBody::Json(content) => content.clone().into_bytes(),
        RequestBody::Raw { content, .. } => content.clone().into_bytes(),
        RequestBody::UrlEncoded(fields) => {
            let pairs: Vec<(&str, &str)> = fields
                .iter()
                .filter(|f| f.enabled && !f.key.is_empty())
                .map(|f| (f.key.as_str(), f.value.as_str()))
                .collect();
            let mut dummy = Url::parse("http://x").unwrap();
            dummy.query_pairs_mut().clear().extend_pairs(&pairs);
            dummy.query().unwrap_or("").as_bytes().to_vec()
        }
        RequestBody::FormData(_) => Vec::new(),
        RequestBody::Binary { path } => std::fs::read(path)?,
        RequestBody::Graphql { query, variables } => {
            let vars: serde_json::Value = match variables {
                Some(v) if !v.trim().is_empty() => serde_json::from_str(v)
                    .map_err(|e| AppError::Other(format!("invalid GraphQL variables JSON: {e}")))?,
                _ => serde_json::Value::Null,
            };
            serde_json::json!({ "query": query, "variables": vars }).to_string().into_bytes()
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
            auth: AuthConfig::None,
        };
        let resp = send(req, None, None).await.unwrap();
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

    fn no_headers() -> Vec<(String, String)> {
        Vec::new()
    }

    #[test]
    fn apply_auth_bearer_adds_authorization_header() {
        let mut url = Url::parse("https://example.com").unwrap();
        let mut headers = no_headers();
        apply_auth(&Method::GET, &mut url, &mut headers, &RequestBody::None, &AuthConfig::Bearer { token: "tok123".into() }).unwrap();
        assert_eq!(headers, vec![("Authorization".to_string(), "Bearer tok123".to_string())]);
    }

    #[test]
    fn apply_auth_basic_encodes_username_password() {
        let mut url = Url::parse("https://example.com").unwrap();
        let mut headers = no_headers();
        apply_auth(
            &Method::GET,
            &mut url,
            &mut headers,
            &RequestBody::None,
            &AuthConfig::Basic { username: "user".into(), password: "pass".into() },
        )
        .unwrap();
        // base64("user:pass") — known fixed value, asserted literally so a
        // future encoding regression (e.g. wrong charset table) shows up here.
        assert_eq!(headers, vec![("Authorization".to_string(), "Basic dXNlcjpwYXNz".to_string())]);
    }

    #[test]
    fn apply_auth_api_key_header_only_adds_header() {
        let mut url = Url::parse("https://example.com").unwrap();
        let mut headers = no_headers();
        apply_auth(
            &Method::GET,
            &mut url,
            &mut headers,
            &RequestBody::None,
            &AuthConfig::ApiKey { key: "X-Api-Key".into(), value: "secret1".into(), location: ApiKeyLocation::Header },
        )
        .unwrap();
        assert_eq!(headers, vec![("X-Api-Key".to_string(), "secret1".to_string())]);
        assert!(!url.as_str().contains("secret1"));
    }

    #[test]
    fn apply_auth_api_key_query_only_adds_param() {
        let mut url = Url::parse("https://example.com").unwrap();
        let mut headers = no_headers();
        apply_auth(
            &Method::GET,
            &mut url,
            &mut headers,
            &RequestBody::None,
            &AuthConfig::ApiKey { key: "api_key".into(), value: "secret2".into(), location: ApiKeyLocation::Query },
        )
        .unwrap();
        assert!(headers.is_empty());
        assert!(url.as_str().contains("api_key=secret2"));
    }

    #[test]
    fn apply_auth_oauth2_is_rejected_as_programming_error() {
        let mut url = Url::parse("https://example.com").unwrap();
        let mut headers = no_headers();
        let result = apply_auth(&Method::GET, &mut url, &mut headers, &RequestBody::None, &AuthConfig::OAuth2(Default::default()));
        assert!(result.is_err());
    }

    #[test]
    fn apply_auth_aws_sigv4_adds_signature_without_leaking_host_header() {
        use crate::model::auth::AwsSigV4Config;

        let mut url = Url::parse("https://example.amazonaws.com/").unwrap();
        let mut headers = no_headers();
        let config = AwsSigV4Config {
            access_key: "AKIDEXAMPLE".into(),
            secret_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY".into(),
            region: "us-east-1".into(),
            service: "service".into(),
            session_token: String::new(),
        };
        apply_auth(&Method::GET, &mut url, &mut headers, &RequestBody::None, &AuthConfig::AwsSigV4(config)).unwrap();

        let auth = headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("authorization"));
        assert!(auth.is_some_and(|(_, v)| v.starts_with("AWS4-HMAC-SHA256")));
        assert!(headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("x-amz-date")));
        // `host` is signed (see `sign_input` in `apply_auth`) but must not
        // land in the real outgoing headers — reqwest derives it from the URL.
        assert!(!headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("host")));
    }

    // Live network tests — run with `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore = "network"]
    async fn live_bearer_auth_against_httpbin() {
        let req = HttpRequest {
            method: "GET".into(),
            url: "https://httpbin.org/bearer".into(),
            headers: vec![],
            query: vec![],
            body: RequestBody::None,
            options: RequestOptions::default(),
            auth: AuthConfig::Bearer { token: "test-token-123".into() },
        };
        let resp = send(req, None, None).await.unwrap();
        assert_eq!(resp.status, 200);
        let body = decode(&resp);
        assert!(body.contains("\"authenticated\": true"));
        assert!(body.contains("test-token-123"));
    }

    #[tokio::test]
    #[ignore = "network"]
    async fn live_basic_auth_against_httpbin() {
        let req = HttpRequest {
            method: "GET".into(),
            url: "https://httpbin.org/basic-auth/restman/s3cr3t".into(),
            headers: vec![],
            query: vec![],
            body: RequestBody::None,
            options: RequestOptions::default(),
            auth: AuthConfig::Basic { username: "restman".into(), password: "s3cr3t".into() },
        };
        let resp = send(req, None, None).await.unwrap();
        assert_eq!(resp.status, 200);
        assert!(decode(&resp).contains("\"authenticated\": true"));
    }

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
            auth: AuthConfig::None,
        };
        let resp = send(req, None, None).await.unwrap();
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
            auth: AuthConfig::None,
        };
        let resp = send(req, None, None).await.unwrap();
        assert_eq!(resp.status, 200);
        let body = decode(&resp);
        assert!(body.contains("restman"));
        assert!(body.contains("application/json"));
    }
}
