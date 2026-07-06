//! gRPC transport over HTTP/2 + TLS (#25 fills in the production `connect()`;
//! the offline TLS-stack validation test landed in #23).
//!
//! Why a hand-rolled h2 client at all: reqwest (0.12) doesn't expose raw
//! HTTP/2 trailers, which gRPC carries `grpc-status` in. So gRPC speaks h2
//! directly via the `h2` crate, over a `tokio-rustls` TLS session with ALPN
//! negotiating `h2`. The TLS stack is **rustls** (not native-tls) — chosen in
//! #23 for clean ALPN and to keep one TLS implementation in the binary (after
//! reqwest was tightened to `rustls-tls-native-roots`), and `aws-lc-rs` is the
//! crypto provider (rustls 0.23 default; already a transitive dep of the
//! tightened reqwest, so this doesn't add a new TLS impl).
//!
//! This module is a building block, not the full unary-RPC flow: it opens the
//! h2 connection, sends gRPC-required headers, and exposes a thin
//! send-frame/recv-frame/recv-trailers API. Driving a complete request/
//! response exchange end-to-end is a later task (#28); this just needs to be
//! usable by it, the same way `engine::ws::connect` separates the handshake
//! from whatever later drives the socket.

use std::collections::VecDeque;
use std::sync::Arc;

use base64::Engine as _;
use bytes::Bytes;
use h2::client::{ResponseFuture, SendRequest};
use h2::SendStream;
use http::{HeaderMap, Request, StatusCode};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use rustls::{ClientConfig, RootCertStore};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

use crate::engine::http::ClientCertPem;
use crate::error::AppError;

use super::framing::{frame, FrameUnframer};

/// Parsed `grpc://`/`grpcs://` target. gRPC has no HTTP/1.1 fallback (it
/// requires real HTTP/2 trailers for `grpc-status`), so unlike
/// `engine::ws::connect` there's no protocol-negotiation branch here — just
/// the plaintext-vs-TLS split on the scheme.
struct Target {
    tls: bool,
    host: String,
    port: u16,
}

/// Parses `grpc://host[:port]` (plaintext, default port 80) or
/// `grpcs://host[:port]` (TLS via ALPN `h2`, default port 443).
fn parse_target(url: &str) -> Result<Target, AppError> {
    let (tls, rest) = if let Some(rest) = url.strip_prefix("grpcs://") {
        (true, rest)
    } else if let Some(rest) = url.strip_prefix("grpc://") {
        (false, rest)
    } else {
        return Err(AppError::Other(format!(
            "unsupported gRPC URL scheme: {url} (expected grpc:// or grpcs://)"
        )));
    };
    // Strip any trailing path/query — the target is host[:port] only; the
    // actual RPC path is supplied separately when a request is built.
    let authority = rest.split(['/', '?']).next().unwrap_or(rest);
    if authority.is_empty() {
        return Err(AppError::Other(format!("missing host in gRPC URL: {url}")));
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => {
            let port = p
                .parse::<u16>()
                .map_err(|_| AppError::Other(format!("invalid port in gRPC URL: {url}")))?;
            (h.to_string(), port)
        }
        None => (authority.to_string(), if tls { 443 } else { 80 }),
    };
    if host.is_empty() {
        return Err(AppError::Other(format!("missing host in gRPC URL: {url}")));
    }
    Ok(Target { tls, host, port })
}

/// Builds a rustls client config trusting the OS/Mozilla root store
/// (`webpki-roots`, mirroring the `rustls-tls-native-roots` precedent reqwest
/// already uses elsewhere in this codebase) with ALPN pinned to `h2` — gRPC
/// never negotiates HTTP/1.1. When `client_cert` is set, the session also
/// presents that certificate for mTLS — the rustls-side counterpart of
/// `TransportOverrides.client_identity`, which reqwest/ws/sse consume via
/// their own opaque `reqwest::Identity` instead.
fn client_config(client_cert: Option<&ClientCertPem>) -> Result<Arc<ClientConfig>, AppError> {
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let builder = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("rustls: safe protocol versions")
        .with_root_certificates(roots);
    let mut cfg = match client_cert {
        Some(pem) => {
            let (certs, key) = parse_client_identity(pem)?;
            builder
                .with_client_auth_cert(certs, key)
                .map_err(|e| AppError::Other(format!("invalid gRPC client certificate: {e}")))?
        }
        None => builder.with_no_client_auth(),
    };
    cfg.alpn_protocols = vec![b"h2".to_vec()];
    Ok(Arc::new(cfg))
}

/// Parses the raw cert/key PEM `resolve_transport` hydrated from the
/// keychain/disk into rustls' DER types. Mirrors `reqwest::Identity::from_pem`'s
/// scope exactly (same source bytes, same "encrypted keys unsupported"
/// limitation) rather than adding passphrase-decryption logic this codebase
/// doesn't have anywhere else yet: `rustls_pemfile::private_key` returns
/// `Ok(None)` for a key it can't parse as a bare PKCS#8/PKCS#1/SEC1 key
/// (including an encrypted one), which is turned into a clean, explicit
/// `AppError` here rather than an opaque downstream TLS failure.
fn parse_client_identity(
    pem: &ClientCertPem,
) -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>), AppError> {
    let certs = rustls_pemfile::certs(&mut pem.cert_pem.as_bytes())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| AppError::Other(format!("invalid client certificate PEM: {e}")))?;
    if certs.is_empty() {
        return Err(AppError::Other(
            "client certificate PEM contains no certificates".into(),
        ));
    }
    let key = rustls_pemfile::private_key(&mut pem.key_pem.as_bytes())
        .map_err(|e| AppError::Other(format!("invalid client certificate private key: {e}")))?
        .ok_or_else(|| {
            AppError::Other(
                "client certificate private key is not in a supported format (encrypted/passphrase-protected keys are not supported)".into(),
            )
        })?;
    Ok((certs, key))
}

/// Dials `proxy_url` (an `http://` proxy — TLS-to-proxy is not supported, a
/// clean upfront error rather than silently skipping the tunnel) and issues
/// an HTTP `CONNECT` for `target_authority`, returning the raw TCP stream once
/// the proxy answers `200`. The returned stream is exactly as if it were
/// dialed directly at the target — the caller layers TLS/h2 on top the same
/// way either way. Proxy credentials in the URL's userinfo (if any) are sent
/// as `Proxy-Authorization: Basic`, mirroring what `reqwest::Proxy::all`
/// does with the same URL shape elsewhere in this codebase.
async fn connect_through_proxy(proxy_url: &str, target_authority: &str) -> Result<TcpStream, AppError> {
    let proxy = reqwest::Url::parse(proxy_url)
        .map_err(|e| AppError::Other(format!("invalid gRPC proxy URL \"{proxy_url}\": {e}")))?;
    if proxy.scheme() != "http" {
        return Err(AppError::Other(format!(
            "gRPC proxy support only handles http:// proxies (TLS-to-proxy is not supported); got scheme \"{}\"",
            proxy.scheme()
        )));
    }
    let proxy_host = proxy
        .host_str()
        .ok_or_else(|| AppError::Other(format!("missing host in gRPC proxy URL: {proxy_url}")))?;
    let proxy_port = proxy.port_or_known_default().unwrap_or(80);
    let proxy_authority = format!("{proxy_host}:{proxy_port}");

    let mut stream = TcpStream::connect(&proxy_authority)
        .await
        .map_err(|e| AppError::Other(format!("gRPC proxy TCP connect to {proxy_authority} failed: {e}")))?;
    stream.set_nodelay(true).ok();

    let mut request = format!("CONNECT {target_authority} HTTP/1.1\r\nHost: {target_authority}\r\n");
    if !proxy.username().is_empty() {
        let creds = base64::engine::general_purpose::STANDARD
            .encode(format!("{}:{}", proxy.username(), proxy.password().unwrap_or("")));
        request.push_str(&format!("Proxy-Authorization: Basic {creds}\r\n"));
    }
    request.push_str("\r\n");

    stream
        .write_all(request.as_bytes())
        .await
        .map_err(|e| AppError::Other(format!("failed to send CONNECT to gRPC proxy: {e}")))?;

    // Read the proxy's response one byte at a time until the header
    // terminator, so we never read past the header block into bytes that
    // belong to the tunneled TLS/h2 session on the other side of `200`.
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        let n = stream
            .read(&mut byte)
            .await
            .map_err(|e| AppError::Other(format!("failed reading CONNECT response from gRPC proxy: {e}")))?;
        if n == 0 {
            return Err(AppError::Other(
                "gRPC proxy closed the connection before completing CONNECT".into(),
            ));
        }
        buf.push(byte[0]);
        if buf.len() >= 4 && buf[buf.len() - 4..] == *b"\r\n\r\n" {
            break;
        }
        if buf.len() > 8192 {
            return Err(AppError::Other("gRPC proxy CONNECT response headers too large".into()));
        }
    }
    let response = String::from_utf8_lossy(&buf);
    let status_line = response.lines().next().unwrap_or("");
    let status_code = status_line.split_whitespace().nth(1).unwrap_or("");
    if status_code != "200" {
        return Err(AppError::Other(format!(
            "gRPC proxy CONNECT to {target_authority} failed: {status_line}"
        )));
    }
    Ok(stream)
}

/// A connected gRPC transport: an h2 client handle plus the connection driver
/// already spawned onto the runtime. Mirrors `engine::ws::connect`'s
/// connect-then-let-the-caller-drive shape — `connect()` only stands up the
/// session; sending a request and reading frames/trailers back are separate
/// calls made later by the unary/streaming RPC drive loop (#28).
pub(crate) struct GrpcTransport {
    send_request: SendRequest<Bytes>,
    authority: String,
    tls: bool,
}

/// One in-flight RPC's send/recv halves, returned by `GrpcTransport::send`.
///
/// `send_frame` writes a length-prefixed gRPC message; `recv_frame` decodes
/// the next one off the response body (buffering partial HTTP/2 DATA frames
/// via `FrameUnframer`, since h2 data frames don't align to gRPC message
/// boundaries); `recv_trailers` reads the HTTP/2 trailers carrying
/// `grpc-status`/`grpc-message` — the entire reason this client is
/// hand-rolled instead of going through reqwest (which can't see trailers).
///
/// The response headers are *not* awaited until the first `recv_frame`/
/// `http_status`/`recv_trailers` call: a unary (or client-streaming) gRPC
/// server reads the whole request body before sending response headers, so
/// awaiting the response inside `send()` itself — before the caller has had
/// a chance to call `send_frame` — would deadlock both sides.
pub(crate) struct GrpcStream {
    send_stream: SendStream<Bytes>,
    response: ResponseState,
    pending: VecDeque<Vec<u8>>,
    unframer: FrameUnframer,
}

/// Lazily-resolved response half of a `GrpcStream`. Starts holding the raw h2
/// future from `send_request`; the first call that needs headers/body/status
/// awaits it *in place* (never `take()`n out — see `resolve_response`'s doc
/// comment for why that distinction matters for `tokio::select!`
/// cancellation safety) and caches the resulting status + headers + body
/// stream.
enum ResponseState {
    Pending(ResponseFuture),
    Ready {
        status: StatusCode,
        headers: HeaderMap,
        body: h2::RecvStream,
    },
}

impl GrpcTransport {
    /// Opens a `grpc://`/`grpcs://` connection: TCP connect, optional TLS
    /// handshake with ALPN `h2`, then the HTTP/2 client preface. The
    /// connection driver (`h2::client::Connection`) is spawned onto the
    /// runtime immediately, same as h2's own usage convention — nothing flows
    /// over the session otherwise.
    ///
    /// `transport` is the same `engine::http::TransportOverrides` that
    /// `engine::ws::connect`/SSE/HTTP honor for proxy/client-cert settings.
    /// Unlike those (which upgrade *through* a reqwest `Client` and so inherit
    /// reqwest's proxy/TLS handling for free), this client speaks raw h2 over
    /// a hand-rolled `tokio-rustls` session (see this module's own doc
    /// comment for why: reqwest can't see HTTP/2 trailers), so both settings
    /// are handled by hand here: `proxy_url` via a plain HTTP `CONNECT`
    /// tunnel dialed before TLS/h2 starts (`connect_through_proxy`), and
    /// `client_identity` via the raw PEM `TransportOverrides.client_cert_pem`
    /// carries alongside the opaque `reqwest::Identity` (`client_config`
    /// builds a client-auth rustls `ClientConfig` from it).
    pub(crate) async fn connect(
        url: &str,
        transport: Option<&crate::engine::http::TransportOverrides>,
    ) -> Result<Self, AppError> {
        let target = parse_target(url)?;
        let authority = format!("{}:{}", target.host, target.port);

        let proxy_url = transport
            .and_then(|t| t.proxy_url.as_deref())
            .filter(|p| !p.trim().is_empty());
        let tcp = match proxy_url {
            Some(proxy_url) => connect_through_proxy(proxy_url, &authority).await?,
            None => {
                let tcp = TcpStream::connect(&authority)
                    .await
                    .map_err(|e| AppError::Other(format!("gRPC TCP connect to {authority} failed: {e}")))?;
                tcp.set_nodelay(true).ok();
                tcp
            }
        };

        if target.tls {
            let client_cert = transport.and_then(|t| t.client_cert_pem.as_ref());
            let config = client_config(client_cert)?;
            let connector = TlsConnector::from(config);
            let server_name = ServerName::try_from(target.host.clone()).map_err(|e| {
                AppError::Other(format!("invalid TLS server name {:?}: {e}", target.host))
            })?;
            let tls = connector
                .connect(server_name, tcp)
                .await
                .map_err(|e| AppError::Other(format!("gRPC TLS handshake with {authority} failed: {e}")))?;
            let (_, session) = tls.get_ref();
            if session.alpn_protocol() != Some(b"h2") {
                return Err(AppError::Other(format!(
                    "gRPC server at {authority} did not negotiate ALPN h2"
                )));
            }
            Self::drive(tls, authority, true).await
        } else {
            Self::drive(tcp, authority, false).await
        }
    }

    /// Shared tail of `connect()` for both the plaintext and TLS branches:
    /// runs the h2 client handshake over whatever `AsyncRead + AsyncWrite`
    /// socket was built, spawns the connection driver, and wraps the
    /// resulting `SendRequest` handle. Generic over the IO type so both
    /// branches produce the exact same `GrpcTransport` — no enum/boxing
    /// needed since `SendRequest<Bytes>` itself is IO-agnostic once the
    /// connection is spawned. Exposed at `pub(super)` so the offline loopback
    /// test can drive a self-signed `TlsStream` directly (production
    /// `connect()` trusts the real webpki root store, which would correctly
    /// reject a self-signed loopback cert).
    pub(super) async fn drive<IO>(io: IO, authority: String, tls: bool) -> Result<Self, AppError>
    where
        IO: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let (send_request, connection) = h2::client::handshake(io)
            .await
            .map_err(|e| AppError::Other(format!("h2 handshake with {authority} failed: {e}")))?;
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                eprintln!("gRPC h2 connection driver ended with error: {e}");
            }
        });
        Ok(Self {
            send_request,
            authority,
            tls,
        })
    }

    /// Opens a new HTTP/2 stream for a unary or streaming gRPC call against
    /// `path` (e.g. `/package.Service/Method`) and sends the gRPC-required
    /// request headers: `:method POST`, `:scheme`/`:authority`/`:path`,
    /// `content-type: application/grpc`, `te: trailers` (gRPC's signal that
    /// it expects to read HTTP/2 trailers for the status), and
    /// `grpc-encoding: identity` (no compression — matches `framing::frame`'s
    /// always-uncompressed flag byte).
    ///
    /// Headers are sent with `end_of_stream: false` since the caller still
    /// has to send at least one body frame (even a unary call's single
    /// message is a body frame on this stream, sent via the returned
    /// `GrpcStream::send_frame`). The response is *not* awaited here — see
    /// `GrpcStream`'s doc comment for why that would deadlock.
    pub(crate) async fn send(&mut self, path: &str) -> Result<GrpcStream, AppError> {
        let scheme = if self.tls { "https" } else { "http" };
        let uri = format!("{scheme}://{}{path}", self.authority);
        let request = Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/grpc")
            .header("te", "trailers")
            .header("grpc-encoding", "identity")
            .body(())
            .map_err(|e| AppError::Other(format!("invalid gRPC request: {e}")))?;

        // `ready()` takes `SendRequest` by value; `SendRequest` is `Clone`
        // (cloning just hands out another handle to the same h2 connection),
        // so clone rather than move out of `&mut self`.
        let mut ready = self
            .send_request
            .clone()
            .ready()
            .await
            .map_err(|e| AppError::Other(format!("h2 connection not ready to send: {e}")))?;
        let (response_future, send_stream) = ready
            .send_request(request, false)
            .map_err(|e| AppError::Other(format!("failed to open gRPC stream: {e}")))?;

        Ok(GrpcStream {
            send_stream,
            response: ResponseState::Pending(response_future),
            pending: VecDeque::new(),
            unframer: FrameUnframer::default(),
        })
    }
}

impl GrpcStream {
    /// Sends one length-prefixed gRPC message frame (via
    /// `super::framing::frame`) on this stream. `end_of_stream` should be
    /// `true` for a unary request's only message (or a client-stream's final
    /// message); gRPC clients never send trailers of their own, so ending the
    /// stream is purely a data-frame flag, not a `send_trailers` call.
    pub(crate) fn send_frame(&mut self, payload: &[u8], end_of_stream: bool) -> Result<(), AppError> {
        let framed = frame(payload);
        self.send_stream
            .send_data(Bytes::from(framed), end_of_stream)
            .map_err(|e| AppError::Other(format!("failed to send gRPC frame: {e}")))
    }

    /// Half-closes the send side with no further data: a true HTTP/2
    /// zero-length DATA frame carrying `END_STREAM`, *not* a gRPC message —
    /// `framing::frame(&[])` would instead emit a 5-byte length-prefixed
    /// frame for an empty *message* (compression-flag byte + 4-byte
    /// length-0), which is a real (if degenerate) gRPC message and would
    /// corrupt a client-streaming exchange where the server counts messages.
    /// Needed for client-streaming/bidi once the last request message has
    /// already gone out via `send_frame(_, false)` and the caller is done
    /// sending — `send_frame`'s own `end_of_stream` flag only covers the
    /// "last message IS the final frame" case; this covers "no more
    /// messages, full stop" after the fact.
    pub(crate) fn half_close(&mut self) -> Result<(), AppError> {
        self.send_stream
            .send_data(Bytes::new(), true)
            .map_err(|e| AppError::Other(format!("failed to half-close gRPC request stream: {e}")))
    }

    /// Resolves the response headers (awaiting them on first call, then
    /// caching) and returns the HTTP/2 response status. This is distinct from
    /// the gRPC status, which rides in trailers — see `recv_trailers`. A
    /// non-200 here means the request never reached gRPC semantics at all
    /// (e.g. a proxy/load-balancer error).
    pub(crate) async fn http_status(&mut self) -> Result<StatusCode, AppError> {
        self.resolve_response().await?;
        match &self.response {
            ResponseState::Ready { status, .. } => Ok(*status),
            ResponseState::Pending(_) => unreachable!("resolve_response always leaves Ready"),
        }
    }

    /// Returns the HTTP/2 response HEADERS frame's header map (awaiting it on
    /// first call, same as `http_status`). Exists for the "Trailers-Only"
    /// response shape gRPC servers are allowed to use for an immediate error
    /// (most commonly: a method that doesn't exist, or one rejected before
    /// any message is read/written) — `grpc-status`/`grpc-message` arrive
    /// directly in this HEADERS frame, with no DATA frame and no separate
    /// HTTP/2 trailers frame at all. Without this accessor, a caller that
    /// only checks `recv_trailers` after `recv_frame` returns `None` would
    /// see `None` trailers and have no way to tell "no status info at all"
    /// apart from "status was actually in headers" — silently defaulting to
    /// OK either way, which would hide a real RPC failure (#28's 17d-6 report
    /// flagged exactly this gap, deferred until this module was editable
    /// again in 17d-7).
    pub(crate) async fn response_headers(&mut self) -> Result<&HeaderMap, AppError> {
        self.resolve_response().await?;
        match &self.response {
            ResponseState::Ready { headers, .. } => Ok(headers),
            ResponseState::Pending(_) => unreachable!("resolve_response always leaves Ready"),
        }
    }

    /// Awaits the response future the first time it's needed (lazily, so
    /// `send()` never blocks on it before the caller has sent any request
    /// frames) and caches the resulting status + headers + body stream. A
    /// no-op on every call after the first.
    ///
    /// Cancellation-safety matters here specifically because the streaming
    /// drive loop (`engine::grpc::drive_streaming_call`) calls `recv_frame`
    /// (which calls this) inside a `tokio::select!` arm — `select!` polls
    /// every arm's future once per loop iteration and drops whichever one(s)
    /// didn't complete, so `resolve_response`'s own future can be cancelled
    /// mid-await on any given iteration and re-entered on the next. The
    /// future is therefore awaited *in place*, through `&mut` on the
    /// `Pending` variant, never `take()`n out first: if this await is
    /// cancelled, `self.response` is untouched (still `Pending`, holding the
    /// same not-yet-resolved `ResponseFuture`), so the next call simply polls
    /// it again — the normal, documented behavior of any `Future` per the
    /// `std::future::Future` contract, since `h2`'s `ResponseFuture` doesn't
    /// drop any buffered state on a dropped poll. An earlier version of this
    /// function `take()`-then-awaited a temporary, which left `Pending` *or*
    /// briefly empty for the duration of the await; a `select!`-driven
    /// cancellation there could permanently lose the future and panic on the
    /// next call — caught by 17d-7's streaming loopback tests.
    async fn resolve_response(&mut self) -> Result<(), AppError> {
        let response = match &mut self.response {
            ResponseState::Ready { .. } => return Ok(()),
            ResponseState::Pending(fut) => fut
                .await
                .map_err(|e| AppError::Other(format!("gRPC response failed: {e}")))?,
        };
        let status = response.status();
        let headers = response.headers().clone();
        let body = response.into_body();
        self.response = ResponseState::Ready {
            status,
            headers,
            body,
        };
        Ok(())
    }

    /// Reads the next complete gRPC message frame, buffering HTTP/2 DATA
    /// frames as they arrive (they don't align to gRPC message boundaries).
    /// Returns `Ok(None)` once the response body is exhausted (the caller
    /// should then read trailers via `recv_trailers`).
    pub(crate) async fn recv_frame(&mut self) -> Result<Option<Vec<u8>>, AppError> {
        self.resolve_response().await?;
        loop {
            if let Some(payload) = self.pending.pop_front() {
                return Ok(Some(payload));
            }
            let body = match &mut self.response {
                ResponseState::Ready { body, .. } => body,
                ResponseState::Pending(_) => unreachable!("resolve_response always leaves Ready"),
            };
            match body.data().await {
                Some(Ok(bytes)) => {
                    let len = bytes.len();
                    let frames = self.unframer.feed(&bytes);
                    self.pending.extend(frames);
                    // Release flow-control capacity now that we've consumed
                    // the chunk — without this, large/long-lived responses
                    // stall once the initial window is exhausted.
                    let _ = body.flow_control().release_capacity(len);
                }
                Some(Err(e)) => {
                    return Err(AppError::Other(format!("gRPC frame read failed: {e}")));
                }
                None => return Ok(None),
            }
        }
    }

    /// Reads the HTTP/2 trailers carrying `grpc-status`/`grpc-message` — the
    /// entire reason this transport is hand-rolled h2 rather than reqwest,
    /// which has no API surface for HTTP/2 trailers. Should be called after
    /// `recv_frame` has returned `Ok(None)` (end of the data stream);
    /// returns `Ok(None)` if the peer closed without sending trailers at all.
    pub(crate) async fn recv_trailers(&mut self) -> Result<Option<HeaderMap>, AppError> {
        self.resolve_response().await?;
        let body = match &mut self.response {
            ResponseState::Ready { body, .. } => body,
            ResponseState::Pending(_) => unreachable!("resolve_response always leaves Ready"),
        };
        body.trailers()
            .await
            .map_err(|e| AppError::Other(format!("gRPC trailers read failed: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use http::HeaderMap;
    use tokio::net::{TcpListener, TcpStream};
    use tokio_rustls::{TlsAcceptor, TlsConnector};

    use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
    use rustls::server::ServerConfig;
    use rustls::{ClientConfig, RootCertStore};

    use super::*;

    /// Self-signed cert with SAN `localhost`, via `rcgen` (dev-only dep, never
    /// shipped). ECDSA P-256 + SHA-256 so it's verifiable by the `aws-lc-rs`
    /// provider used below (standard curve — both `ring`-signed certs and
    /// `aws-lc-rs` verification interop).
    fn self_signed_cert() -> (CertificateDer<'static>, PrivateKeyDer<'static>) {
        let params =
            rcgen::CertificateParams::new(vec!["localhost".to_string()])
                .expect("rcgen: subject alt names");
        let key_pair = rcgen::KeyPair::generate().expect("rcgen: generate keypair");
        let cert = params
            .self_signed(&key_pair)
            .expect("rcgen: self-signed cert");
        let cert_der = cert.der().clone();
        // rcgen 0.13 emits PKCS#8 by default.
        let key = PrivateKeyDer::Pkcs8(key_pair.serialize_der().to_vec().into());
        (cert_der, key)
    }

    /// Same shape as the production `webpki_client_config`, but trusting the
    /// test's self-signed root instead of the real webpki/Mozilla store —
    /// production `connect()` would (correctly) reject a self-signed loopback
    /// cert, so the round-trip test below builds its own `TlsStream` with
    /// this config and hands it straight to `GrpcTransport::drive`, the same
    /// generic-over-IO tail `connect()` itself uses for both schemes.
    fn test_client_config(root: CertificateDer<'static>) -> Arc<ClientConfig> {
        let mut roots = RootCertStore::empty();
        roots.add(root).expect("add self-signed cert as trusted root");
        let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
        let mut cfg = ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .expect("rustls: safe protocol versions")
            .with_root_certificates(roots)
            .with_no_client_auth();
        cfg.alpn_protocols = vec![b"h2".to_vec()];
        Arc::new(cfg)
    }

    fn server_config(
        cert: CertificateDer<'static>,
        key: PrivateKeyDer<'static>,
    ) -> Arc<ServerConfig> {
        let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
        let mut cfg = ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .expect("rustls: safe protocol versions")
            .with_no_client_auth()
            .with_single_cert(vec![cert], key)
            .expect("rustls: load self-signed cert/key");
        cfg.alpn_protocols = vec![b"h2".to_vec()];
        Arc::new(cfg)
    }

    /// Offline proof the chosen TLS stack (#23) composes: a loopback TCP
    /// socket, a `tokio-rustls` server + client, both pinned to ALPN `h2`, and
    /// the negotiated ALPN protocol comes out as `h2`. The full HTTP/2 frame
    /// round-trip on top of this session is covered by the test below this
    /// one; this isolates the riskiest *new* surface (rustls + ALPN +
    /// provider wiring, none of which this repo had before 17d) so the stack
    /// decision gates nothing downstream on a surprise.
    #[tokio::test(flavor = "current_thread")]
    async fn loopback_tls_handshake_negotiates_alpn_h2() {
        let (cert, key) = self_signed_cert();
        let server_cfg = server_config(cert.clone(), key);
        let client_cfg = test_client_config(cert);

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local_addr");

        let server = tokio::spawn(async move {
            let (sock, _) = listener.accept().await.expect("accept");
            let acceptor = TlsAcceptor::from(server_cfg);
            let mut tls = acceptor.accept(sock).await.expect("server tls handshake");
            // ALPN negotiated by the server side.
            let (_, server_session) = tls.get_ref();
            assert_eq!(
                server_session.alpn_protocol(),
                Some(b"h2".as_slice()),
                "server should have ALPN-negotiated h2"
            );
            // Echo a byte back so the client's read path also proves the
            // session is bidirectionally usable, not just established.
            use tokio::io::AsyncWriteExt;
            tls.write_all(b"hello-h2").await.expect("server write");
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            tls.shutdown().await.ok();
        });

        let sock = TcpStream::connect(addr).await.expect("client tcp connect");
        let connector = TlsConnector::from(client_cfg);
        let domain = ServerName::try_from("localhost").expect("server name");
        let mut tls = connector.connect(domain, sock).await.expect("client tls handshake");
        let (_, client_session) = tls.get_ref();
        assert_eq!(
            client_session.alpn_protocol(),
            Some(b"h2".as_slice()),
            "client should have ALPN-negotiated h2"
        );

        // Drain the server's echoed byte to confirm the session carries data.
        use tokio::io::AsyncReadExt;
        let mut buf = [0u8; 8];
        tls.read_exact(&mut buf).await.expect("client read");
        assert_eq!(&buf, b"hello-h2");

        server.await.expect("server task did not finish cleanly");
    }

    /// Full loopback KAT for the production transport code: a minimal h2
    /// server (via `h2::server::handshake`) over the same loopback TCP+TLS
    /// socket pattern as the test above, with `GrpcTransport::drive` used
    /// directly on the client's `TlsStream` (bypassing `connect()`'s webpki
    /// trust store, which would reject this self-signed cert — see
    /// `test_client_config`'s doc comment). Proves: the client sends the
    /// required gRPC headers, a length-prefixed frame written via
    /// `send_frame` decodes byte-for-byte on the server side, and a
    /// `grpc-status`/`grpc-message` pair sent back as HTTP/2 trailers is
    /// readable via `recv_trailers` (the entire reason this hand-rolled
    /// client exists, per #17c's open questions — reqwest can't see
    /// trailers). The client sends its request frame before resolving
    /// response headers, mirroring a real unary exchange where the server
    /// reads the whole request body before responding — awaiting headers any
    /// earlier would deadlock both sides.
    #[tokio::test(flavor = "current_thread")]
    async fn loopback_h2_round_trip_sends_frame_and_reads_status_trailers() {
        let (cert, key) = self_signed_cert();
        let server_cfg = server_config(cert.clone(), key);
        let client_cfg = test_client_config(cert);

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local_addr");

        let request_payload = vec![0xDE, 0xAD, 0xBE, 0xEF, 0x42];
        let expected_frame = request_payload.clone();
        let response_payload = vec![0xC0, 0xFF, 0xEE];
        let response_payload_for_server = response_payload.clone();

        let server = tokio::spawn(async move {
            let (sock, _) = listener.accept().await.expect("server accept tcp");
            let acceptor = TlsAcceptor::from(server_cfg);
            let tls = acceptor.accept(sock).await.expect("server tls handshake");

            let mut conn = h2::server::handshake(tls)
                .await
                .expect("server h2 handshake");

            let (request, mut respond) = conn
                .accept()
                .await
                .expect("server should see an incoming stream")
                .expect("server accept should not error");

            // `send_response`/`send_trailers` below only *queue* frames into
            // the connection state; they only reach the wire once `conn` is
            // polled again. `respond`/the request body are independent
            // handles, so it's safe to keep driving `conn` in the background
            // while this task reads the body and queues its response.
            tokio::spawn(async move { while conn.accept().await.is_some() {} });

            // Assert the gRPC-required headers landed.
            assert_eq!(request.method(), &http::Method::POST);
            assert_eq!(
                request.headers().get("content-type").map(|v| v.to_str().unwrap()),
                Some("application/grpc")
            );
            assert_eq!(
                request.headers().get("te").map(|v| v.to_str().unwrap()),
                Some("trailers")
            );
            assert_eq!(
                request.headers().get("grpc-encoding").map(|v| v.to_str().unwrap()),
                Some("identity")
            );
            assert_eq!(request.uri().path(), "/test.Service/Method");

            // Read the client's single length-prefixed frame back to raw
            // bytes and assert it round-trips through h2 DATA frames intact.
            let mut body = request.into_body();
            let mut received = Vec::new();
            while let Some(chunk) = body.data().await {
                let chunk = chunk.expect("server body read");
                received.extend_from_slice(&chunk);
                let _ = body.flow_control().release_capacity(chunk.len());
            }
            assert_eq!(
                received,
                frame(&expected_frame),
                "server should receive the exact length-prefixed frame bytes the client sent"
            );

            // Respond with headers, one length-prefixed response message
            // (exercises the client's recv_frame/unframer path, not just
            // send_frame), then gRPC status trailers (grpc-status rides in
            // HTTP/2 trailers, never headers).
            let response = http::Response::builder()
                .status(200)
                .body(())
                .expect("server response head");
            let mut send_stream = respond
                .send_response(response, false)
                .expect("server send_response");
            send_stream
                .send_data(Bytes::from(frame(&response_payload_for_server)), false)
                .expect("server send_data");

            let mut trailers = HeaderMap::new();
            trailers.insert("grpc-status", "0".parse().unwrap());
            trailers.insert("grpc-message", "".parse().unwrap());
            send_stream
                .send_trailers(trailers)
                .expect("server send_trailers");
        });

        let sock = TcpStream::connect(addr).await.expect("client tcp connect");
        let connector = TlsConnector::from(client_cfg);
        let domain = ServerName::try_from("localhost").expect("server name");
        let tls = connector
            .connect(domain, sock)
            .await
            .expect("client tls handshake");

        let mut transport = GrpcTransport::drive(tls, "localhost:0".to_string(), true)
            .await
            .expect("client h2 handshake over the loopback TLS session");

        let mut stream = transport
            .send("/test.Service/Method")
            .await
            .expect("client send should open a stream");

        stream
            .send_frame(&request_payload, true)
            .expect("client send_frame should succeed");

        let status = stream
            .http_status()
            .await
            .expect("client should resolve response headers after sending its frame");
        assert_eq!(status, http::StatusCode::OK);

        // Exercises the unframer/pending-queue/flow-control path on the recv
        // side, not just send_frame: the server sent exactly one
        // length-prefixed response message ahead of its trailers.
        let received_frame = stream
            .recv_frame()
            .await
            .expect("client recv_frame")
            .expect("server sent one response frame");
        assert_eq!(received_frame, response_payload);

        // Drain to end-of-stream — no more data frames, only trailers next.
        let next = stream.recv_frame().await.expect("client recv_frame");
        assert_eq!(next, None, "exactly one response frame was sent");

        let trailers = stream
            .recv_trailers()
            .await
            .expect("client recv_trailers")
            .expect("server should have sent trailers");
        assert_eq!(
            trailers.get("grpc-status").map(|v| v.to_str().unwrap()),
            Some("0")
        );
        assert_eq!(
            trailers.get("grpc-message").map(|v| v.to_str().unwrap()),
            Some("")
        );

        server.await.expect("server task did not finish cleanly");
    }

    /// Proves `response_headers()` can see a "Trailers-Only" gRPC response:
    /// the server puts `grpc-status`/`grpc-message` directly in the HEADERS
    /// frame (no DATA frame, no separate HTTP/2 trailers frame at all) —
    /// real servers use this shape for an immediate error, most commonly
    /// "method not found." Before this accessor existed, a caller could only
    /// see `recv_frame` return `None` and `recv_trailers` return `None`,
    /// with no way to distinguish "no status info at all" from "status was
    /// actually in headers" — silently defaulting to OK either way, which
    /// would hide a real RPC failure (flagged as a gap in the 17d-6 unary
    /// drive's report, fixed here in 17d-7 since streaming modes hit the
    /// same shape: a server-streaming RPC that errors before sending any
    /// message looks exactly like this).
    #[tokio::test(flavor = "current_thread")]
    async fn trailers_only_response_status_is_readable_from_headers() {
        let (cert, key) = self_signed_cert();
        let server_cfg = server_config(cert.clone(), key);
        let client_cfg = test_client_config(cert);

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local_addr");

        let server = tokio::spawn(async move {
            let (sock, _) = listener.accept().await.expect("server accept tcp");
            let acceptor = TlsAcceptor::from(server_cfg);
            let tls = acceptor.accept(sock).await.expect("server tls handshake");

            let mut conn = h2::server::handshake(tls)
                .await
                .expect("server h2 handshake");
            let (request, mut respond) = conn
                .accept()
                .await
                .expect("server should see an incoming stream")
                .expect("server accept should not error");
            tokio::spawn(async move { while conn.accept().await.is_some() {} });

            // Drain the client's request body before responding.
            let mut body = request.into_body();
            while let Some(chunk) = body.data().await {
                let chunk = chunk.expect("server body read");
                let _ = body.flow_control().release_capacity(chunk.len());
            }

            // Trailers-Only: grpc-status/grpc-message ride in the HEADERS
            // frame itself, end_of_stream = true, no DATA frame and no
            // separate trailers frame follow at all.
            let response = http::Response::builder()
                .status(200)
                .header("grpc-status", "12")
                .header("grpc-message", "unimplemented method")
                .body(())
                .expect("server response head");
            respond
                .send_response(response, true)
                .expect("server send_response (end_of_stream)");
        });

        let sock = TcpStream::connect(addr).await.expect("client tcp connect");
        let connector = TlsConnector::from(client_cfg);
        let domain = ServerName::try_from("localhost").expect("server name");
        let tls = connector
            .connect(domain, sock)
            .await
            .expect("client tls handshake");

        let mut transport = GrpcTransport::drive(tls, "localhost:0".to_string(), true)
            .await
            .expect("client h2 handshake over the loopback TLS session");

        let mut stream = transport
            .send("/test.Service/Method")
            .await
            .expect("client send should open a stream");
        stream
            .send_frame(&[], true)
            .expect("client send_frame should succeed");

        // No response message and no separate trailers frame at all.
        let frame = stream.recv_frame().await.expect("client recv_frame");
        assert_eq!(frame, None, "Trailers-Only response sends no DATA frame");
        let trailers = stream.recv_trailers().await.expect("client recv_trailers");
        assert_eq!(
            trailers, None,
            "Trailers-Only response sends no separate trailers frame"
        );

        // The status is readable from the HEADERS frame itself.
        let headers = stream
            .response_headers()
            .await
            .expect("client response_headers");
        assert_eq!(
            headers.get("grpc-status").map(|v| v.to_str().unwrap()),
            Some("12")
        );
        assert_eq!(
            headers.get("grpc-message").map(|v| v.to_str().unwrap()),
            Some("unimplemented method")
        );

        server.await.expect("server task did not finish cleanly");
    }

    #[test]
    fn parses_plaintext_grpc_url_with_default_port() {
        let target = parse_target("grpc://example.com").expect("parse");
        assert!(!target.tls);
        assert_eq!(target.host, "example.com");
        assert_eq!(target.port, 80);
    }

    #[test]
    fn parses_tls_grpc_url_with_explicit_port() {
        let target = parse_target("grpcs://example.com:8443").expect("parse");
        assert!(target.tls);
        assert_eq!(target.host, "example.com");
        assert_eq!(target.port, 8443);
    }

    #[test]
    fn parses_tls_grpc_url_with_default_port() {
        let target = parse_target("grpcs://example.com").expect("parse");
        assert!(target.tls);
        assert_eq!(target.port, 443);
    }

    #[test]
    fn strips_trailing_path_from_authority() {
        let target = parse_target("grpc://example.com:50051/pkg.Service/Method").expect("parse");
        assert_eq!(target.host, "example.com");
        assert_eq!(target.port, 50051);
    }

    #[test]
    fn rejects_unsupported_scheme() {
        assert!(parse_target("http://example.com").is_err());
    }

    #[test]
    fn rejects_missing_host() {
        assert!(parse_target("grpc://").is_err());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn connect_with_no_overrides_reaches_the_real_dial() {
        let result = GrpcTransport::connect("grpc://127.0.0.1:1", None).await;
        let err = match result {
            Ok(_) => panic!("connecting to an unused loopback port should fail at the dial"),
            Err(e) => e,
        };
        assert!(err.to_string().contains("TCP connect"));
    }

    // --- proxy: HTTP CONNECT tunnel ----------------------------------------

    /// A minimal forward proxy: accepts one connection, reads a `CONNECT`
    /// request, dials the target itself, answers `200`, then splices bytes
    /// bidirectionally — the same shape a real corporate proxy presents.
    async fn run_test_connect_proxy(proxy_listener: TcpListener) {
        let (mut client_sock, _) = proxy_listener.accept().await.expect("proxy accept");
        let mut buf = Vec::new();
        let mut byte = [0u8; 1];
        loop {
            let n = client_sock.read(&mut byte).await.expect("proxy read CONNECT");
            assert!(n > 0, "client closed before completing CONNECT");
            buf.push(byte[0]);
            if buf.len() >= 4 && buf[buf.len() - 4..] == *b"\r\n\r\n" {
                break;
            }
        }
        let request_text = String::from_utf8_lossy(&buf);
        let request_line = request_text.lines().next().unwrap_or("");
        assert!(
            request_line.starts_with("CONNECT "),
            "expected a CONNECT request, got: {request_line}"
        );
        let target = request_line.split_whitespace().nth(1).unwrap_or("");
        let mut origin_sock = TcpStream::connect(target).await.expect("proxy dial origin");
        client_sock
            .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            .await
            .expect("proxy send 200");
        tokio::io::copy_bidirectional(&mut client_sock, &mut origin_sock)
            .await
            .ok();
    }

    /// Proves `GrpcTransport::connect` actually tunnels through a configured
    /// proxy rather than dialing the target directly: the "origin" listener
    /// only accepts a connection *from the proxy*, so a real h2 client
    /// preface completing against it means the CONNECT tunnel carried real
    /// h2 traffic, not just a bare TCP handshake.
    #[tokio::test(flavor = "current_thread")]
    async fn connect_tunnels_through_http_connect_proxy() {
        let origin_listener = TcpListener::bind("127.0.0.1:0").await.expect("bind origin");
        let origin_addr = origin_listener.local_addr().expect("origin addr");
        let origin = tokio::spawn(async move {
            let (sock, _) = origin_listener.accept().await.expect("origin accept");
            let _conn = h2::server::handshake(sock)
                .await
                .expect("origin h2 handshake should complete through the tunnel");
        });

        let proxy_listener = TcpListener::bind("127.0.0.1:0").await.expect("bind proxy");
        let proxy_addr = proxy_listener.local_addr().expect("proxy addr");
        let proxy = tokio::spawn(run_test_connect_proxy(proxy_listener));

        let overrides = crate::engine::http::TransportOverrides {
            proxy_url: Some(format!("http://{proxy_addr}")),
            ..Default::default()
        };
        GrpcTransport::connect(&format!("grpc://{origin_addr}"), Some(&overrides))
            .await
            .expect("connect through proxy should succeed");

        origin.await.expect("origin task panicked");
        proxy.await.expect("proxy task panicked");
    }

    /// A proxy that refuses the tunnel (a non-200 CONNECT response) must
    /// surface as a connect error, not a silent fallback to a direct dial.
    #[tokio::test(flavor = "current_thread")]
    async fn connect_surfaces_proxy_connect_refusal() {
        let proxy_listener = TcpListener::bind("127.0.0.1:0").await.expect("bind proxy");
        let proxy_addr = proxy_listener.local_addr().expect("proxy addr");
        let proxy = tokio::spawn(async move {
            let (mut client_sock, _) = proxy_listener.accept().await.expect("proxy accept");
            let mut buf = Vec::new();
            let mut byte = [0u8; 1];
            loop {
                let n = client_sock.read(&mut byte).await.expect("proxy read CONNECT");
                assert!(n > 0);
                buf.push(byte[0]);
                if buf.len() >= 4 && buf[buf.len() - 4..] == *b"\r\n\r\n" {
                    break;
                }
            }
            client_sock
                .write_all(b"HTTP/1.1 502 Bad Gateway\r\n\r\n")
                .await
                .expect("proxy send 502");
        });

        let overrides = crate::engine::http::TransportOverrides {
            proxy_url: Some(format!("http://{proxy_addr}")),
            ..Default::default()
        };
        let result = GrpcTransport::connect("grpc://127.0.0.1:1", Some(&overrides)).await;
        let err = match result {
            Ok(_) => panic!("a proxy CONNECT refusal must surface as an error"),
            Err(e) => e,
        };
        assert!(err.to_string().contains("502"));
        proxy.await.expect("proxy task panicked");
    }

    // --- mTLS: client certificate authentication ---------------------------

    /// Builds a `ClientConfig` exactly like `client_config`'s client-cert
    /// branch, but trusting the test's self-signed server root instead of
    /// the real webpki store — mirrors `test_client_config`'s rationale.
    fn test_client_config_with_identity(
        server_root: CertificateDer<'static>,
        client_cert: &ClientCertPem,
    ) -> Arc<ClientConfig> {
        let mut roots = RootCertStore::empty();
        roots.add(server_root).expect("add self-signed server cert as trusted root");
        let (certs, key) = parse_client_identity(client_cert).expect("parse test client identity");
        let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
        let mut cfg = ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .expect("rustls: safe protocol versions")
            .with_root_certificates(roots)
            .with_client_auth_cert(certs, key)
            .expect("rustls: install client auth cert");
        cfg.alpn_protocols = vec![b"h2".to_vec()];
        Arc::new(cfg)
    }

    /// A server config that *requires* a client certificate, trusting only
    /// `client_root` — the same shape a corporate gRPC server enforcing mTLS
    /// presents.
    fn server_config_requiring_client_cert(
        cert: CertificateDer<'static>,
        key: PrivateKeyDer<'static>,
        client_root: CertificateDer<'static>,
    ) -> Arc<ServerConfig> {
        let mut roots = RootCertStore::empty();
        roots.add(client_root).expect("add client root");
        let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
        let verifier = rustls::server::WebPkiClientVerifier::builder_with_provider(
            Arc::new(roots),
            provider.clone(),
        )
        .build()
        .expect("build client cert verifier");
        let mut cfg = ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .expect("rustls: safe protocol versions")
            .with_client_cert_verifier(verifier)
            .with_single_cert(vec![cert], key)
            .expect("rustls: load self-signed cert/key");
        cfg.alpn_protocols = vec![b"h2".to_vec()];
        Arc::new(cfg)
    }

    fn self_signed_client_identity() -> (ClientCertPem, CertificateDer<'static>) {
        let params = rcgen::CertificateParams::new(vec!["restman-client".to_string()])
            .expect("rcgen: subject alt names");
        let key_pair = rcgen::KeyPair::generate().expect("rcgen: generate keypair");
        let cert = params.self_signed(&key_pair).expect("rcgen: self-signed cert");
        let der = cert.der().clone();
        let pem = ClientCertPem {
            cert_pem: cert.pem(),
            key_pem: key_pair.serialize_pem(),
        };
        (pem, der)
    }

    /// Proves mTLS actually authenticates the client, not just that two TLS
    /// stacks agree to talk: a server requiring a client cert accepts the
    /// handshake when the (matching-root) client cert is presented, and
    /// rejects it outright when it isn't.
    #[tokio::test(flavor = "current_thread")]
    async fn mtls_handshake_succeeds_only_when_client_cert_presented() {
        let (server_cert, server_key) = self_signed_cert();
        let (client_pem, client_root) = self_signed_client_identity();
        let server_cfg =
            server_config_requiring_client_cert(server_cert.clone(), server_key, client_root);
        let client_cfg = test_client_config_with_identity(server_cert.clone(), &client_pem);

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local_addr");
        let server = tokio::spawn(async move {
            let (sock, _) = listener.accept().await.expect("accept");
            let acceptor = TlsAcceptor::from(server_cfg);
            acceptor
                .accept(sock)
                .await
                .expect("server should accept a client presenting the required cert");
        });

        let sock = TcpStream::connect(addr).await.expect("client tcp connect");
        let connector = TlsConnector::from(client_cfg);
        let domain = ServerName::try_from("localhost").expect("server name");
        connector
            .connect(domain, sock)
            .await
            .expect("client tls handshake with client cert should succeed");
        server.await.expect("server task did not finish cleanly");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mtls_handshake_rejected_without_client_cert() {
        let (server_cert, server_key) = self_signed_cert();
        let (_unused_pem, client_root) = self_signed_client_identity();
        let server_cfg =
            server_config_requiring_client_cert(server_cert.clone(), server_key, client_root);
        // No client identity this time — same trusted server root, but the
        // client presents no certificate at all.
        let client_cfg = test_client_config(server_cert.clone());

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local_addr");
        let server = tokio::spawn(async move {
            let (sock, _) = listener.accept().await.expect("accept");
            let acceptor = TlsAcceptor::from(server_cfg);
            let result = acceptor.accept(sock).await;
            assert!(
                result.is_err(),
                "server requiring a client cert must reject a handshake with none presented"
            );
        });

        let sock = TcpStream::connect(addr).await.expect("client tcp connect");
        let connector = TlsConnector::from(client_cfg);
        let domain = ServerName::try_from("localhost").expect("server name");
        // The client side may see the handshake fail too (server closes the
        // connection); either outcome is fine, the server-side assertion
        // above is the one that matters.
        let _ = connector.connect(domain, sock).await;
        server.await.expect("server task did not finish cleanly");
    }

}
