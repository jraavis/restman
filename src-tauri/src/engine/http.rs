//! HTTP engine: turns an `HttpRequest` spec into a reqwest call and captures
//! the response plus timing.
//!
//! Timing note: `total`/`ttfb`/`download` are measured directly around the
//! `send()`/`bytes()` calls. `dns`/`connect` are measured via two of
//! reqwest's public instrumentation hooks — `ClientBuilder::dns_resolver`
//! (see `TimingResolver`) and `ClientBuilder::connector_layer` (see
//! `TimingLayer`) — installed only on the one-shot `Client` `send()` builds
//! per request (see `build_timed_client`); `build_client`/`build_ws_client`
//! (used by SSE/WS, which don't report per-phase timing) skip them entirely.
//! `connector_layer`'s hook wraps the *whole* connect step reqwest performs
//! internally — for `https://` targets that's DNS resolution, TCP connect,
//! *and* the TLS handshake all nested inside one span, with no public seam
//! to split TCP from TLS — so `attribute_connect_ms` subtracts the
//! separately-measured `dns_ms` back out (the resolver call happens inside
//! the connector's span, not before it) and the result holds TCP connect +
//! TLS combined; `tls_ms` stays `None` rather than faking a further split.
//!
//! Waterfall convention, spelled out because the frontend's `TimingView`
//! renders these as if they were sequential non-overlapping bars (they are
//! not): `dns_ms`/`connect_ms` are each that phase's own duration (mutually
//! exclusive of each other, see `attribute_connect_ms`), but `ttfb_ms` is
//! measured from *before* the connection is even opened (`start` is set
//! right before `builder.send().await`, which performs DNS+connect+TLS+the
//! request write before the first response byte arrives) — so `ttfb_ms`
//! already *contains* `dns_ms`+`connect_ms`, it doesn't follow them. Only
//! `ttfb_ms + download_ms == total_ms` is a true additive identity; the
//! individual dns/connect/ttfb rows don't sum to `total_ms`. This matches
//! curl's `-w` cumulative timing vars more than Chrome DevTools' strictly
//! non-overlapping waterfall — revisit if the frontend wants the latter
//! (which would mean re-deriving a request-send-to-ttfb-only figure).

use crate::error::{AppError, AppResult};
use crate::model::auth::{ApiKeyLocation, AuthConfig};
use crate::model::http::*;
use base64::Engine as _;
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use reqwest::header::CONTENT_TYPE;
use reqwest::{Client, Method, RequestBuilder, Url};
use reqwest_cookie_store::CookieStoreMutex;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant, SystemTime};
use tower::{Layer, Service};

/// Where `TimingResolver`/`TimingLayer` (installed only by
/// `build_timed_client`) deposit their measurements for `send()` to read
/// back once the response has arrived. One sink per request — a fresh
/// `Client` is built for every `send()` call (see `build_client_inner`), so
/// there's no cross-request state to worry about.
#[derive(Default)]
struct PhaseTimingSink {
    dns_ms: Mutex<Option<f64>>,
    connect_ms: Mutex<Option<f64>>,
}

/// Wraps DNS resolution to time it. Delegates to `tokio::net::lookup_host`
/// (the same getaddrinfo-backed resolution hyper's own default resolver
/// uses) rather than special resolver logic — this hook exists purely to
/// time the lookup, not to change how it resolves. Naturally never invoked
/// (and `dns_ms` naturally stays `None`) when the host is already an IP
/// literal, since reqwest's connector skips resolution in that case.
struct TimingResolver {
    sink: Arc<PhaseTimingSink>,
}

impl Resolve for TimingResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let sink = self.sink.clone();
        let host = name.as_str().to_string();
        Box::pin(async move {
            let start = Instant::now();
            let addrs = tokio::net::lookup_host((host, 0))
                .await
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
            *sink.dns_ms.lock().unwrap() = Some(ms(start.elapsed()));
            let iter: Addrs = Box::new(addrs);
            Ok(iter)
        })
    }
}

/// A `tower::Layer` generic over the wrapped service — mirrors how
/// `tower::timeout::TimeoutLayer` etc. are usable with `ClientBuilder::connector_layer`
/// in reqwest's own docs, despite that method's bound naming reqwest-private
/// types (`BoxedConnectorService`/`Unnameable`/`Conn`): a generic-over-`S`
/// `Layer`/`Service` impl never has to name them — reqwest monomorphizes
/// with its own private concrete types internally.
#[derive(Clone)]
struct TimingLayer {
    sink: Arc<PhaseTimingSink>,
}

impl<S> Layer<S> for TimingLayer {
    type Service = TimingService<S>;
    fn layer(&self, inner: S) -> Self::Service {
        TimingService { inner, sink: self.sink.clone() }
    }
}

#[derive(Clone)]
struct TimingService<S> {
    inner: S,
    sink: Arc<PhaseTimingSink>,
}

impl<S, Req> Service<Req> for TimingService<S>
where
    S: Service<Req> + Send,
    S::Future: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<S::Response, S::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Req) -> Self::Future {
        let start = Instant::now();
        let fut = self.inner.call(req);
        let sink = self.sink.clone();
        Box::pin(async move {
            let result = fut.await;
            *sink.connect_ms.lock().unwrap() = Some(ms(start.elapsed()));
            result
        })
    }
}

/// `TimingLayer` wraps the *whole* connector — which itself calls the
/// resolver internally before dialing — so its raw span strictly *contains*
/// whatever `TimingResolver` measured, not sits after it. Reporting both
/// numbers as-is would double-count DNS time on the connect row (e.g. "DNS
/// 5ms / connect 15ms" when the real TCP+TLS work was only 10ms). Subtracting
/// gives a connect figure that means "TCP connect + TLS handshake, DNS
/// excluded" — consistent with `tls_ms` staying `None` rather than a
/// fabricated split. `dns_ms <= raw_connect_ms` always holds since one span
/// contains the other; `.max(0.0)` is defensive, not expected to trigger.
fn attribute_connect_ms(dns_ms: Option<f64>, raw_connect_ms: Option<f64>) -> Option<f64> {
    raw_connect_ms.map(|raw| match dns_ms {
        Some(dns) => (raw - dns).max(0.0),
        None => raw,
    })
}

pub async fn send(
    req: HttpRequest,
    cookie_jar: Option<Arc<CookieStoreMutex>>,
    transport: Option<&TransportOverrides>,
) -> AppResult<HttpResponse> {
    let (client, timing_sink) = build_timed_client(&req.options, cookie_jar, transport)?;

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

    let dns_ms = *timing_sink.dns_ms.lock().unwrap();
    let connect_ms = attribute_connect_ms(dns_ms, *timing_sink.connect_ms.lock().unwrap());

    Ok(HttpResponse {
        status: status.as_u16(),
        status_text,
        headers,
        size_bytes: bytes.len() as u64,
        body_base64: base64::engine::general_purpose::STANDARD.encode(&bytes),
        timing: Timing {
            total_ms: ms(total),
            dns_ms,
            connect_ms,
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
    build_client_inner(opts, cookie_jar, transport, false, None)
}

/// Same as `build_client`, but also installs `TimingResolver`/`TimingLayer`
/// and hands back the sink they write into — used only by `send()`, which is
/// the only caller that reports per-phase timing back to the frontend.
fn build_timed_client(
    opts: &RequestOptions,
    cookie_jar: Option<Arc<CookieStoreMutex>>,
    transport: Option<&TransportOverrides>,
) -> AppResult<(Client, Arc<PhaseTimingSink>)> {
    let sink = Arc::new(PhaseTimingSink::default());
    let client = build_client_inner(opts, cookie_jar, transport, false, Some(sink.clone()))?;
    Ok((client, sink))
}

/// Same as `build_client`, pinned to HTTP/1.1. The WebSocket handshake
/// (`engine::ws::connect`) speaks the RFC 6455 `Upgrade:` header dance, which
/// HTTP/2 has no concept of (RFC 7540 §8.1.2.2 has h2 strip `Upgrade` outright)
/// — if ALPN picked h2 on a `wss://` connection the server would just answer
/// the GET normally and the handshake would never see its 101.
pub(crate) fn build_ws_client(
    opts: &RequestOptions,
    transport: Option<&TransportOverrides>,
) -> AppResult<Client> {
    build_client_inner(opts, None, transport, true, None)
}

fn build_client_inner(
    opts: &RequestOptions,
    cookie_jar: Option<Arc<CookieStoreMutex>>,
    transport: Option<&TransportOverrides>,
    http1_only: bool,
    timing: Option<Arc<PhaseTimingSink>>,
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
    if http1_only {
        builder = builder.http1_only();
    }
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
    if let Some(sink) = timing {
        builder = builder
            .dns_resolver(Arc::new(TimingResolver { sink: sink.clone() }))
            .connector_layer(TimingLayer { sink });
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
    /// Same client certificate as `client_identity`, kept as raw PEM
    /// alongside the opaque `reqwest::Identity` so gRPC's hand-rolled rustls
    /// client (which has no path to a `reqwest::Identity`) can build its own
    /// `rustls::ClientConfig` from it. reqwest/ws/sse keep consuming
    /// `client_identity` unchanged.
    pub client_cert_pem: Option<ClientCertPem>,
}

#[derive(Debug, Clone)]
pub struct ClientCertPem {
    pub cert_pem: String,
    pub key_pem: String,
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
        RequestBody::Graphql { query, variables, operation_name } => {
            let payload = graphql_payload(query, variables, operation_name)?;
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
        RequestBody::Graphql { query, variables, operation_name } => {
            graphql_payload(query, variables, operation_name)?.to_string().into_bytes()
        }
    })
}

/// Builds the standard `{ query, variables, operationName }` GraphQL POST envelope.
/// `operationName` is omitted from the JSON entirely when absent (not sent as
/// `null`) since some servers reject an explicit `null` for a field they expect
/// to be either a string or missing.
fn graphql_payload(
    query: &str,
    variables: &Option<String>,
    operation_name: &Option<String>,
) -> AppResult<serde_json::Value> {
    let vars: serde_json::Value = match variables {
        Some(v) if !v.trim().is_empty() => serde_json::from_str(v)
            .map_err(|e| AppError::Other(format!("invalid GraphQL variables JSON: {e}")))?,
        _ => serde_json::Value::Null,
    };
    let mut payload = serde_json::json!({ "query": query, "variables": vars });
    if let Some(name) = operation_name {
        if !name.trim().is_empty() {
            payload["operationName"] = serde_json::Value::String(name.clone());
        }
    }
    Ok(payload)
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

    // `attribute_connect_ms` is the fix for a real bug caught in review:
    // `TimingLayer`'s raw span *contains* `TimingResolver`'s DNS span (the
    // connector calls the resolver internally before dialing), so reporting
    // both numbers unsubtracted would double-count DNS time on the connect
    // row. These pin the containment-subtraction contract directly, since
    // racing a real slow DNS lookup against a real TCP handshake in a test
    // would be flaky and prove the same thing far less precisely.
    #[test]
    fn attribute_connect_ms_subtracts_dns_time_from_the_containing_connect_span() {
        // Connector span was 20ms total; 5ms of that was the nested DNS
        // lookup — the TCP+TLS-only portion is the remaining 15ms.
        assert_eq!(attribute_connect_ms(Some(5.0), Some(20.0)), Some(15.0));
    }

    #[test]
    fn attribute_connect_ms_passes_through_when_dns_was_not_measured() {
        // IP-literal target: the resolver never ran, so there's nothing
        // nested to subtract — the raw connect span is already TCP+TLS only.
        assert_eq!(attribute_connect_ms(None, Some(20.0)), Some(20.0));
    }

    #[test]
    fn attribute_connect_ms_stays_none_when_the_connector_layer_never_ran() {
        // e.g. a pooled/reused connection — no connect span to attribute at all.
        assert_eq!(attribute_connect_ms(Some(5.0), None), None);
    }

    #[test]
    fn attribute_connect_ms_clamps_instead_of_going_negative() {
        // Containment guarantees dns <= connect in practice, but the clamp
        // means a clock-source anomaly reports 0ms rather than a negative
        // duration, which would be a more confusing thing to render.
        assert_eq!(attribute_connect_ms(Some(20.0), Some(5.0)), Some(0.0));
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
        // Real TCP handshake against the loopback listener above — proves
        // `TimingLayer` actually measures something, not just that it compiles.
        assert!(resp.timing.connect_ms.is_some());
        // `127.0.0.1` is an IP literal — reqwest's connector never calls the
        // resolver for it, so `TimingResolver` never runs and `dns_ms` must
        // stay `None` rather than reporting a fake zero.
        assert!(resp.timing.dns_ms.is_none());
    }

    /// Same loopback server as `sends_over_socket_and_parses_response`, but
    /// addressed by hostname so the connector actually calls `TimingResolver`
    /// — proves `dns_ms` is populated on the one path where it's expected to be.
    #[tokio::test]
    async fn dns_ms_is_populated_when_the_host_is_a_name_not_an_ip_literal() {
        use std::io::{Read, Write};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf);
            let body = b"ok";
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(head.as_bytes()).unwrap();
            stream.write_all(body).unwrap();
            stream.flush().unwrap();
        });

        let req = HttpRequest {
            method: "GET".into(),
            url: format!("http://localhost:{port}/"),
            headers: vec![],
            query: vec![],
            body: RequestBody::None,
            options: RequestOptions::default(),
            auth: AuthConfig::None,
        };
        let resp = send(req, None, None).await.unwrap();
        server.join().unwrap();

        assert_eq!(resp.status, 200);
        assert!(resp.timing.dns_ms.is_some(), "expected a real DNS lookup for a hostname target");
        assert!(resp.timing.connect_ms.is_some());
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

    #[test]
    fn graphql_payload_omits_operation_name_when_absent() {
        let payload = graphql_payload("{ pets { id } }", &None, &None).unwrap();
        assert_eq!(payload, serde_json::json!({"query": "{ pets { id } }", "variables": null}));
    }

    #[test]
    fn graphql_payload_omits_operation_name_when_blank() {
        let payload = graphql_payload("{ pets { id } }", &None, &Some("   ".into())).unwrap();
        assert!(payload.get("operationName").is_none());
    }

    #[test]
    fn graphql_payload_includes_operation_name_and_parsed_variables() {
        let payload =
            graphql_payload("query Pets($id: ID) { pet(id: $id) { name } }", &Some("{\"id\":\"1\"}".into()), &Some("Pets".into()))
                .unwrap();
        assert_eq!(
            payload,
            serde_json::json!({
                "query": "query Pets($id: ID) { pet(id: $id) { name } }",
                "variables": {"id": "1"},
                "operationName": "Pets",
            })
        );
    }
}
