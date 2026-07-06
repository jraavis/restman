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

/// Events pushed to the frontend over the course of one gRPC call — unary
/// (`call_unary`) and the streaming modes (`drive_streaming_call`, 17d-7)
/// both emit this same enum without any mode-specific variants: `Response`
/// fires once for unary/client-streaming and once per server message for
/// server-streaming/bidi, `Status`/`Closed` always end the call exactly the
/// same way regardless of mode. No "Sent"/ack variant was needed for
/// outbound streaming messages — see `engine::grpc::drive_streaming_call`'s
/// doc comment for why an ack-on-send event isn't part of this protocol.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum GrpcEvent {
    /// The h2 stream opened: request headers are on the wire. Unlike
    /// `WsEvent::Open` (which fires after a handshake completes before any
    /// data flows), gRPC has no separate handshake step the way WS upgrade
    /// does — this fires as soon as the stream itself opens, *before* any
    /// request message frame is sent. That ordering is deliberate, not just
    /// "as early as possible": for client-streaming/bidi, request frames
    /// keep flowing throughout the call (driven by the UI's own send
    /// actions), so "after the request frame(s) were sent" has no single
    /// point in time it could mean for those modes.
    Open,
    /// One decoded response message, converted from its wire-format
    /// `DynamicMessage` to JSON via the method's output descriptor. For a
    /// unary or client-streaming call this fires at most once;
    /// server-streaming/bidi (`drive_streaming_call`, 17d-7) fire it once per
    /// server message, in the order the server sent them.
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
    /// No further events follow on this channel. Always sent immediately
    /// after a `Status` event on the drive paths in this codebase
    /// (`call_unary`/`drive_streaming_call`'s `resolve_call_status` always
    /// resolves to a `GrpcStatus` — defaulting to code 0 if the peer closed
    /// without sending any status info at all — so a bare `Closed` with no
    /// preceding `Status` does not occur on the success/failure path; only
    /// `Error` skips `Status` entirely).
    Closed,
}

/// Arguments for the `grpc_connect` command: everything needed to build a
/// `DescriptorPool`, open the connection, and drive the first request.
///
/// `method_full_name` is the slash-separated form `GrpcMessageBuilder.tsx`/
/// `GrpcSchemaPicker.tsx` already use (`"package.Service/Method"` — see
/// `src/features/streaming/grpcSchemaTypes.ts`'s `GrpcMethodDescriptor.
/// fullName`), not `prost_reflect`'s dot-separated
/// `MethodDescriptor::full_name()` form. This doubles as the HTTP/2 request
/// path once a leading `/` is added (`grpc_request_path` below).
///
/// `request` is the request message's fields as a JSON object, matching the
/// shape `GrpcMessageBuilder`'s `onSend` callback produces (currently a JSON
/// *string* there, per its mock IPC in `grpcMessageIpc.ts`; the frontend's
/// real `grpcConnect` IPC wrapper parses that string into an object before
/// invoking, same as `serde_json::Value` expects here).
///
/// `proto_files`/`entry_point` are one of two ways this command sources a
/// `DescriptorPool` — compiling inline `.proto` source itself, the same way
/// `engine::grpc::schema::compile_proto_set`'s own tests do: a small
/// in-memory file set (`path -> source text`) plus which file is the entry
/// point. `proto_files` uses the same shape as `engine::grpc::schema::
/// ProtoFileSet` (a `BTreeMap`, but plain `HashMap`-shaped JSON crosses IPC
/// the same way either type serializes).
///
/// `descriptor_set` is the other way: a base64-encoded `FileDescriptorSet`,
/// as returned by `grpc_discover_schema`'s reflection mode — the
/// reflection-to-connect handoff. Exactly one of `descriptor_set` or
/// `proto_files`/`entry_point` must be set; `grpc_connect` rejects both-set
/// and neither-set as a clean argument error rather than guessing which one
/// the caller meant. Kept as an alternative *field*, not a replacement, so
/// `grpc_connect` stays stateless — no in-memory pool cache handed across
/// the IPC boundary by reference, which would be fragile (the pool could
/// outlive the discovery session, or vice versa).
///
/// The streaming mode (unary vs. client/server-streaming vs. bidi) is
/// deliberately *not* a field here: once the method descriptor is resolved
/// from the pool, `MethodDescriptor::is_client_streaming()`/
/// `is_server_streaming()` give it directly — passing a redundant mode flag
/// would just create a way for the frontend and the schema to disagree.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GrpcConnectArgs {
    pub url: String,
    pub method_full_name: String,
    pub request: serde_json::Value,
    #[serde(default)]
    pub proto_files: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub entry_point: String,
    #[serde(default)]
    pub descriptor_set: Option<String>,
}

/// An outbound request message sent on an already-open client-streaming/bidi
/// gRPC connection, via the `grpc_send` command. No `method_full_name`
/// field, unlike an early sketch of this type might suggest — a connection
/// is already bound to one method for its whole lifetime (set once at
/// `grpc_connect`), so repeating it on every subsequent send would be
/// redundant and could only ever disagree with the connection it's sent on.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GrpcOutbound {
    pub request: serde_json::Value,
}

// --- Schema discovery (grpc_discover_schema, reflection-to-connect handoff) ---
//
// Mirrors `src/features/streaming/grpcSchemaTypes.ts`'s `GrpcFieldDescriptor`/
// `GrpcMethodDescriptor` shape field-for-field (the frontend's
// `GrpcMessageBuilder`/`GrpcSchemaPicker` components already render against
// that shape), converted from `engine::grpc::reflection`'s pure-Rust
// `Discovered*` types at the command layer — the engine stays free of serde
// IPC concerns, same convention as the rest of this module.

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GrpcFieldDescriptorDto {
    pub name: String,
    pub r#type: String,
    pub repeated: bool,
    pub message_type_name: Option<String>,
}

impl From<crate::engine::grpc::reflection::DiscoveredField> for GrpcFieldDescriptorDto {
    fn from(f: crate::engine::grpc::reflection::DiscoveredField) -> Self {
        Self {
            name: f.name,
            r#type: f.type_name,
            repeated: f.repeated,
            message_type_name: f.message_type_name,
        }
    }
}

/// Matches `GrpcMethodDescriptor.streamingType`'s TS union exactly.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum GrpcStreamingTypeDto {
    Unary,
    ServerStreaming,
    ClientStreaming,
    Bidi,
}

impl From<crate::engine::grpc::reflection::GrpcStreamingKind> for GrpcStreamingTypeDto {
    fn from(kind: crate::engine::grpc::reflection::GrpcStreamingKind) -> Self {
        use crate::engine::grpc::reflection::GrpcStreamingKind;
        match kind {
            GrpcStreamingKind::Unary => GrpcStreamingTypeDto::Unary,
            GrpcStreamingKind::ServerStreaming => GrpcStreamingTypeDto::ServerStreaming,
            GrpcStreamingKind::ClientStreaming => GrpcStreamingTypeDto::ClientStreaming,
            GrpcStreamingKind::Bidi => GrpcStreamingTypeDto::Bidi,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GrpcMethodDescriptorDto {
    pub service_name: String,
    pub method_name: String,
    pub full_name: String,
    pub streaming_type: GrpcStreamingTypeDto,
    pub input_fields: Vec<GrpcFieldDescriptorDto>,
    pub output_fields: Vec<GrpcFieldDescriptorDto>,
}

impl From<crate::engine::grpc::reflection::DiscoveredMethod> for GrpcMethodDescriptorDto {
    fn from(m: crate::engine::grpc::reflection::DiscoveredMethod) -> Self {
        Self {
            service_name: m.service_name,
            method_name: m.method_name,
            full_name: m.full_name,
            streaming_type: m.streaming.into(),
            input_fields: m.input_fields.into_iter().map(Into::into).collect(),
            output_fields: m.output_fields.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GrpcServiceDescriptorDto {
    pub name: String,
    pub methods: Vec<GrpcMethodDescriptorDto>,
}

impl From<crate::engine::grpc::reflection::DiscoveredService> for GrpcServiceDescriptorDto {
    fn from(s: crate::engine::grpc::reflection::DiscoveredService) -> Self {
        Self {
            name: s.name,
            methods: s.methods.into_iter().map(Into::into).collect(),
        }
    }
}

/// `grpc_discover_schema`'s return value. `descriptor_set` is the base64-
/// encoded `FileDescriptorSet` bytes `engine::grpc::reflection::
/// discover_schema` assembled — round-trips back into `GrpcConnectArgs.
/// descriptor_set` unchanged, so the frontend never needs to inspect or
/// re-encode it, just carry it from discovery to connect.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GrpcSchemaDiscoveryResult {
    pub services: Vec<GrpcServiceDescriptorDto>,
    pub descriptor_set: String,
}

impl From<crate::engine::grpc::reflection::DiscoveredSchema> for GrpcSchemaDiscoveryResult {
    fn from(schema: crate::engine::grpc::reflection::DiscoveredSchema) -> Self {
        use base64::Engine as _;
        Self {
            services: schema.services.into_iter().map(Into::into).collect(),
            descriptor_set: base64::engine::general_purpose::STANDARD.encode(schema.file_descriptor_set),
        }
    }
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
            "request": { "name": "world" }
        });
        let outbound: GrpcOutbound = serde_json::from_value(json).unwrap();
        assert_eq!(outbound.request["name"], "world");
    }

    #[test]
    fn grpc_connect_args_deserializes_camel_case_fields() {
        let json = serde_json::json!({
            "url": "grpc://example.com:50051",
            "methodFullName": "example.Greeter/SayHello",
            "request": { "name": "world" },
            "protoFiles": { "greeter.proto": "syntax = \"proto3\";" },
            "entryPoint": "greeter.proto"
        });
        let args: GrpcConnectArgs = serde_json::from_value(json).unwrap();
        assert_eq!(args.url, "grpc://example.com:50051");
        assert_eq!(args.method_full_name, "example.Greeter/SayHello");
        assert_eq!(args.request["name"], "world");
        assert_eq!(
            args.proto_files.get("greeter.proto").map(String::as_str),
            Some("syntax = \"proto3\";")
        );
        assert_eq!(args.entry_point, "greeter.proto");
    }
}
