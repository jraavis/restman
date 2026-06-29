//! gRPC dynamic client (Phase 6 task 4, sub-phase 17d). Sub-modules:
//! - `framing`: gRPC length-prefix (de)framing — re-homed from the 17c
//!   throwaway spike into real module code (landed with #24).
//! - `transport` (#25), `reflection` (#26), `schema` (#27): land in their own
//!   files as the corresponding sub-tasks complete.
//!
//! This module replaces the 17c spike (`engine/grpc.rs`). The protox/
//! prost-reflect compilation tests below stay here under `#[cfg(test)]`; the
//! framing helpers and their tests moved to `framing.rs`.
//!
//! ## Unary RPC drive (17d-6 / #28, #30)
//!
//! `call_unary` below is the first piece of "drive a real exchange end to
//! end" code: given a `DescriptorPool` (from either `reflection` or `schema`
//! — both produce the same type, so this function doesn't care which),
//! a `"package.Service/Method"` full name, a JSON request body, and an
//! already-connected `&mut transport::GrpcTransport`, it looks up the
//! method's input/output `MessageDescriptor`s, builds a `DynamicMessage` from
//! the JSON via `prost-reflect`'s `serde` support (newly enabled in
//! `Cargo.toml` — see the doc comment on `call_unary` for why), encodes it,
//! sends it as a single gRPC frame, awaits exactly one response frame (a
//! unary call is one request / one response), decodes it against the output
//! descriptor, and converts it back to JSON.
//!
//! This deliberately takes an already-connected transport rather than
//! connecting itself, mirroring `engine::ws::connect`'s
//! connect-then-let-the-caller-drive split — #31 (streaming RPC modes) reuses
//! the same connected `GrpcTransport` and the same method-lookup/JSON
//! conversion helpers, varying only the send/recv loop shape.

pub(crate) mod framing;
pub(crate) mod reflection;
pub(crate) mod schema;
pub(crate) mod transport;

#[cfg(test)]
mod testsupport;

use prost::Message as _;
use prost_reflect::{DescriptorPool, DynamicMessage, MethodDescriptor};
use serde::de::DeserializeSeed as _;

use crate::error::{AppError, AppResult};
use crate::model::grpc::{grpc_request_path, split_method_full_name};

use self::transport::GrpcTransport;

/// Looks up a `MethodDescriptor` from a `"package.Service/Method"` full name
/// (the form the frontend uses throughout — see `model::grpc`'s doc comment)
/// against a `DescriptorPool`. Shared by `call_unary` and (later) the
/// streaming drive functions in #31, since the lookup itself doesn't depend
/// on the RPC's streaming mode.
#[allow(dead_code)] // caller lands in #29 (Tauri command wiring) / #31 (streaming RPC modes)
pub(crate) fn resolve_method(
    pool: &DescriptorPool,
    method_full_name: &str,
) -> AppResult<MethodDescriptor> {
    let (service_name, method_name) = split_method_full_name(method_full_name).ok_or_else(|| {
        AppError::Other(format!(
            "invalid gRPC method full name (expected \"package.Service/Method\"): {method_full_name}"
        ))
    })?;
    let service = pool.get_service_by_name(service_name).ok_or_else(|| {
        AppError::Other(format!("gRPC service not found in schema: {service_name}"))
    })?;
    let method = service.methods().find(|m| m.name() == method_name);
    method.ok_or_else(|| {
        AppError::Other(format!(
            "gRPC method not found in schema: {method_name} (service {service_name})"
        ))
    })
}

/// Builds a `DynamicMessage` of the given descriptor's type from a JSON
/// request body, using `prost-reflect`'s `serde` support (the
/// `MessageDescriptor: DeserializeSeed` impl) — this is what lets the
/// frontend hand over a plain JSON object built by `GrpcMessageBuilder.tsx`
/// without either side needing a compile-time Rust type for the message.
#[allow(dead_code)] // caller lands in #29 (Tauri command wiring) / #31 (streaming RPC modes)
pub(crate) fn json_to_dynamic_message(
    descriptor: &prost_reflect::MessageDescriptor,
    json: serde_json::Value,
) -> AppResult<DynamicMessage> {
    descriptor
        .clone()
        .deserialize(json)
        .map_err(|e| AppError::Other(format!("request JSON does not match method input type: {e}")))
}

/// Converts a decoded `DynamicMessage` back to JSON for the frontend, again
/// via `prost-reflect`'s `serde` support (`DynamicMessage: Serialize`).
#[allow(dead_code)] // caller lands in #29 (Tauri command wiring) / #31 (streaming RPC modes)
pub(crate) fn dynamic_message_to_json(message: &DynamicMessage) -> AppResult<serde_json::Value> {
    serde_json::to_value(message)
        .map_err(|e| AppError::Other(format!("failed to convert response message to JSON: {e}")))
}

/// The gRPC-level outcome of a completed call: the `grpc-status` code and
/// optional `grpc-message` detail read from the HTTP/2 trailers. Mirrors
/// `GrpcEvent::Status`'s fields — kept as a plain struct here (rather than
/// constructing `model::grpc::GrpcEvent` directly in this module) so
/// `call_unary` stays usable from a future non-Tauri-Channel caller (e.g. a
/// test) without dragging in the IPC event enum's full surface.
#[allow(dead_code)] // caller lands in #29 (Tauri command wiring)
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GrpcStatus {
    pub code: u32,
    pub message: Option<String>,
}

/// Reads `grpc-status`/`grpc-message` out of HTTP/2 trailers. `grpc-status`
/// absent entirely is treated as status 0 (OK) per how `h2`/most gRPC
/// servers behave for a clean stream end with no explicit trailers — though
/// in practice a compliant server always sends `grpc-status` in trailers (or
/// in headers for a Trailers-Only error response, which `transport.rs`
/// doesn't yet expose — see this task's report for that gap).
#[allow(dead_code)] // caller lands in #29 (Tauri command wiring)
fn status_from_trailers(trailers: &http::HeaderMap) -> AppResult<GrpcStatus> {
    let code = match trailers.get("grpc-status") {
        Some(v) => v
            .to_str()
            .map_err(|e| AppError::Other(format!("grpc-status header is not valid UTF-8: {e}")))?
            .parse::<u32>()
            .map_err(|e| AppError::Other(format!("grpc-status header is not a valid u32: {e}")))?,
        None => 0,
    };
    let message = trailers
        .get("grpc-message")
        .map(|v| {
            v.to_str()
                .map(|s| s.to_string())
                .map_err(|e| AppError::Other(format!("grpc-message header is not valid UTF-8: {e}")))
        })
        .transpose()?
        .filter(|s| !s.is_empty());
    Ok(GrpcStatus { code, message })
}

/// The result of a completed unary call: the decoded response message (if
/// the server sent exactly one, which it should for a successful unary RPC —
/// `None` if the call errored before any message arrived) plus the gRPC
/// status read from trailers.
#[allow(dead_code)] // caller lands in #29 (Tauri command wiring)
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct UnaryCallResult {
    pub response: Option<serde_json::Value>,
    pub status: GrpcStatus,
}

/// Drives one complete unary gRPC call over an already-connected transport:
/// resolves the method against `pool`, builds and sends the request message,
/// awaits exactly one response frame, decodes it, and reads the trailing
/// gRPC status. A unary call is defined as one request message and (on
/// success) one response message, so this reads at most one frame before
/// moving on to trailers — a second frame would indicate a server-streaming
/// method was called as if unary, which is a caller error this function
/// doesn't attempt to paper over.
#[allow(dead_code)] // caller lands in #29 (Tauri command wiring)
pub(crate) async fn call_unary(
    pool: &DescriptorPool,
    method_full_name: &str,
    request_json: serde_json::Value,
    transport: &mut GrpcTransport,
) -> AppResult<UnaryCallResult> {
    let method = resolve_method(pool, method_full_name)?;
    let request_message = json_to_dynamic_message(&method.input(), request_json)?;
    let payload = request_message.encode_to_vec();

    let path = grpc_request_path(method_full_name);
    let mut stream = transport.send(&path).await?;
    stream.send_frame(&payload, true)?;

    let status = stream.http_status().await?;
    if !status.is_success() {
        return Err(AppError::Other(format!(
            "gRPC request rejected at the HTTP layer: {status} (expected 200; grpc-status would otherwise carry the RPC-level result)"
        )));
    }

    let response = match stream.recv_frame().await? {
        Some(bytes) => {
            let output_desc = method.output();
            let decoded = DynamicMessage::decode(output_desc, bytes.as_slice()).map_err(|e| {
                AppError::Other(format!("failed to decode gRPC response message: {e}"))
            })?;
            Some(dynamic_message_to_json(&decoded)?)
        }
        None => None,
    };

    let trailers = stream.recv_trailers().await?.unwrap_or_default();
    let grpc_status = status_from_trailers(&trailers)?;

    Ok(UnaryCallResult {
        response,
        status: grpc_status,
    })
}

#[cfg(test)]
mod tests {
    use super::testsupport::{compile_reflection_proto, reflection_request_descriptor, FIXTURES_ROOT};
    use prost::Message as _;
    use prost_reflect::{DynamicMessage, Value};

    #[test]
    fn protox_compiles_self_contained_reflection_proto() {
        let fds = compile_reflection_proto();
        assert!(fds
            .file
            .iter()
            .any(|f| f.name.as_deref() == Some("reflection.proto")));
    }

    #[test]
    fn protox_resolves_relative_import_within_include_dir() {
        // importer/main.proto imports "common/shared.proto" — both fixtures
        // sit under one include root, so this must compile clean.
        let fds = protox::compile(["importer/main.proto"], [FIXTURES_ROOT])
            .expect("import should resolve when common/ is under the same include root");
        let names: Vec<&str> = fds.file.iter().filter_map(|f| f.name.as_deref()).collect();
        assert!(names.contains(&"importer/main.proto"));
        assert!(names.contains(&"common/shared.proto"));
    }

    #[test]
    fn protox_errors_on_missing_import_rather_than_hanging() {
        // Same importer/main.proto, but the include root is narrowed to just
        // the importer/ directory — "common/shared.proto" can't be found.
        // Negative case proving import resolution fails closed on a local
        // lookup miss rather than (say) trying the network; see the
        // `cargo tree` check in PLAN.md #17c.
        let narrow_root = format!("{FIXTURES_ROOT}/importer");
        let result = protox::compile(["main.proto"], [narrow_root]);
        assert!(
            result.is_err(),
            "expected a clean Err when common/shared.proto isn't reachable, got Ok"
        );
    }

    #[test]
    fn descriptor_pool_builds_from_compiled_file_descriptor_set() {
        let desc = reflection_request_descriptor();
        assert_eq!(
            desc.full_name(),
            "grpc.reflection.v1.ServerReflectionRequest"
        );
    }

    #[test]
    fn dynamic_message_round_trips_through_encode_decode() {
        let desc = reflection_request_descriptor();

        let mut msg = DynamicMessage::new(desc.clone());
        msg.set_field_by_name("host", Value::String("example.com".to_string()));
        msg.set_field_by_name("list_services", Value::String("*".to_string()));

        let bytes = msg.encode_to_vec();
        let decoded = DynamicMessage::decode(desc, bytes.as_slice())
            .expect("encoded DynamicMessage bytes should decode cleanly");

        assert_eq!(
            decoded.get_field_by_name("host").unwrap().as_str(),
            Some("example.com")
        );
        assert_eq!(
            decoded.get_field_by_name("list_services").unwrap().as_str(),
            Some("*")
        );
    }
}

/// Tests for the unary RPC drive (17d-6). All offline — no live gRPC server
/// is reachable in this sandbox (confirmed repeatedly elsewhere in this
/// codebase), so these prove two things separately: (1) the JSON <->
/// `DynamicMessage` conversion + method-lookup logic against the vendored
/// `reflection.proto` fixture descriptors, with no network involved at all;
/// and (2) the full `call_unary` drive function end-to-end against a
/// loopback h2 server, mirroring `transport.rs`'s existing
/// `loopback_h2_round_trip_sends_frame_and_reads_status_trailers` test
/// pattern exactly (self-signed TLS via `rcgen`, `GrpcTransport::drive` over
/// the resulting `TlsStream` rather than `connect()`, which would reject a
/// self-signed cert).
#[cfg(test)]
mod unary_call_tests {
    use super::*;
    use crate::engine::grpc::testsupport::{compile_reflection_proto, reflection_request_descriptor};
    use prost_reflect::DescriptorPool as TestDescriptorPool;
    use prost_reflect::Value;

    // --- Pure JSON <-> DynamicMessage conversion (no network at all) -------

    #[test]
    fn json_round_trips_into_dynamic_message_and_back() {
        let desc = reflection_request_descriptor();
        let json = serde_json::json!({
            "host": "example.com",
            "listServices": "*",
        });

        let message = json_to_dynamic_message(&desc, json.clone())
            .expect("ServerReflectionRequest-shaped JSON should match the descriptor");
        assert_eq!(
            message.get_field_by_name("host").unwrap().as_str(),
            Some("example.com")
        );
        assert_eq!(
            message.get_field_by_name("list_services").unwrap().as_str(),
            Some("*")
        );

        let round_tripped =
            dynamic_message_to_json(&message).expect("decoded message should convert back to JSON");
        assert_eq!(round_tripped, json);
    }

    #[test]
    fn json_to_dynamic_message_rejects_unknown_shape() {
        let desc = reflection_request_descriptor();
        // `host` here is a number, not a string — the descriptor requires a
        // string, so this must fail closed rather than silently coerce.
        let json = serde_json::json!({ "host": 12345 });
        let err = json_to_dynamic_message(&desc, json)
            .expect_err("a type-mismatched field should fail conversion");
        assert!(err.to_string().contains("does not match"));
    }

    #[test]
    fn dynamic_message_encode_decode_round_trips_through_wire_bytes() {
        // Proves the encode_to_vec() -> DynamicMessage::decode() path
        // call_unary uses for the request/response payloads, independent of
        // the gRPC framing/transport layer entirely.
        let desc = reflection_request_descriptor();
        let mut msg = DynamicMessage::new(desc.clone());
        msg.set_field_by_name("host", Value::String("round-trip.example".to_string()));

        let bytes = msg.encode_to_vec();
        let decoded = DynamicMessage::decode(desc, bytes.as_slice())
            .expect("encoded bytes should decode back into a DynamicMessage");
        let json = dynamic_message_to_json(&decoded).expect("decoded message should convert to JSON");
        assert_eq!(json["host"], "round-trip.example");
    }

    // --- Method resolution (no network) -------------------------------

    fn reflection_pool() -> DescriptorPool {
        let fds = compile_reflection_proto();
        TestDescriptorPool::from_file_descriptor_set(fds)
            .expect("compiled reflection.proto descriptor set should be valid")
    }

    #[test]
    fn resolve_method_finds_method_by_slash_separated_full_name() {
        let pool = reflection_pool();
        let method = resolve_method(&pool, "grpc.reflection.v1.ServerReflection/ServerReflectionInfo")
            .expect("ServerReflectionInfo should resolve");
        assert_eq!(method.name(), "ServerReflectionInfo");
        assert_eq!(
            method.input().full_name(),
            "grpc.reflection.v1.ServerReflectionRequest"
        );
        assert_eq!(
            method.output().full_name(),
            "grpc.reflection.v1.ServerReflectionResponse"
        );
    }

    #[test]
    fn resolve_method_errors_on_unknown_service() {
        let pool = reflection_pool();
        let err = resolve_method(&pool, "nope.Nothing/Method")
            .expect_err("unknown service should fail closed");
        assert!(err.to_string().contains("service not found"));
    }

    #[test]
    fn resolve_method_errors_on_unknown_method() {
        let pool = reflection_pool();
        let err = resolve_method(&pool, "grpc.reflection.v1.ServerReflection/Nope")
            .expect_err("unknown method should fail closed");
        assert!(err.to_string().contains("method not found"));
    }

    #[test]
    fn resolve_method_errors_on_malformed_full_name_without_slash() {
        let pool = reflection_pool();
        let err = resolve_method(&pool, "no-slash-here")
            .expect_err("a full name without a '/' should fail closed");
        assert!(err.to_string().contains("invalid gRPC method full name"));
    }

    // --- status_from_trailers (no network) -----------------------------

    #[test]
    fn status_from_trailers_reads_code_and_message() {
        let mut trailers = http::HeaderMap::new();
        trailers.insert("grpc-status", "5".parse().unwrap());
        trailers.insert("grpc-message", "not found".parse().unwrap());
        let status = status_from_trailers(&trailers).expect("trailers should parse");
        assert_eq!(status.code, 5);
        assert_eq!(status.message, Some("not found".to_string()));
    }

    #[test]
    fn status_from_trailers_defaults_to_ok_when_absent() {
        let trailers = http::HeaderMap::new();
        let status = status_from_trailers(&trailers).expect("empty trailers should still parse");
        assert_eq!(status.code, 0);
        assert_eq!(status.message, None);
    }

    #[test]
    fn status_from_trailers_drops_empty_message() {
        let mut trailers = http::HeaderMap::new();
        trailers.insert("grpc-status", "0".parse().unwrap());
        trailers.insert("grpc-message", "".parse().unwrap());
        let status = status_from_trailers(&trailers).expect("trailers should parse");
        assert_eq!(status.code, 0);
        assert_eq!(status.message, None);
    }

    // --- Full end-to-end drive over a loopback h2+TLS server ------------
    //
    // Mirrors transport.rs's loopback_h2_round_trip test pattern exactly: a
    // self-signed cert (rcgen, dev-only dep), a minimal h2 server accepting
    // one stream, GrpcTransport::drive() (not connect(), which would reject
    // the self-signed cert) over the resulting client TlsStream. Uses a
    // small synthetic unary proto (compiled in-process via protox, no real
    // filesystem/network dependency beyond the in-memory source string)
    // rather than reflection.proto's bidi-streaming service, so the method
    // descriptor's streaming flags are genuinely unary.

    use std::sync::Arc;
    use bytes::Bytes;
    use http::HeaderMap;
    use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
    use rustls::server::ServerConfig;
    use rustls::{ClientConfig, RootCertStore};
    use tokio::net::{TcpListener, TcpStream};
    use tokio_rustls::{TlsAcceptor, TlsConnector};

    const UNARY_PROTO: &str = r#"
        syntax = "proto3";
        package unarytest;
        service Greeter {
          rpc SayHello (HelloRequest) returns (HelloResponse);
        }
        message HelloRequest {
          string name = 1;
        }
        message HelloResponse {
          string message = 1;
        }
    "#;

    fn unary_test_pool() -> DescriptorPool {
        // Reuses schema::compile_proto_set (an in-memory file-set -> pool
        // compiler, already proven in schema.rs's own tests) rather than
        // protox::compile, which only takes real filesystem include roots —
        // this synthetic proto has no file on disk at all.
        let mut files = super::schema::ProtoFileSet::new();
        files.insert("greeter.proto".to_string(), UNARY_PROTO.to_string());
        super::schema::compile_proto_set(&files, &["greeter.proto".to_string()])
            .expect("synthetic unary proto should compile")
    }

    fn self_signed_cert() -> (CertificateDer<'static>, PrivateKeyDer<'static>) {
        let params = rcgen::CertificateParams::new(vec!["localhost".to_string()])
            .expect("rcgen: subject alt names");
        let key_pair = rcgen::KeyPair::generate().expect("rcgen: generate keypair");
        let cert = params
            .self_signed(&key_pair)
            .expect("rcgen: self-signed cert");
        let cert_der = cert.der().clone();
        let key = PrivateKeyDer::Pkcs8(key_pair.serialize_der().to_vec().into());
        (cert_der, key)
    }

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

    fn server_config(cert: CertificateDer<'static>, key: PrivateKeyDer<'static>) -> Arc<ServerConfig> {
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

    #[tokio::test(flavor = "current_thread")]
    async fn call_unary_round_trips_request_and_response_over_loopback_h2() {
        let pool = unary_test_pool();
        let (cert, key) = self_signed_cert();
        let server_cfg = server_config(cert.clone(), key);
        let client_cfg = test_client_config(cert);

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local_addr");

        let server = tokio::spawn(async move {
            let (sock, _) = listener.accept().await.expect("server accept tcp");
            let acceptor = TlsAcceptor::from(server_cfg);
            let tls = acceptor.accept(sock).await.expect("server tls handshake");

            let mut conn = h2::server::handshake(tls).await.expect("server h2 handshake");
            let (request, mut respond) = conn
                .accept()
                .await
                .expect("server should see an incoming stream")
                .expect("server accept should not error");
            tokio::spawn(async move { while conn.accept().await.is_some() {} });

            assert_eq!(request.uri().path(), "/unarytest.Greeter/SayHello");

            // Read the client's request frame, decode it as a HelloRequest
            // (length-prefix stripped manually here since the server side
            // doesn't go through FrameUnframer), and build a HelloResponse
            // that echoes the name back.
            let mut body = request.into_body();
            let mut received = Vec::new();
            while let Some(chunk) = body.data().await {
                let chunk = chunk.expect("server body read");
                received.extend_from_slice(&chunk);
                let _ = body.flow_control().release_capacity(chunk.len());
            }
            // Strip the 5-byte gRPC length-prefix header manually.
            let payload = &received[5..];
            let pool = unary_test_pool();
            let req_desc = pool.get_message_by_name("unarytest.HelloRequest").unwrap();
            let req_msg = DynamicMessage::decode(req_desc, payload).expect("decode HelloRequest");
            let name = req_msg
                .get_field_by_name("name")
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_default();

            let resp_desc = pool.get_message_by_name("unarytest.HelloResponse").unwrap();
            let mut resp_msg = DynamicMessage::new(resp_desc);
            resp_msg.set_field_by_name("message", Value::String(format!("Hello, {name}!")));
            let resp_bytes = resp_msg.encode_to_vec();

            let response = http::Response::builder()
                .status(200)
                .body(())
                .expect("server response head");
            let mut send_stream = respond
                .send_response(response, false)
                .expect("server send_response");
            send_stream
                .send_data(Bytes::from(super::framing::frame(&resp_bytes)), false)
                .expect("server send_data");

            let mut trailers = HeaderMap::new();
            trailers.insert("grpc-status", "0".parse().unwrap());
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

        let request_json = serde_json::json!({ "name": "world" });
        let result = call_unary(
            &pool,
            "unarytest.Greeter/SayHello",
            request_json,
            &mut transport,
        )
        .await
        .expect("call_unary should drive the full request/response exchange");

        assert_eq!(result.status.code, 0);
        assert_eq!(
            result.response,
            Some(serde_json::json!({ "message": "Hello, world!" }))
        );

        server.await.expect("server task did not finish cleanly");
    }

    /// Closes a verification gap the happy-path test above can't see: that
    /// test's server always sends `grpc-status: 0`, so a `call_unary` bug
    /// that silently defaulted to `code: 0` without ever actually reading
    /// trailers (e.g. if `h2` needed `recv_frame` drained to `None` first,
    /// the way `transport.rs`'s own loopback test deliberately does before
    /// reading trailers) would pass that test too. Here the server sends NO
    /// response message at all (a realistic error shape — many gRPC servers
    /// fail before producing any output) and a non-zero `grpc-status` +
    /// `grpc-message` in trailers; asserting the client observes that exact
    /// non-zero code/message proves trailers are genuinely read off the wire,
    /// not defaulted.
    #[tokio::test(flavor = "current_thread")]
    async fn call_unary_surfaces_non_zero_grpc_status_from_trailers() {
        let pool = unary_test_pool();
        let (cert, key) = self_signed_cert();
        let server_cfg = server_config(cert.clone(), key);
        let client_cfg = test_client_config(cert);

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local_addr");

        let server = tokio::spawn(async move {
            let (sock, _) = listener.accept().await.expect("server accept tcp");
            let acceptor = TlsAcceptor::from(server_cfg);
            let tls = acceptor.accept(sock).await.expect("server tls handshake");

            let mut conn = h2::server::handshake(tls).await.expect("server h2 handshake");
            let (request, mut respond) = conn
                .accept()
                .await
                .expect("server should see an incoming stream")
                .expect("server accept should not error");
            tokio::spawn(async move { while conn.accept().await.is_some() {} });

            // Drain the client's request body (still required even though
            // this server never answers with a message) before responding.
            let mut body = request.into_body();
            while let Some(chunk) = body.data().await {
                let chunk = chunk.expect("server body read");
                let _ = body.flow_control().release_capacity(chunk.len());
            }

            // Respond with headers but no data frame at all, then a
            // non-OK grpc-status/grpc-message pair in trailers — simulating
            // a server that fails the RPC without producing any response
            // message (e.g. NOT_FOUND = 5).
            let response = http::Response::builder()
                .status(200)
                .body(())
                .expect("server response head");
            let mut send_stream = respond
                .send_response(response, false)
                .expect("server send_response");

            let mut trailers = HeaderMap::new();
            trailers.insert("grpc-status", "5".parse().unwrap());
            trailers.insert("grpc-message", "no such greeting".parse().unwrap());
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

        let request_json = serde_json::json!({ "name": "world" });
        let result = call_unary(
            &pool,
            "unarytest.Greeter/SayHello",
            request_json,
            &mut transport,
        )
        .await
        .expect("call_unary should complete even when the RPC itself reports an error status");

        assert_eq!(result.response, None, "no response message was ever sent");
        assert_eq!(
            result.status.code, 5,
            "trailers must be genuinely read off the wire, not defaulted to 0"
        );
        assert_eq!(
            result.status.message,
            Some("no such greeting".to_string())
        );

        server.await.expect("server task did not finish cleanly");
    }

    /// The genuinely discriminating case the two tests above don't cover:
    /// the success-path test always sends `grpc-status: 0` (indistinguishable
    /// from `status_from_trailers`'s absent-trailers default), and the
    /// non-zero-status test above sends *no* DATA frame at all, so
    /// `recv_frame` hits end-of-stream on its very first poll — it doesn't
    /// prove trailers are read correctly when a response message *was* sent
    /// first. `transport.rs`'s own loopback test treats "drain recv_frame to
    /// `None` before reading trailers" as a precondition; `call_unary` reads
    /// only one frame and moves straight to `recv_trailers` without that
    /// drain. This test sends both a response message AND a non-zero status,
    /// so a regression where the data-frame path silently defaults to status
    /// 0 (trailers never actually resolving because the body wasn't drained)
    /// would be caught here.
    #[tokio::test(flavor = "current_thread")]
    async fn call_unary_surfaces_non_zero_status_after_a_response_message() {
        let pool = unary_test_pool();
        let (cert, key) = self_signed_cert();
        let server_cfg = server_config(cert.clone(), key);
        let client_cfg = test_client_config(cert);

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local_addr");

        let server = tokio::spawn(async move {
            let (sock, _) = listener.accept().await.expect("server accept tcp");
            let acceptor = TlsAcceptor::from(server_cfg);
            let tls = acceptor.accept(sock).await.expect("server tls handshake");

            let mut conn = h2::server::handshake(tls).await.expect("server h2 handshake");
            let (request, mut respond) = conn
                .accept()
                .await
                .expect("server should see an incoming stream")
                .expect("server accept should not error");
            tokio::spawn(async move { while conn.accept().await.is_some() {} });

            let mut body = request.into_body();
            while let Some(chunk) = body.data().await {
                let chunk = chunk.expect("server body read");
                let _ = body.flow_control().release_capacity(chunk.len());
            }

            // Send a real response message (unlike the no-data test above)
            // AND a non-zero grpc-status in trailers — a server that
            // produces output but still reports the RPC as failed (this
            // does happen in practice, e.g. partial results + an error).
            let pool = unary_test_pool();
            let resp_desc = pool.get_message_by_name("unarytest.HelloResponse").unwrap();
            let mut resp_msg = DynamicMessage::new(resp_desc);
            resp_msg.set_field_by_name("message", Value::String("partial".to_string()));
            let resp_bytes = resp_msg.encode_to_vec();

            let response = http::Response::builder()
                .status(200)
                .body(())
                .expect("server response head");
            let mut send_stream = respond
                .send_response(response, false)
                .expect("server send_response");
            send_stream
                .send_data(Bytes::from(super::framing::frame(&resp_bytes)), false)
                .expect("server send_data");

            let mut trailers = HeaderMap::new();
            trailers.insert("grpc-status", "5".parse().unwrap());
            trailers.insert("grpc-message", "no such greeting".parse().unwrap());
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

        let request_json = serde_json::json!({ "name": "world" });
        let result = call_unary(
            &pool,
            "unarytest.Greeter/SayHello",
            request_json,
            &mut transport,
        )
        .await
        .expect("call_unary should complete even when a message precedes an error status");

        assert_eq!(
            result.response,
            Some(serde_json::json!({ "message": "partial" })),
            "the response message that was sent should still be decoded"
        );
        assert_eq!(
            result.status.code, 5,
            "trailers after a data frame must be genuinely read, not defaulted to 0"
        );
        assert_eq!(
            result.status.message,
            Some("no such greeting".to_string())
        );

        server.await.expect("server task did not finish cleanly");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn call_unary_errors_cleanly_on_unknown_method() {
        let pool = unary_test_pool();
        let (cert, key) = self_signed_cert();
        let server_cfg = server_config(cert.clone(), key);
        let client_cfg = test_client_config(cert);

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local_addr");

        // No server-side logic needed: resolve_method fails before any
        // network I/O happens, so there's nothing for a server to accept.
        let server = tokio::spawn(async move {
            let (sock, _) = listener.accept().await.expect("server accept tcp");
            let acceptor = TlsAcceptor::from(server_cfg);
            let _tls = acceptor.accept(sock).await.expect("server tls handshake");
            // Intentionally do nothing further — the client should never
            // get far enough to need a response.
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

        let request_json = serde_json::json!({ "name": "world" });
        let err = call_unary(
            &pool,
            "unarytest.Greeter/NoSuchMethod",
            request_json,
            &mut transport,
        )
        .await
        .expect_err("unknown method should fail before any network I/O");
        assert!(err.to_string().contains("method not found"));

        drop(transport);
        server.abort();
        let _ = server.await;
    }
}