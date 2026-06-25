//! WebSocket transport. The handshake runs *through* reqwest, so the
//! workspace's proxy / client-cert / default-header transport settings carry
//! over exactly as they do for HTTP and SSE; the upgraded byte stream is then
//! framed by tokio-tungstenite via `from_raw_socket`. We deliberately never
//! use tokio-tungstenite's own `connect_async` — it would open a second,
//! untransported TCP/TLS connection and silently ignore those settings (fatal
//! behind a corporate proxy).
//!
//! Like `engine::sse`, the network part can't run in this sandbox; it's kept
//! thin and correct-by-inspection. The one piece with real logic — deriving
//! and validating the `Sec-WebSocket-Accept` key — is a pure function unit
//! tested below against the RFC 6455 example vector.

use crate::error::AppError;
use crate::model::http::HeaderEntry;
use base64::Engine as _;
use rand::RngCore;
use reqwest::header::{
    CONNECTION, SEC_WEBSOCKET_ACCEPT, SEC_WEBSOCKET_KEY, SEC_WEBSOCKET_VERSION, UPGRADE,
};
use reqwest::{Client, StatusCode, Upgraded};
use tokio_tungstenite::tungstenite::handshake::derive_accept_key;
use tokio_tungstenite::tungstenite::protocol::Role;
use tokio_tungstenite::WebSocketStream;

/// A fresh 16-byte `Sec-WebSocket-Key`, base64-encoded (RFC 6455 §4.1).
fn generate_key() -> String {
    let mut bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// Maps the user-facing `ws(s)://` scheme to the `http(s)://` reqwest needs for
/// the upgrade GET. Other schemes pass through unchanged (reqwest rejects them).
fn http_url(url: &str) -> String {
    if let Some(rest) = url.strip_prefix("wss://") {
        format!("https://{rest}")
    } else if let Some(rest) = url.strip_prefix("ws://") {
        format!("http://{rest}")
    } else {
        url.to_string()
    }
}

/// Performs the WebSocket upgrade handshake over `client` and returns a framed
/// stream ready for read/write. Validates the server's `Sec-WebSocket-Accept`
/// against the key we sent before handing back the socket.
pub async fn connect(
    client: &Client,
    url: &str,
    headers: &[HeaderEntry],
) -> Result<WebSocketStream<Upgraded>, AppError> {
    let key = generate_key();
    let mut builder = client
        .get(http_url(url))
        .header(UPGRADE, "websocket")
        .header(CONNECTION, "Upgrade")
        .header(SEC_WEBSOCKET_VERSION, "13")
        .header(SEC_WEBSOCKET_KEY, &key);
    for h in headers
        .iter()
        .filter(|h| h.enabled && !h.name.trim().is_empty())
    {
        builder = builder.header(&h.name, &h.value);
    }

    let resp = builder.send().await?;
    if resp.status() != StatusCode::SWITCHING_PROTOCOLS {
        return Err(AppError::Other(format!(
            "WebSocket upgrade failed: server returned {} (expected 101)",
            resp.status()
        )));
    }

    let expected = derive_accept_key(key.as_bytes());
    let got = resp
        .headers()
        .get(SEC_WEBSOCKET_ACCEPT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    if got != expected {
        return Err(AppError::Other(
            "WebSocket handshake failed: Sec-WebSocket-Accept mismatch".to_string(),
        ));
    }

    let upgraded = resp.upgrade().await?;
    Ok(WebSocketStream::from_raw_socket(upgraded, Role::Client, None).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    // RFC 6455 §1.3 example: key "dGhlIHNhbXBsZSBub25jZQ==" must derive accept
    // "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=". Locks our handshake validation against the
    // spec vector — the one piece of WS logic verifiable offline.
    #[test]
    fn derives_rfc6455_accept_key() {
        assert_eq!(
            derive_accept_key(b"dGhlIHNhbXBsZSBub25jZQ=="),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
    }

    #[test]
    fn generated_key_is_16_bytes_base64() {
        let key = generate_key();
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(key.as_bytes())
            .expect("key must be valid base64");
        assert_eq!(decoded.len(), 16);
    }

    #[test]
    fn rewrites_ws_schemes_to_http() {
        assert_eq!(http_url("ws://example.com/x"), "http://example.com/x");
        assert_eq!(http_url("wss://example.com/x"), "https://example.com/x");
        assert_eq!(http_url("http://example.com/x"), "http://example.com/x");
    }
}
