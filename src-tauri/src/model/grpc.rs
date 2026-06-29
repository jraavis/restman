//! Events/outbound types for a live gRPC call, pushed to/from the frontend
//! over a Tauri `Channel` — same shape as `model::streaming`'s `WsEvent`/
//! `WsOutbound` pair, but for gRPC's request/response semantics rather than a
//! WS frame stream.
//!
//! gRPC has no single "is this call done" bit at the HTTP/2 layer: a 200
//! response status only means the server accepted the request, never that
//! the RPC itself succeeded — that verdict rides in `grpc-status`/
//! `grpc-message` trailers (see `engine::grpc::transport`'s module docs for
//! why this client hand-rolls h2 instead of going through reqwest). So unlike
//! `WsEvent` (where `Closed` is the terminal/status-bearing event), gRPC gets
//! a dedicated `Status` event distinct from `Closed` — `Status` carries the
//! RPC-level verdict, `Closed` just marks "no more events on this channel."
//!
//! `DynamicMessage` (a decoded protobuf message with no compile-time Rust
//! type — see `engine::grpc::schema`/`reflection`) can't itself cross serde
//! IPC as a *request* shape the frontend could construct, so messages travel
//! as `serde_json::Value` in both directions: `GrpcMessageBuilder.tsx`
//! (already shipped, 17d-10) builds a JSON object matching the method's input
//! fields and is the eventual producer of `GrpcOutbound::request`; decoded
//! response messages are converted back to JSON via the same prost-reflect
//! `serde` support before reaching `GrpcEvent::Response`.

use serde::{Deserialize, Serialize};

/// Events pushed to the frontend over the course of one gRPC call (unary now;
/// the streaming modes in #31 reuse this same enum — `Response`/`Message` can
/// fire more than once for server-streaming, and `Sent` borrows the same
/// shape a client-stream send-ack would need).
#[allow(dead_code)] // caller lands in #29 (Tauri command wiring)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum GrpcEvent {
    /// The h2 stream opened and the request frame(s) were sent. Unlike
    /// `WsEvent::Open` (which fires after a handshake completes before any
    /// data flows), this fires after the request is already on the wire —
    /// gRPC has no separate handshake step the way WS upgrade does.
    Open,
    /// One decoded response message, converted from its wire-format
    /// `DynamicMessage` to JSON via the method's output descriptor. For a
    /// unary call this fires at most once; server-streaming (#31) fires it
    /// once per server message.
    Response { message: serde_json::Value },
    /// The HTTP/2 trailers carrying the RPC's actual verdict (`grpc-status` /
    /// `grpc-message`) arrived. This is the gRPC-level success/failure
    /// signal, independent of `http_status` (which only reflects whether the
    /// server accepted the request at all — see `engine::grpc::transport`).
    /// `code` is the numeric `grpc-status` value (0 = OK); `message` is the
    /// optional `grpc-message` detail text.
    Status { code: u32, message: Option<String> },
    /// The call failed before a gRPC status could be determined at all —
    /// connect/transport failure, a non-200 HTTP status, or a JSON/schema
    /// mismatch building the request. Distinct from `Status` with a non-zero
    /// code: this is "we never got far enough to ask the server," whereas a
    /// `Status` event (even an error one) means the server actually
    /// responded. Terminal.
    Error { message: String },
    /// No further events follow on this channel. Sent after `Status` on the
    /// success/failure path, or standalone if the peer closed the stream
    /// without ever sending trailers.
    Closed,
}

/// An outbound gRPC call request from the UI. `method_full_name` is the
/// slash-separated form `GrpcMessageBuilder.tsx`/`GrpcSchemaPicker.tsx`
/// already use (`"package.Service/Method"` — see
/// `src/features/streaming/grpcSchemaTypes.ts`'s `GrpcMethodDescriptor.
/// fullName`), not `prost_reflect`'s dot-separated
/// `MethodDescriptor::full_name()` form. This single field doubles as the
/// HTTP/2 request path once a leading `/` is added (`grpc_request_path`
/// below) — no separate "path" field is needed.
///
/// `request` is the request message's fields as a JSON object, matching the
/// shape `GrpcMessageBuilder`'s `onSend` callback produces (currently a JSON
/// *string* there, per its mock IPC in `grpcMessageIpc.ts` — #34/17d-8 decides
/// whether the Tauri command parses that string or the frontend is changed to
/// invoke with a parsed object; `serde_json::Value` here is the natural Rust
/// shape either way).
#[allow(dead_code)] // caller lands in #29 (Tauri command wiring)
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GrpcOutbound {
    pub method_full_name: String,
    pub request: serde_json::Value,
}

/// Splits a `"package.Service/Method"` full name into its service and method
/// parts. Returns `None` if there's no `/` separator at all — a malformed
/// name from a caller that didn't go through `GrpcSchemaPicker`/
/// `GrpcMessageBuilder`.
pub(crate) fn split_method_full_name(full: &str) -> Option<(&str, &str)> {
    full.split_once('/')
}

/// Builds the HTTP/2 request path for a gRPC call from the slash-separated
/// full name (e.g. `"pkg.Greeter/SayHello"` -> `"/pkg.Greeter/SayHello"`),
/// matching what `GrpcTransport::send` expects per its doc comment.
pub(crate) fn grpc_request_path(method_full_name: &str) -> String {
    format!("/{method_full_name}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_service_and_method_on_slash() {
        assert_eq!(
            split_method_full_name("example.Greeter/SayHello"),
            Some(("example.Greeter", "SayHello"))
        );
    }

    #[test]
    fn split_returns_none_without_a_slash() {
        assert_eq!(split_method_full_name("no-slash-here"), None);
    }

    #[test]
    fn split_uses_first_slash_only() {
        // A method name itself can't contain '/', but guard the boundary
        // choice explicitly: the service part stops at the first slash.
        assert_eq!(
            split_method_full_name("pkg.Service/Method/extra"),
            Some(("pkg.Service", "Method/extra"))
        );
    }

    #[test]
    fn builds_leading_slash_request_path() {
        assert_eq!(
            grpc_request_path("example.Greeter/SayHello"),
            "/example.Greeter/SayHello"
        );
    }

    #[test]
    fn grpc_event_serializes_with_camel_case_tag() {
        let event = GrpcEvent::Status {
            code: 0,
            message: None,
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "status");
        assert_eq!(json["code"], 0);
    }

    #[test]
    fn grpc_outbound_deserializes_camel_case_fields() {
        let json = serde_json::json!({
            "methodFullName": "example.Greeter/SayHello",
            "request": { "name": "world" }
        });
        let outbound: GrpcOutbound = serde_json::from_value(json).unwrap();
        assert_eq!(outbound.method_full_name, "example.Greeter/SayHello");
        assert_eq!(outbound.request["name"], "world");
    }
}
