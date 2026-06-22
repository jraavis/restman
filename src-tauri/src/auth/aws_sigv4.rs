//! AWS SigV4 request signing.
//!
//! A pure function of `(config, method, url, headers, body, time)` — no DB or
//! keychain access — so `engine::http::send` can call it synchronously right
//! before dispatching the request. `time` is an explicit parameter rather
//! than read internally via `SystemTime::now()` so tests can pin it against
//! AWS's published reference vectors, which sign a fixed date.

use crate::error::{AppError, AppResult};
use crate::model::auth::AwsSigV4Config;
use aws_credential_types::Credentials;
use aws_sigv4::http_request::{sign, SignableBody, SignableRequest, SigningSettings};
use aws_sigv4::sign::v4;
use std::time::SystemTime;

/// Returns the `(name, value)` header pairs SigV4 adds for this request —
/// `Authorization`, `X-Amz-Date`, and (only when a session token is set)
/// `X-Amz-Security-Token`. The caller merges these into the outgoing
/// request's headers; this function never mutates anything itself.
pub fn sign_headers(
    config: &AwsSigV4Config,
    method: &str,
    url: &str,
    headers: &[(String, String)],
    body: Option<&[u8]>,
    time: SystemTime,
) -> AppResult<Vec<(String, String)>> {
    let session_token = if config.session_token.is_empty() {
        None
    } else {
        Some(config.session_token.clone())
    };
    let identity = Credentials::new(&config.access_key, &config.secret_key, session_token, None, "restman").into();

    let signing_settings = SigningSettings::default();
    let signing_params = v4::SigningParams::builder()
        .identity(&identity)
        .region(&config.region)
        .name(&config.service)
        .time(time)
        .settings(signing_settings)
        .build()
        .map_err(|e| AppError::Other(format!("AWS SigV4 signing params: {e}")))?
        .into();

    let header_refs: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let signable_body = SignableBody::Bytes(body.unwrap_or(&[]));
    let signable_request = SignableRequest::new(method, url, header_refs.into_iter(), signable_body)
        .map_err(|e| AppError::Other(format!("AWS SigV4 signable request: {e}")))?;

    let (instructions, _signature) = sign(signable_request, &signing_params)
        .map_err(|e| AppError::Other(format!("AWS SigV4 signing failed: {e}")))?
        .into_parts();

    Ok(instructions.headers().map(|(k, v)| (k.to_string(), v.to_string())).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// AWS's own published SigV4 reference vector (`get-vanilla`):
    /// https://docs.aws.amazon.com/IAM/latest/UserGuide/samples/sigv4_aws-sig-v4-test-suite.zip
    /// Fixed date 2015-08-30T12:36:00Z, region us-east-1, service "service",
    /// the well-known test access/secret key pair. Pinning `time` (rather
    /// than `SystemTime::now()`) is what makes this reproducible.
    #[test]
    fn matches_aws_get_vanilla_reference_vector() {
        let config = AwsSigV4Config {
            access_key: "AKIDEXAMPLE".into(),
            secret_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY".into(),
            region: "us-east-1".into(),
            service: "service".into(),
            session_token: String::new(),
        };
        // 2015-08-30T12:36:00Z
        let time = SystemTime::UNIX_EPOCH + Duration::from_secs(1_440_938_160);
        let headers = sign_headers(
            &config,
            "GET",
            "https://example.amazonaws.com/",
            &[("host".to_string(), "example.amazonaws.com".to_string())],
            None,
            time,
        )
        .unwrap();

        let auth = headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("authorization"))
            .map(|(_, v)| v.as_str())
            .expect("Authorization header present");
        assert_eq!(
            auth,
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/service/aws4_request, \
             SignedHeaders=host;x-amz-date, \
             Signature=5fa00fa31553b73ebf1942676e86291e8372ff2a2260956d9b8aae1d763fbf31"
        );
    }
}
