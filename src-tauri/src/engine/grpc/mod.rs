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
//! connect-then-let-the-caller-drive split — `drive_streaming_call` (17d-7,
//! below `call_unary` in this file) reuses the same connected
//! `GrpcTransport` and the same method-lookup/JSON conversion helpers,
//! varying only the send/recv loop shape.
//!
//! ## Streaming RPC modes (17d-7 / #31)
//!
//! `drive_streaming_call` covers client-streaming, server-streaming, and
//! bidi with one full-duplex `tokio::select!` loop (see its own doc comment
//! for the shape), modeled on `commands/streaming.rs::run_ws`'s drive loop
//! but kept Tauri-agnostic: it takes plain `tokio::sync::mpsc` channels for
//! events out / requests in rather than a `tauri::ipc::Channel`, so it can
//! run under an offline test the same way `call_unary` does. #29/17d-8 wires
//! a `Channel<GrpcEvent>` and a `grpc_send`-style command around these mpsc
//! channels — see `drive_streaming_call`'s doc comment for the exact split.
//!
//! This sub-phase also closes a gap flagged in 17d-6's report: gRPC's
//! "Trailers-Only" response shape (the server reports `grpc-status` directly
//! in the response HEADERS frame, with no DATA frame and no separate
//! trailers frame — used for immediate errors like "method not found", and
//! exactly the shape a server-streaming call gets if it errors before
//! sending any message) previously read as a silent status-0 success because
//! `transport::GrpcStream` had no way to expose the response headers.
//! `transport::GrpcStream::response_headers` (new) and
//! `resolve_call_status` (new, used by both `call_unary` and
//! `drive_streaming_call`) fix this: trailers are read first, falling back
//! to headers only when trailers carry no `grpc-status` at all.

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
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GrpcStatus {
    pub code: u32,
    pub message: Option<String>,
}

/// Reads `grpc-status`/`grpc-message` out of an HTTP/2 header map (either the
/// real trailers, or the response HEADERS frame itself for a Trailers-Only
/// response — see `resolve_call_status`, which is what actually decides
/// which one to read). `grpc-status` absent from the given map is treated as
/// status 0 (OK) — callers needing the Trailers-Only fallback go through
/// `resolve_call_status` rather than calling this directly on trailers alone.
fn status_from_header_map(headers: &http::HeaderMap) -> AppResult<GrpcStatus> {
    let code = match headers.get("grpc-status") {
        Some(v) => v
            .to_str()
            .map_err(|e| AppError::Other(format!("grpc-status header is not valid UTF-8: {e}")))?
            .parse::<u32>()
            .map_err(|e| AppError::Other(format!("grpc-status header is not a valid u32: {e}")))?,
        None => 0,
    };
    let message = headers
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

/// Resolves the gRPC-level status for a stream whose body has already been
/// fully drained (`recv_frame` returned `Ok(None)`): reads the real HTTP/2
/// trailers first, and if those carry no `grpc-status` at all, falls back to
/// the response HEADERS frame — covering a "Trailers-Only" response, where a
/// server reports an immediate error (most commonly "method not found")
/// directly in headers with no DATA frame and no separate trailers frame.
/// Without this fallback, a Trailers-Only error would silently read as
/// status 0 (OK), hiding a real RPC failure — the exact gap flagged in
/// 17d-6's report and now fixed via `transport::GrpcStream::response_headers`
/// (added in 17d-7 specifically to close it). Used by both `call_unary` and
/// the streaming drive loop in #31, since both end the same way: drain
/// frames, then resolve one final status.
async fn resolve_call_status(stream: &mut transport::GrpcStream) -> AppResult<GrpcStatus> {
    let trailers = stream.recv_trailers().await?.unwrap_or_default();
    if trailers.contains_key("grpc-status") {
        return status_from_header_map(&trailers);
    }
    let headers = stream.response_headers().await?;
    status_from_header_map(headers)
}

/// The result of a completed unary call: the decoded response message (if
/// the server sent exactly one, which it should for a successful unary RPC —
/// `None` if the call errored before any message arrived) plus the gRPC
/// status read from trailers.
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

    let grpc_status = resolve_call_status(&mut stream).await?;

    Ok(UnaryCallResult {
        response,
        status: grpc_status,
    })
}

// --- Streaming RPC modes (17d-7 / #31) --------------------------------
//
// `call_unary` above stays exactly as it was (it's tested byte-for-byte and
// a unary call's "one request, one response" shape doesn't benefit from a
// generic loop). Client-streaming, server-streaming, and bidi all share one
// full-duplex drive loop instead, since they're really the same shape with
// different cardinalities on each side: client-streaming is N requests / 1
// response, server-streaming is 1 request / N responses, bidi is N/M with
// both sides live concurrently. `drive_streaming_call` below covers all
// three by always running the same `tokio::select!` over inbound frames and
// an outbound request channel — a caller that only ever sends one request
// (server-streaming) or only ever receives one response (client-streaming)
// is just a degenerate case of the general loop, not a separate code path.
//
// This loop is deliberately Tauri-agnostic, same as `engine::ws`/
// `engine::sse`: it takes a plain `mpsc::UnboundedSender<GrpcEvent>` for
// outbound events and a plain `mpsc::UnboundedReceiver<serde_json::Value>`
// for inbound request messages from the caller, rather than a
// `tauri::ipc::Channel`. `commands/streaming.rs`'s `run_ws` is `Channel`-
// driven, but `Channel` only exists in the command layer — the engine layer
// has zero Tauri imports throughout this codebase (`engine::ws::connect`,
// `engine::sse::open`, etc.), and more importantly a `Channel`-shaped
// signature couldn't be driven by an offline test at all, which this task
// requires. #29/17d-8 bridges this: a `grpc_call` command wraps the
// `Channel<GrpcEvent>` it receives around the event mpsc here, and a
// `grpc_send` command (mirroring `ws_send`) feeds the request mpsc keyed by
// connection id. Tests below drain the event receiver into a `Vec` directly.

/// One request message to send on an already-open streaming call, or a
/// signal that the caller is done sending (`None` half-closes the request
/// side without sending the in-flight message it would otherwise carry).
/// `drive_streaming_call`'s request channel carries `Option<Value>` rather
/// than bare `Value` so a sender can explicitly half-close (client-streaming/
/// bidi's "no more requests") without needing a sentinel JSON value or a
/// second channel.
pub(crate) type GrpcRequestMsg = Option<serde_json::Value>;

/// Drives one streaming gRPC call (client-streaming, server-streaming, or
/// bidi — unary uses `call_unary` instead) over an already-connected
/// transport, full-duplex: concurrently sends every request message that
/// arrives on `requests` and emits a `GrpcEvent` for every response message,
/// the final status, and connection lifecycle, until either side closes.
///
/// Mirrors `commands/streaming.rs::run_ws`'s `tokio::select!` shape exactly
/// (read it for the WS precedent this follows): one arm awaits the next
/// inbound frame (`stream.recv_frame()`), the other awaits the next outbound
/// item (`requests.recv()`); each iteration borrows `stream`/`requests`
/// independently, and `select!` drops the losing future before running the
/// winning arm's body, so e.g. the outbound arm's `stream.send_frame(...)`
/// never races the inbound arm's `stream.recv_frame()` — only one half of
/// `stream` is touched per iteration.
///
/// - Server-streaming: the caller sends exactly one request then drops (or
///   never refills) the sending half of `requests`, so the outbound arm's
///   `None` case fires once and the loop becomes inbound-only — this
///   function doesn't need a separate code path for it.
/// - Client-streaming: the server doesn't write a response until the request
///   stream half-closes, so the inbound arm naturally waits; once `requests`
///   yields `None` (no more messages), `stream.half_close()` is called and
///   inbound reads continue alone until the single response + status arrive.
/// - Bidi: both arms stay live throughout; whichever side has data ready
///   fires first, same as `run_ws`'s read/write interleaving.
///
/// `events` errors (the receiver was dropped) end the loop silently, same as
/// `run_ws` treating a failed `channel.send` as "no one is listening
/// anymore, stop driving" rather than a hard failure.
///
/// No "Sent"/ack `GrpcEvent` variant exists for an outbound message: unlike
/// `WsOutbound` (where a send is fire-and-forget over a socket with no
/// protocol-level ack), gRPC's wire protocol genuinely has nothing to ack a
/// client message with mid-stream — the only signal the wire ever produces
/// is the final response(s)/status. A UI wanting to show "message N sent"
/// can derive that from having called the send command itself; there's no
/// information from the transport to relay back, so adding an event for it
/// would just be restating the caller's own input.
pub(crate) async fn drive_streaming_call(
    pool: &DescriptorPool,
    method_full_name: &str,
    transport: &mut GrpcTransport,
    mut requests: tokio::sync::mpsc::UnboundedReceiver<GrpcRequestMsg>,
    events: tokio::sync::mpsc::UnboundedSender<crate::model::grpc::GrpcEvent>,
) {
    use crate::model::grpc::GrpcEvent;

    let method = match resolve_method(pool, method_full_name) {
        Ok(m) => m,
        Err(e) => {
            let _ = events.send(GrpcEvent::Error {
                message: e.to_string(),
            });
            return;
        }
    };

    let path = grpc_request_path(method_full_name);
    let mut stream = match transport.send(&path).await {
        Ok(s) => s,
        Err(e) => {
            let _ = events.send(GrpcEvent::Error {
                message: e.to_string(),
            });
            return;
        }
    };
    if events.send(GrpcEvent::Open).is_err() {
        return;
    }

    // True once the request side has half-closed (either `requests` was
    // drained to `None`, or it was dropped outright) — once that happens the
    // outbound arm of the select loop below is permanently disabled (an
    // already-closed `mpsc::UnboundedReceiver::recv()` would otherwise spin
    // ready-but-`None` forever) and only inbound reads continue.
    let mut sending_done = false;

    loop {
        tokio::select! {
            incoming = stream.recv_frame() => {
                match incoming {
                    Ok(Some(bytes)) => {
                        let output_desc = method.output();
                        let decoded = match DynamicMessage::decode(output_desc, bytes.as_slice()) {
                            Ok(d) => d,
                            Err(e) => {
                                let _ = events.send(GrpcEvent::Error {
                                    message: format!("failed to decode gRPC response message: {e}"),
                                });
                                return;
                            }
                        };
                        let json = match dynamic_message_to_json(&decoded) {
                            Ok(j) => j,
                            Err(e) => {
                                let _ = events.send(GrpcEvent::Error { message: e.to_string() });
                                return;
                            }
                        };
                        if events.send(GrpcEvent::Response { message: json }).is_err() {
                            return;
                        }
                    }
                    Ok(None) => {
                        // Response body exhausted: resolve the final status
                        // (trailers, falling back to headers for a
                        // Trailers-Only response) and end the call.
                        let status = match resolve_call_status(&mut stream).await {
                            Ok(s) => s,
                            Err(e) => {
                                let _ = events.send(GrpcEvent::Error { message: e.to_string() });
                                return;
                            }
                        };
                        if events
                            .send(GrpcEvent::Status {
                                code: status.code,
                                message: status.message,
                            })
                            .is_err()
                        {
                            return;
                        }
                        let _ = events.send(GrpcEvent::Closed);
                        return;
                    }
                    Err(e) => {
                        let _ = events.send(GrpcEvent::Error { message: e.to_string() });
                        return;
                    }
                }
            }
            outgoing = requests.recv(), if !sending_done => {
                match outgoing {
                    Some(Some(json)) => {
                        let message = match json_to_dynamic_message(&method.input(), json) {
                            Ok(m) => m,
                            Err(e) => {
                                let _ = events.send(GrpcEvent::Error { message: e.to_string() });
                                return;
                            }
                        };
                        let payload = message.encode_to_vec();
                        if let Err(e) = stream.send_frame(&payload, false) {
                            let _ = events.send(GrpcEvent::Error { message: e.to_string() });
                            return;
                        }
                    }
                    Some(None) | None => {
                        // Explicit half-close request, or the sender was
                        // dropped outright — either way, no more requests
                        // are coming. A true HTTP/2 half-close (zero-length
                        // DATA frame + END_STREAM), never a framed empty gRPC
                        // message — see `GrpcStream::half_close`'s doc
                        // comment for why `send_frame(&[], true)` would be
                        // wrong here.
                        if let Err(e) = stream.half_close() {
                            let _ = events.send(GrpcEvent::Error { message: e.to_string() });
                            return;
                        }
                        sending_done = true;
                    }
                }
            }
        }
    }
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

    // --- status_from_header_map (no network) ----------------------------

    #[test]
    fn status_from_header_map_reads_code_and_message() {
        let mut trailers = http::HeaderMap::new();
        trailers.insert("grpc-status", "5".parse().unwrap());
        trailers.insert("grpc-message", "not found".parse().unwrap());
        let status = status_from_header_map(&trailers).expect("trailers should parse");
        assert_eq!(status.code, 5);
        assert_eq!(status.message, Some("not found".to_string()));
    }

    #[test]
    fn status_from_header_map_defaults_to_ok_when_absent() {
        let trailers = http::HeaderMap::new();
        let status = status_from_header_map(&trailers).expect("empty trailers should still parse");
        assert_eq!(status.code, 0);
        assert_eq!(status.message, None);
    }

    #[test]
    fn status_from_header_map_drops_empty_message() {
        let mut trailers = http::HeaderMap::new();
        trailers.insert("grpc-status", "0".parse().unwrap());
        trailers.insert("grpc-message", "".parse().unwrap());
        let status = status_from_header_map(&trailers).expect("trailers should parse");
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

    /// Proves `call_unary` itself (not just `transport.rs` in isolation)
    /// surfaces a Trailers-Only response correctly via `resolve_call_status`:
    /// the server reports `grpc-status` directly in the HEADERS frame, with
    /// no DATA frame and no separate trailers frame — without the headers
    /// fallback added in 17d-7, `recv_trailers` would return `None` and the
    /// old `status_from_trailers(&HeaderMap::default())` call would silently
    /// report status 0 (OK), hiding the real error.
    #[tokio::test(flavor = "current_thread")]
    async fn call_unary_surfaces_trailers_only_status_via_headers_fallback() {
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

            // Trailers-Only: status/message in the HEADERS frame itself,
            // end_of_stream = true, no DATA frame, no separate trailers.
            let response = http::Response::builder()
                .status(200)
                .header("grpc-status", "5")
                .header("grpc-message", "no such greeting")
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

        let request_json = serde_json::json!({ "name": "world" });
        let result = call_unary(
            &pool,
            "unarytest.Greeter/SayHello",
            request_json,
            &mut transport,
        )
        .await
        .expect("call_unary should complete even on a Trailers-Only error response");

        assert_eq!(result.response, None);
        assert_eq!(
            result.status.code, 5,
            "Trailers-Only status must be read via the headers fallback, not defaulted to 0"
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

/// Tests for the streaming RPC drive loop (17d-7). All offline, same bar as
/// `unary_call_tests`: self-signed loopback h2+TLS servers, no live network.
/// One test per mode (server-streaming, client-streaming, bidi) drains
/// `drive_streaming_call`'s event mpsc into a `Vec<GrpcEvent>` and asserts on
/// the exact sequence — this is the "Channel-free" shape flagged in this
/// module's doc comment: a real `tauri::ipc::Channel` couldn't be driven this
/// way, which is exactly why the drive loop takes plain `mpsc` channels.
#[cfg(test)]
mod streaming_call_tests {
    use super::*;
    use crate::model::grpc::GrpcEvent;
    use std::sync::Arc;

    use bytes::Bytes;
    use http::HeaderMap;
    use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
    use rustls::server::ServerConfig;
    use rustls::{ClientConfig, RootCertStore};
    use tokio::net::{TcpListener, TcpStream};
    use tokio::sync::mpsc;
    use tokio_rustls::{TlsAcceptor, TlsConnector};

    // Distinct service from unary_call_tests's Greeter so each test module
    // compiles its own self-contained schema — avoids any cross-module
    // visibility plumbing for what are otherwise near-identical small helpers
    // (mirrors transport.rs's tests and unary_call_tests each owning their
    // own cert/config helpers rather than sharing them).
    const STREAMING_PROTO: &str = r#"
        syntax = "proto3";
        package streamtest;
        service Counter {
          rpc CountUp (CountRequest) returns (stream CountReply);
          rpc Sum (stream CountRequest) returns (SumReply);
          rpc Echo (stream CountRequest) returns (stream CountReply);
        }
        message CountRequest {
          int32 value = 1;
        }
        message CountReply {
          int32 value = 1;
        }
        message SumReply {
          int32 total = 1;
        }
    "#;

    fn streaming_test_pool() -> DescriptorPool {
        let mut files = super::schema::ProtoFileSet::new();
        files.insert("counter.proto".to_string(), STREAMING_PROTO.to_string());
        super::schema::compile_proto_set(&files, &["counter.proto".to_string()])
            .expect("synthetic streaming proto should compile")
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

    /// Sets up a loopback TLS+h2 client connection (self-signed cert via
    /// `rcgen`, `GrpcTransport::drive` rather than `connect()` which would
    /// reject it) and hands back the connected transport plus the listener
    /// address the caller already used to spawn its own server task — shared
    /// tail for all three mode tests below, which otherwise only differ in
    /// what the server does with the stream.
    async fn connect_loopback_client(addr: std::net::SocketAddr, client_cfg: Arc<ClientConfig>) -> GrpcTransport {
        let sock = TcpStream::connect(addr).await.expect("client tcp connect");
        let connector = TlsConnector::from(client_cfg);
        let domain = ServerName::try_from("localhost").expect("server name");
        let tls = connector
            .connect(domain, sock)
            .await
            .expect("client tls handshake");
        GrpcTransport::drive(tls, "localhost:0".to_string(), true)
            .await
            .expect("client h2 handshake over the loopback TLS session")
    }

    /// Drains an event receiver into a `Vec` once the drive loop has
    /// finished (`drive_streaming_call` always ends by sending `Closed` or
    /// dropping the sender on an early return, either of which makes
    /// `recv()` eventually yield `None`).
    async fn collect_events(mut rx: mpsc::UnboundedReceiver<GrpcEvent>) -> Vec<GrpcEvent> {
        let mut events = Vec::new();
        while let Some(event) = rx.recv().await {
            events.push(event);
        }
        events
    }

    #[tokio::test(flavor = "current_thread")]
    async fn server_streaming_emits_one_response_per_server_message() {
        let pool = streaming_test_pool();
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

            assert_eq!(request.uri().path(), "/streamtest.Counter/CountUp");

            // Drain the single client request frame.
            let mut body = request.into_body();
            while let Some(chunk) = body.data().await {
                let chunk = chunk.expect("server body read");
                let _ = body.flow_control().release_capacity(chunk.len());
            }

            // Stream three reply messages, then a successful status.
            let response = http::Response::builder()
                .status(200)
                .body(())
                .expect("server response head");
            let mut send_stream = respond
                .send_response(response, false)
                .expect("server send_response");

            let pool = streaming_test_pool();
            let reply_desc = pool.get_message_by_name("streamtest.CountReply").unwrap();
            for n in 1..=3 {
                let mut msg = DynamicMessage::new(reply_desc.clone());
                msg.set_field_by_name("value", prost_reflect::Value::I32(n));
                let bytes = msg.encode_to_vec();
                send_stream
                    .send_data(Bytes::from(super::framing::frame(&bytes)), false)
                    .expect("server send_data");
            }

            let mut trailers = HeaderMap::new();
            trailers.insert("grpc-status", "0".parse().unwrap());
            send_stream
                .send_trailers(trailers)
                .expect("server send_trailers");
        });

        let mut transport = connect_loopback_client(addr, client_cfg).await;

        // Server-streaming: exactly one request, then the sender is dropped
        // so the outbound side of the drive loop half-closes immediately.
        let (req_tx, req_rx) = mpsc::unbounded_channel::<GrpcRequestMsg>();
        req_tx.send(Some(serde_json::json!({ "value": 0 }))).unwrap();
        drop(req_tx);

        let (event_tx, event_rx) = mpsc::unbounded_channel::<GrpcEvent>();
        drive_streaming_call(
            &pool,
            "streamtest.Counter/CountUp",
            &mut transport,
            req_rx,
            event_tx,
        )
        .await;

        let events = collect_events(event_rx).await;
        assert_eq!(events[0], GrpcEvent::Open);
        assert_eq!(
            events[1],
            GrpcEvent::Response {
                message: serde_json::json!({ "value": 1 })
            }
        );
        assert_eq!(
            events[2],
            GrpcEvent::Response {
                message: serde_json::json!({ "value": 2 })
            }
        );
        assert_eq!(
            events[3],
            GrpcEvent::Response {
                message: serde_json::json!({ "value": 3 })
            }
        );
        assert_eq!(
            events[4],
            GrpcEvent::Status {
                code: 0,
                message: None
            }
        );
        assert_eq!(events[5], GrpcEvent::Closed);
        assert_eq!(events.len(), 6);

        server.await.expect("server task did not finish cleanly");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn client_streaming_sends_n_requests_then_awaits_one_response() {
        let pool = streaming_test_pool();
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

            assert_eq!(request.uri().path(), "/streamtest.Counter/Sum");

            // Read every request frame the client sends (three values) and
            // sum them, proving the client-streaming send loop actually
            // delivered all of them before half-closing.
            let mut body = request.into_body();
            let mut buf = Vec::new();
            while let Some(chunk) = body.data().await {
                let chunk = chunk.expect("server body read");
                buf.extend_from_slice(&chunk);
                let _ = body.flow_control().release_capacity(chunk.len());
            }
            let mut unframer = super::framing::FrameUnframer::default();
            let frames = unframer.feed(&buf);
            assert_eq!(frames.len(), 3, "all three client messages should have arrived");

            let pool = streaming_test_pool();
            let req_desc = pool.get_message_by_name("streamtest.CountRequest").unwrap();
            let total: i32 = frames
                .iter()
                .map(|bytes| {
                    let msg = DynamicMessage::decode(req_desc.clone(), bytes.as_slice())
                        .expect("decode CountRequest");
                    msg.get_field_by_name("value")
                        .and_then(|v| v.as_i32())
                        .unwrap_or_default()
                })
                .sum();
            assert_eq!(total, 1 + 2 + 3);

            let sum_desc = pool.get_message_by_name("streamtest.SumReply").unwrap();
            let mut reply = DynamicMessage::new(sum_desc);
            reply.set_field_by_name("total", prost_reflect::Value::I32(total));
            let reply_bytes = reply.encode_to_vec();

            let response = http::Response::builder()
                .status(200)
                .body(())
                .expect("server response head");
            let mut send_stream = respond
                .send_response(response, false)
                .expect("server send_response");
            send_stream
                .send_data(Bytes::from(super::framing::frame(&reply_bytes)), false)
                .expect("server send_data");

            let mut trailers = HeaderMap::new();
            trailers.insert("grpc-status", "0".parse().unwrap());
            send_stream
                .send_trailers(trailers)
                .expect("server send_trailers");
        });

        let mut transport = connect_loopback_client(addr, client_cfg).await;

        let (req_tx, req_rx) = mpsc::unbounded_channel::<GrpcRequestMsg>();
        req_tx.send(Some(serde_json::json!({ "value": 1 }))).unwrap();
        req_tx.send(Some(serde_json::json!({ "value": 2 }))).unwrap();
        req_tx.send(Some(serde_json::json!({ "value": 3 }))).unwrap();
        req_tx.send(None).unwrap(); // explicit half-close
        drop(req_tx);

        let (event_tx, event_rx) = mpsc::unbounded_channel::<GrpcEvent>();
        drive_streaming_call(
            &pool,
            "streamtest.Counter/Sum",
            &mut transport,
            req_rx,
            event_tx,
        )
        .await;

        let events = collect_events(event_rx).await;
        assert_eq!(events[0], GrpcEvent::Open);
        assert_eq!(
            events[1],
            GrpcEvent::Response {
                message: serde_json::json!({ "total": 6 })
            }
        );
        assert_eq!(
            events[2],
            GrpcEvent::Status {
                code: 0,
                message: None
            }
        );
        assert_eq!(events[3], GrpcEvent::Closed);
        assert_eq!(events.len(), 4);

        server.await.expect("server task did not finish cleanly");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn bidi_streaming_echoes_each_request_as_it_arrives() {
        let pool = streaming_test_pool();
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

            assert_eq!(request.uri().path(), "/streamtest.Counter/Echo");

            let response = http::Response::builder()
                .status(200)
                .body(())
                .expect("server response head");
            let mut send_stream = respond
                .send_response(response, false)
                .expect("server send_response");

            // True full duplex: echo each request frame back as it arrives,
            // rather than draining the whole body first (which would just
            // exercise server-streaming again under a different name).
            let pool = streaming_test_pool();
            let req_desc = pool.get_message_by_name("streamtest.CountRequest").unwrap();
            let reply_desc = pool.get_message_by_name("streamtest.CountReply").unwrap();
            let mut body = request.into_body();
            let mut unframer = super::framing::FrameUnframer::default();
            while let Some(chunk) = body.data().await {
                let chunk = chunk.expect("server body read");
                let len = chunk.len();
                for payload in unframer.feed(&chunk) {
                    let msg = DynamicMessage::decode(req_desc.clone(), payload.as_slice())
                        .expect("decode CountRequest");
                    let value = msg
                        .get_field_by_name("value")
                        .and_then(|v| v.as_i32())
                        .unwrap_or_default();
                    let mut reply = DynamicMessage::new(reply_desc.clone());
                    reply.set_field_by_name("value", prost_reflect::Value::I32(value * 10));
                    let reply_bytes = reply.encode_to_vec();
                    send_stream
                        .send_data(Bytes::from(super::framing::frame(&reply_bytes)), false)
                        .expect("server send_data");
                }
                let _ = body.flow_control().release_capacity(len);
            }

            let mut trailers = HeaderMap::new();
            trailers.insert("grpc-status", "0".parse().unwrap());
            send_stream
                .send_trailers(trailers)
                .expect("server send_trailers");
        });

        let mut transport = connect_loopback_client(addr, client_cfg).await;

        let (req_tx, req_rx) = mpsc::unbounded_channel::<GrpcRequestMsg>();
        req_tx.send(Some(serde_json::json!({ "value": 1 }))).unwrap();
        req_tx.send(Some(serde_json::json!({ "value": 2 }))).unwrap();
        req_tx.send(None).unwrap();
        drop(req_tx);

        let (event_tx, event_rx) = mpsc::unbounded_channel::<GrpcEvent>();
        drive_streaming_call(
            &pool,
            "streamtest.Counter/Echo",
            &mut transport,
            req_rx,
            event_tx,
        )
        .await;

        let events = collect_events(event_rx).await;
        assert_eq!(events[0], GrpcEvent::Open);
        // Two echoed responses (order preserved: the server replies to each
        // request as it arrives), then status, then closed.
        assert_eq!(
            events[1],
            GrpcEvent::Response {
                message: serde_json::json!({ "value": 10 })
            }
        );
        assert_eq!(
            events[2],
            GrpcEvent::Response {
                message: serde_json::json!({ "value": 20 })
            }
        );
        assert_eq!(
            events[3],
            GrpcEvent::Status {
                code: 0,
                message: None
            }
        );
        assert_eq!(events[4], GrpcEvent::Closed);
        assert_eq!(events.len(), 5);

        server.await.expect("server task did not finish cleanly");
    }

    /// Proves the Trailers-Only headers fallback also engages for a
    /// streaming call: a server-streaming RPC that errors before sending any
    /// message looks exactly like the unary Trailers-Only case (status in
    /// HEADERS, no DATA, no separate trailers) — `resolve_call_status` is
    /// shared between `call_unary` and `drive_streaming_call`, so this closes
    /// the loop on that fix applying to both drive paths, not just unary.
    #[tokio::test(flavor = "current_thread")]
    async fn server_streaming_surfaces_trailers_only_status() {
        let pool = streaming_test_pool();
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

            let response = http::Response::builder()
                .status(200)
                .header("grpc-status", "5")
                .header("grpc-message", "no such counter")
                .body(())
                .expect("server response head");
            respond
                .send_response(response, true)
                .expect("server send_response (end_of_stream)");
        });

        let mut transport = connect_loopback_client(addr, client_cfg).await;

        let (req_tx, req_rx) = mpsc::unbounded_channel::<GrpcRequestMsg>();
        req_tx.send(Some(serde_json::json!({ "value": 0 }))).unwrap();
        drop(req_tx);

        let (event_tx, event_rx) = mpsc::unbounded_channel::<GrpcEvent>();
        drive_streaming_call(
            &pool,
            "streamtest.Counter/CountUp",
            &mut transport,
            req_rx,
            event_tx,
        )
        .await;

        let events = collect_events(event_rx).await;
        assert_eq!(events[0], GrpcEvent::Open);
        assert_eq!(
            events[1],
            GrpcEvent::Status {
                code: 5,
                message: Some("no such counter".to_string())
            },
            "Trailers-Only status must be read via the headers fallback, not defaulted to 0"
        );
        assert_eq!(events[2], GrpcEvent::Closed);
        assert_eq!(events.len(), 3);

        server.await.expect("server task did not finish cleanly");
    }
}