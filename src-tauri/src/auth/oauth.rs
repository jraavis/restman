//! OAuth2 token lifecycle: the authorize-URL half of the Authorization Code
//! browser flow plus the pure parsing it needs (`loopback_bind_addr`,
//! `parse_callback_request_line`), all four grant-type token exchanges,
//! refresh, and the on-disk/keychain cache (`token_store`).
//!
//! Deliberately has no opinion on *when* to use a cached token vs. fetch a
//! fresh one — that orchestration needs the DB connection only for two quick
//! sync calls (`token_store::get`/`put`) bracketing a network round trip, and
//! holding `AppState`'s mutex across that round trip would block every other
//! command for as long as the IdP takes to respond. So the caller
//! (`commands::http::send_request` for the headless grants,
//! `commands::oauth::start_oauth2_authorization` for Authorization Code) owns
//! the lock scope: read the cache, drop the lock, await
//! `fetch_token`/`exchange_refresh_token`/`exchange_code` here, then re-lock
//! to `token_store::put`. This module never holds a `Connection` across an
//! `.await`.
//!
//! Opening the actual browser and running the loopback `TcpListener` that
//! catches the redirect is a Tauri/socket-specific concern that lives in
//! `commands::oauth` instead — this module only builds the URL and parses
//! what comes back, so it stays testable without a real socket or browser.

pub mod token_store;

use crate::error::{AppError, AppResult};
use crate::model::auth::{OAuth2Config, OAuth2GrantType, PkceMethod};
use oauth2::basic::BasicClient;
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointNotSet, EndpointSet,
    PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, RefreshToken, ResourceOwnerPassword,
    ResourceOwnerUsername, Scope, TokenResponse, TokenUrl,
};

/// A client configured only enough to hit the token endpoint — used by every
/// grant type except building the authorize URL itself. `auth_url`/redirect
/// aren't required for client-credentials/password/refresh, so this doesn't
/// force the user to fill in a meaningless Auth URL field for those grants.
type TokenClient = BasicClient<EndpointNotSet, EndpointNotSet, EndpointNotSet, EndpointNotSet, EndpointSet>;

/// A client with the auth endpoint set too — only `authorize_url` needs this.
type AuthorizeClient = BasicClient<EndpointSet, EndpointNotSet, EndpointNotSet, EndpointNotSet, EndpointSet>;

fn build_token_client(config: &OAuth2Config) -> AppResult<TokenClient> {
    let client = BasicClient::new(ClientId::new(config.client_id.clone()))
        .set_client_secret(ClientSecret::new(config.client_secret.clone()))
        .set_token_uri(TokenUrl::new(config.token_url.clone()).map_err(|e| AppError::Other(format!("invalid token URL: {e}")))?);
    Ok(if config.redirect_uri.trim().is_empty() {
        client
    } else {
        client.set_redirect_uri(RedirectUrl::new(config.redirect_uri.clone()).map_err(|e| AppError::Other(format!("invalid redirect URI: {e}")))?)
    })
}

fn build_authorize_client(config: &OAuth2Config) -> AppResult<AuthorizeClient> {
    Ok(BasicClient::new(ClientId::new(config.client_id.clone()))
        .set_client_secret(ClientSecret::new(config.client_secret.clone()))
        .set_auth_uri(AuthUrl::new(config.auth_url.clone()).map_err(|e| AppError::Other(format!("invalid auth URL: {e}")))?)
        .set_token_uri(TokenUrl::new(config.token_url.clone()).map_err(|e| AppError::Other(format!("invalid token URL: {e}")))?)
        .set_redirect_uri(RedirectUrl::new(config.redirect_uri.clone()).map_err(|e| AppError::Other(format!("invalid redirect URI: {e}")))?))
}

/// oauth2's own docs warn this is required to prevent SSRF: a token-endpoint
/// HTTP client must not follow redirects.
fn http_client() -> AppResult<reqwest::Client> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| AppError::Other(format!("failed to build OAuth2 HTTP client: {e}")))
}

fn scopes_of(config: &OAuth2Config) -> Vec<Scope> {
    config.scope.split_whitespace().map(|s| Scope::new(s.to_string())).collect()
}

fn to_cached(token: &impl TokenResponse) -> token_store::CachedToken {
    token_store::CachedToken {
        access_token: token.access_token().secret().clone(),
        token_type: format!("{:?}", token.token_type()),
        refresh_token: token.refresh_token().map(|r| r.secret().clone()),
        expires_at: token.expires_in().map(|d| crate::util::now_millis() + d.as_millis() as i64),
        scope: token
            .scopes()
            .map(|scopes| scopes.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" ")),
    }
}

/// What the caller needs to send the user's browser to the IdP and, later,
/// match up + verify the redirect it sends back.
pub struct AuthorizeRequest {
    pub url: String,
    pub csrf_state: String,
    /// Empty when `config.pkce` is `PkceMethod::None`.
    pub pkce_verifier: String,
}

/// Builds the URL to open in the user's browser for the Authorization Code
/// grant. The caller is responsible for stashing `csrf_state`/`pkce_verifier`
/// until the redirect comes back (not persisted here — purely in-memory,
/// single-use, lives only as long as the in-flight authorization).
pub fn authorize_url(config: &OAuth2Config) -> AppResult<AuthorizeRequest> {
    let client = build_authorize_client(config)?;
    let mut req = client.authorize_url(CsrfToken::new_random);
    for scope in scopes_of(config) {
        req = req.add_scope(scope);
    }
    let pkce_verifier = match config.pkce {
        PkceMethod::S256 => {
            let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
            req = req.set_pkce_challenge(challenge);
            verifier.secret().clone()
        }
        PkceMethod::Plain => {
            let (challenge, verifier) = PkceCodeChallenge::new_random_plain();
            req = req.set_pkce_challenge(challenge);
            verifier.secret().clone()
        }
        PkceMethod::None => String::new(),
    };
    let (url, csrf) = req.url();
    Ok(AuthorizeRequest { url: url.to_string(), csrf_state: csrf.secret().clone(), pkce_verifier })
}

/// Finishes the Authorization Code grant: trades the code the redirect
/// carried (plus the PKCE verifier stashed from `authorize_url`, if any) for
/// a token.
pub async fn exchange_code(config: &OAuth2Config, code: &str, pkce_verifier: &str) -> AppResult<token_store::CachedToken> {
    let client = build_token_client(config)?;
    let http = http_client()?;
    let mut req = client.exchange_code(AuthorizationCode::new(code.to_string()));
    if !pkce_verifier.is_empty() {
        req = req.set_pkce_verifier(PkceCodeVerifier::new(pkce_verifier.to_string()));
    }
    let token = req
        .request_async(&http)
        .await
        .map_err(|e| AppError::Other(format!("OAuth2 code exchange failed: {e}")))?;
    Ok(to_cached(&token))
}

pub async fn exchange_client_credentials(config: &OAuth2Config) -> AppResult<token_store::CachedToken> {
    let client = build_token_client(config)?;
    let http = http_client()?;
    let mut req = client.exchange_client_credentials();
    for scope in scopes_of(config) {
        req = req.add_scope(scope);
    }
    let token = req
        .request_async(&http)
        .await
        .map_err(|e| AppError::Other(format!("OAuth2 client_credentials exchange failed: {e}")))?;
    Ok(to_cached(&token))
}

pub async fn exchange_password(config: &OAuth2Config) -> AppResult<token_store::CachedToken> {
    let client = build_token_client(config)?;
    let http = http_client()?;
    let username = ResourceOwnerUsername::new(config.username.clone());
    let password = ResourceOwnerPassword::new(config.password.clone());
    let mut req = client.exchange_password(&username, &password);
    for scope in scopes_of(config) {
        req = req.add_scope(scope);
    }
    let token = req
        .request_async(&http)
        .await
        .map_err(|e| AppError::Other(format!("OAuth2 password exchange failed: {e}")))?;
    Ok(to_cached(&token))
}

pub async fn exchange_refresh_token(config: &OAuth2Config, refresh_token: &str) -> AppResult<token_store::CachedToken> {
    let client = build_token_client(config)?;
    let http = http_client()?;
    let refresh = RefreshToken::new(refresh_token.to_string());
    let mut req = client.exchange_refresh_token(&refresh);
    for scope in scopes_of(config) {
        req = req.add_scope(scope);
    }
    let token = req
        .request_async(&http)
        .await
        .map_err(|e| AppError::Other(format!("OAuth2 refresh failed: {e}")))?;
    Ok(to_cached(&token))
}

/// Dispatches to the right grant for a from-scratch fetch (no cached/refresh
/// token available yet). `AuthorizationCode` has no headless equivalent — it
/// only ever produces a token via `exchange_code` after the browser redirect.
pub async fn fetch_token(config: &OAuth2Config) -> AppResult<token_store::CachedToken> {
    match config.grant_type {
        OAuth2GrantType::ClientCredentials => exchange_client_credentials(config).await,
        OAuth2GrantType::Password => exchange_password(config).await,
        OAuth2GrantType::RefreshToken => exchange_refresh_token(config, &config.refresh_token).await,
        OAuth2GrantType::AuthorizationCode => {
            Err(AppError::Other("authorization_code grant has no cached token yet — run the browser flow first".into()))
        }
    }
}

/// Picks the local address to bind the loopback HTTP listener that catches
/// the Authorization Code redirect (RFC 8252 native-app pattern). Requires
/// `redirect_uri` to be `http://` with an explicit port on a loopback host
/// (`127.0.0.1`, `::1`, or `localhost`) — that exact URI is what must be
/// registered with the identity provider, so a fixed, predictable port is
/// required rather than letting the OS pick one at random.
pub fn loopback_bind_addr(redirect_uri: &str) -> AppResult<std::net::SocketAddr> {
    let hint = "register http://127.0.0.1:<port>/callback (with an explicit port) as the redirect URI with your identity provider for the desktop browser flow";
    let url = reqwest::Url::parse(redirect_uri).map_err(|_| AppError::Other(format!("invalid redirect URI — {hint}")))?;
    if url.scheme() != "http" {
        return Err(AppError::Other(format!("redirect URI must use http:// — {hint}")));
    }
    // `host_str()` wraps IPv6 literals in brackets (URL syntax to disambiguate
    // from the port separator) — strip them before handing off to
    // `IpAddr::from_str`, which doesn't accept brackets. A no-op for IPv4.
    let ip: std::net::IpAddr = match url.host_str() {
        Some("localhost") => std::net::Ipv4Addr::LOCALHOST.into(),
        Some(h) => h
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse()
            .map_err(|_| AppError::Other(format!("redirect URI host \"{h}\" isn't a loopback address — {hint}")))?,
        None => return Err(AppError::Other(format!("redirect URI is missing a host — {hint}"))),
    };
    if !ip.is_loopback() {
        return Err(AppError::Other(format!("redirect URI host \"{ip}\" isn't a loopback address — {hint}")));
    }
    let port = url.port().ok_or_else(|| AppError::Other(format!("redirect URI is missing an explicit port — {hint}")))?;
    Ok(std::net::SocketAddr::new(ip, port))
}

/// The `code`/`state` pair carried by the redirect the loopback listener
/// catches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallbackResult {
    pub code: String,
    pub state: String,
}

/// Parses the request line of the loopback HTTP callback (e.g.
/// `GET /callback?code=abc&state=xyz HTTP/1.1`). Surfaces an `error`/
/// `error_description` query param — the IdP rejecting the request, e.g. the
/// user clicking "deny" — as a readable error rather than a generic
/// "missing code". Does not check `state` against the expected CSRF token;
/// that comparison is the caller's responsibility.
pub fn parse_callback_request_line(line: &str) -> AppResult<CallbackResult> {
    let malformed = || AppError::Other("malformed OAuth2 callback request".to_string());
    let target = line.split_whitespace().nth(1).ok_or_else(malformed)?;
    let url = reqwest::Url::parse(&format!("http://localhost{target}")).map_err(|_| malformed())?;
    let params: std::collections::HashMap<String, String> = url.query_pairs().into_owned().collect();

    if let Some(err) = params.get("error") {
        let desc = params.get("error_description").map(String::as_str).unwrap_or("");
        return Err(AppError::Other(format!("identity provider denied authorization: {err} {desc}").trim().to_string()));
    }
    let code = params.get("code").filter(|c| !c.is_empty()).ok_or_else(|| AppError::Other("OAuth2 callback missing \"code\"".into()))?;
    let state = params.get("state").filter(|s| !s.is_empty()).ok_or_else(|| AppError::Other("OAuth2 callback missing \"state\"".into()))?;
    Ok(CallbackResult { code: code.clone(), state: state.clone() })
}

#[cfg(test)]
mod loopback_tests {
    use super::*;

    #[test]
    fn loopback_bind_addr_accepts_ipv4_loopback_with_port() {
        let addr = loopback_bind_addr("http://127.0.0.1:43251/callback").unwrap();
        assert_eq!(addr.to_string(), "127.0.0.1:43251");
    }

    #[test]
    fn loopback_bind_addr_accepts_localhost_with_port() {
        let addr = loopback_bind_addr("http://localhost:8080/cb").unwrap();
        assert_eq!(addr.to_string(), "127.0.0.1:8080");
    }

    #[test]
    fn loopback_bind_addr_accepts_ipv6_loopback_with_port() {
        let addr = loopback_bind_addr("http://[::1]:9000/cb").unwrap();
        assert!(addr.is_ipv6());
        assert_eq!(addr.port(), 9000);
    }

    #[test]
    fn loopback_bind_addr_rejects_non_loopback_host() {
        assert!(loopback_bind_addr("http://example.com:8080/cb").is_err());
    }

    #[test]
    fn loopback_bind_addr_rejects_https() {
        assert!(loopback_bind_addr("https://127.0.0.1:8080/cb").is_err());
    }

    #[test]
    fn loopback_bind_addr_rejects_missing_port() {
        assert!(loopback_bind_addr("http://127.0.0.1/cb").is_err());
    }

    #[test]
    fn loopback_bind_addr_rejects_garbage_uri() {
        assert!(loopback_bind_addr("not a url").is_err());
    }

    #[test]
    fn parse_callback_request_line_extracts_code_and_state() {
        let result = parse_callback_request_line("GET /callback?code=abc123&state=xyz789 HTTP/1.1\r\n").unwrap();
        assert_eq!(result, CallbackResult { code: "abc123".into(), state: "xyz789".into() });
    }

    #[test]
    fn parse_callback_request_line_surfaces_idp_error() {
        let err = parse_callback_request_line("GET /callback?error=access_denied&error_description=user+said+no&state=xyz HTTP/1.1\r\n")
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("access_denied"), "expected error to mention access_denied, got: {msg}");
        assert!(msg.contains("user said no"), "expected error to mention the description, got: {msg}");
    }

    #[test]
    fn parse_callback_request_line_errors_on_missing_code() {
        assert!(parse_callback_request_line("GET /callback?state=xyz HTTP/1.1\r\n").is_err());
    }

    #[test]
    fn parse_callback_request_line_errors_on_missing_state() {
        assert!(parse_callback_request_line("GET /callback?code=abc HTTP/1.1\r\n").is_err());
    }

    #[test]
    fn parse_callback_request_line_errors_on_malformed_line() {
        assert!(parse_callback_request_line("garbage").is_err());
    }
}

#[cfg(test)]
mod exchange_tests {
    use super::*;

    /// Exercises the real reqwest round trip — URL build, form encoding via
    /// the oauth2 crate, JSON response parsing — against an in-process TCP
    /// server, same pattern as
    /// `engine::http::tests::sends_over_socket_and_parses_response`. Loopback
    /// only, so it's deterministic in any sandbox without touching a real IdP.
    #[tokio::test]
    async fn exchange_client_credentials_parses_token_response() {
        use std::io::{Read, Write};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf); // consume request line/headers/body
            let body = br#"{"access_token":"tok-abc","token_type":"bearer","expires_in":3600,"scope":"read write"}"#;
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(head.as_bytes()).unwrap();
            stream.write_all(body).unwrap();
            stream.flush().unwrap();
        });

        let config = OAuth2Config {
            token_url: format!("http://{addr}/token"),
            client_id: "cid".into(),
            client_secret: "secret".into(),
            scope: "read write".into(),
            ..Default::default()
        };

        let token = exchange_client_credentials(&config).await.unwrap();
        server.join().unwrap();

        assert_eq!(token.access_token, "tok-abc");
        assert_eq!(token.token_type, "Bearer");
        assert_eq!(token.scope, Some("read write".to_string()));
        assert_eq!(token.refresh_token, None);
        assert!(token.expires_at.is_some());
    }
}
