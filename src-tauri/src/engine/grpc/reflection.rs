//! gRPC server reflection client — v1 (`grpc.reflection.v1`) with `v1alpha`
//! (`grpc.reflection.v1alpha`) fallback. Lands as task #26.
//!
//! ## Scope: message construction/parsing only, not the network call
//!
//! This module builds `ServerReflectionRequest` `DynamicMessage`s and parses
//! `ServerReflectionResponse` `DynamicMessage`s back into a typed
//! [`ReflectionResponse`], plus a pure `should_retry_on_v1alpha` decision
//! function. Actually driving the bidi-streaming `ServerReflectionInfo` RPC
//! over a live `GrpcTransport` connection is later work (#28/#29) — this
//! module has no socket code and opens no connection.
//!
//! ## Why the schemas are inlined `const`s, not loaded from
//! `tests/fixtures/grpc/`
//!
//! `testsupport.rs`'s `FIXTURES_ROOT` resolves via `CARGO_MANIFEST_DIR`,
//! which only exists at *this crate's own build time*, and the
//! `tests/fixtures/grpc/` tree itself is established elsewhere in this
//! module set as a test-only convention (`testsupport.rs` is
//! `#[cfg(test)]`-gated; `schema.rs` only reads fixtures from inside its own
//! `#[cfg(test)]` block). The request builders below are production code
//! (called by #28/#29 against a real server), so neither schema is read
//! from that tree at all — not even via `include_str!`, which would still
//! create a compile-time dependency from production code onto a test-only
//! path. Instead the `v1` schema text is inlined verbatim as the
//! [`V1_PROTO`] `const` (a byte-for-byte copy of
//! `tests/fixtures/grpc/reflection.proto`), and the `v1alpha` schema — not
//! yet vendored as a fixture anywhere in this repo — is inlined the same way
//! as [`V1ALPHA_PROTO`]. Both are compiled in-memory via
//! `schema::compile_proto_set` (the same `protox`-backed,
//! filesystem/network-free primitive #27 built for runtime `.proto`
//! uploads), producing a `prost_reflect::DescriptorPool` — the same output
//! type `engine::grpc::schema` produces, so #29 can treat
//! reflection-discovered and upload-discovered schemas uniformly, per the
//! 17d-4 task brief.
//!
//! A `#[cfg(test)]` test (`embedded_v1_proto_matches_vendored_fixture`)
//! independently compiles the real `tests/fixtures/grpc/reflection.proto`
//! fixture and the inlined [`V1_PROTO`] `const`, then compares the resulting
//! descriptor pools — a real drift guard, since the two are sourced
//! differently (filesystem read vs. inlined literal) and could fall out of
//! sync if one is edited without the other. Production code paths never
//! touch the filesystem.
//!
//! ## `v1alpha` provenance
//!
//! `grpc.reflection.v1alpha` is the older, still-widely-deployed reflection
//! service; `grpc.reflection.v1` (vendored at
//! `tests/fixtures/grpc/reflection.proto`, mirrored as [`V1_PROTO`] below) is
//! the newer, stabilized version. Per the canonical grpc-proto sources
//! (`https://github.com/grpc/grpc-proto/blob/master/grpc/reflection/v1alpha/reflection.proto`
//! and the `v1` sibling already vendored in 17c), the two are
//! message-shape-identical — same field names and numbers throughout
//! (`ServerReflectionRequest.host = 1`,
//! `.file_by_filename = 3`/`.file_containing_symbol = 4`/
//! `.file_containing_extension = 5`/`.all_extension_numbers_of_type = 6`/
//! `.list_services = 7`; `ServerReflectionResponse.valid_host = 1`/
//! `.original_request = 2`/`.file_descriptor_response = 4`/
//! `.all_extension_numbers_response = 5`/`.list_services_response = 6`/
//! `.error_response = 7`; same nested message shapes) — differing only in
//! the `package` declaration (`grpc.reflection.v1alpha` vs
//! `grpc.reflection.v1`) and cosmetic `option` lines. [`V1ALPHA_PROTO`] below
//! is the v1 text with exactly that substitution, so a server implementing
//! only the older service can still be queried with the same request shapes.
//! A `#[cfg(test)]` check locks the embedded field numbers against the
//! values documented here.
//!
//! ## Error handling
//!
//! Follows this repo's established engine-layer convention (see
//! `engine::grpc::schema`'s module docs) — `crate::error::AppError`/
//! `AppResult`, no bespoke enum, no `anyhow`.

use std::collections::BTreeMap;

use prost::Message as _;
use prost_reflect::{DescriptorPool, DynamicMessage, MessageDescriptor, Value};
use prost_types::FileDescriptorProto;

use crate::error::{AppError, AppResult};

use super::schema::{compile_proto_set, ProtoFileSet};

/// Which reflection service version a request/response pair targets.
/// `full_name()`s differ only in this package segment (see module docs), so
/// every descriptor lookup in this module is parameterized on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // callers land in #28/#29 (RPC drive loop, Tauri commands)
pub(crate) enum ReflectionVersion {
    V1,
    V1Alpha,
}

impl ReflectionVersion {
    fn package(self) -> &'static str {
        match self {
            ReflectionVersion::V1 => "grpc.reflection.v1",
            ReflectionVersion::V1Alpha => "grpc.reflection.v1alpha",
        }
    }

    /// Fully-qualified gRPC service path for the bidi-streaming
    /// `ServerReflectionInfo` RPC, e.g.
    /// `/grpc.reflection.v1.ServerReflection/ServerReflectionInfo` — the form
    /// `h2`'s `:path` pseudo-header needs. Exposed now so #28's drive loop
    /// has one canonical source for it per version rather than re-deriving
    /// the string at the call site.
    #[allow(dead_code)] // caller lands in #28 (RPC drive loop)
    pub(crate) fn service_path(self) -> String {
        format!("/{}.ServerReflection/ServerReflectionInfo", self.package())
    }

    fn request_message_name(self) -> String {
        format!("{}.ServerReflectionRequest", self.package())
    }

    fn response_message_name(self) -> String {
        format!("{}.ServerReflectionResponse", self.package())
    }
}

/// `grpc.reflection.v1` schema text, inlined as a true `const` (not
/// `include_str!`) so production code has zero compile-time *or* runtime
/// coupling to `tests/fixtures/grpc/reflection.proto` (a test-only path —
/// see module docs). Kept byte-identical to that fixture; a `#[cfg(test)]`
/// check below (`embedded_v1_proto_matches_vendored_fixture`) compiles both
/// independently and compares the resulting descriptor pools, so the two
/// can't silently drift apart.
const V1_PROTO: &str = r#"// Copyright 2016 The gRPC Authors
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

// Service exported by server reflection.  A more complete description of how
// server reflection works can be found at
// https://github.com/grpc/grpc/blob/master/doc/server-reflection.md
//
// The canonical version of this proto can be found at
// https://github.com/grpc/grpc-proto/blob/master/grpc/reflection/v1/reflection.proto

syntax = "proto3";

package grpc.reflection.v1;

option go_package = "google.golang.org/grpc/reflection/grpc_reflection_v1";
option java_multiple_files = true;
option java_package = "io.grpc.reflection.v1";
option java_outer_classname = "ServerReflectionProto";

service ServerReflection {
  // The reflection service is structured as a bidirectional stream, ensuring
  // all related requests go to a single server.
  rpc ServerReflectionInfo(stream ServerReflectionRequest)
      returns (stream ServerReflectionResponse);
}

// The message sent by the client when calling ServerReflectionInfo method.
message ServerReflectionRequest {
  string host = 1;
  // To use reflection service, the client should set one of the following
  // fields in message_request. The server distinguishes requests by their
  // defined field and then handles them using corresponding methods.
  oneof message_request {
    // Find a proto file by the file name.
    string file_by_filename = 3;

    // Find the proto file that declares the given fully-qualified symbol name.
    // This field should be a fully-qualified symbol name
    // (e.g. <package>.<service>[.<method>] or <package>.<type>).
    string file_containing_symbol = 4;

    // Find the proto file which defines an extension extending the given
    // message type with the given field number.
    ExtensionRequest file_containing_extension = 5;

    // Finds the tag numbers used by all known extensions of the given message
    // type, and appends them to ExtensionNumberResponse in an undefined order.
    // Its corresponding method is best-effort: it's not guaranteed that the
    // reflection service will implement this method, and it's not guaranteed
    // that this method will provide all extensions. Returns
    // StatusCode::UNIMPLEMENTED if it's not implemented.
    // This field should be a fully-qualified type name. The format is
    // <package>.<type>
    string all_extension_numbers_of_type = 6;

    // List the full names of registered services. The content will not be
    // checked.
    string list_services = 7;
  }
}

// The type name and extension number sent by the client when requesting
// file_containing_extension.
message ExtensionRequest {
  // Fully-qualified type name. The format should be <package>.<type>
  string containing_type = 1;
  int32 extension_number = 2;
}

// The message sent by the server to answer ServerReflectionInfo method.
message ServerReflectionResponse {
  string valid_host = 1;
  ServerReflectionRequest original_request = 2;
  // The server sets one of the following fields according to the message_request
  // in the request.
  oneof message_response {
    // This message is used to answer file_by_filename, file_containing_symbol,
    // file_containing_extension requests with transitive dependencies.
    // As the repeated label is not allowed in oneof fields, we use a
    // FileDescriptorResponse message to encapsulate the repeated fields.
    // The reflection service is allowed to avoid sending FileDescriptorProtos
    // that were previously sent in response to earlier requests in the stream.
    FileDescriptorResponse file_descriptor_response = 4;

    // This message is used to answer all_extension_numbers_of_type requests.
    ExtensionNumberResponse all_extension_numbers_response = 5;

    // This message is used to answer list_services requests.
    ListServiceResponse list_services_response = 6;

    // This message is used when an error occurs.
    ErrorResponse error_response = 7;
  }
}

// Serialized FileDescriptorProto messages sent by the server answering
// a file_by_filename, file_containing_symbol, or file_containing_extension
// request.
message FileDescriptorResponse {
  // Serialized FileDescriptorProto messages. We avoid taking a dependency on
  // descriptor.proto, which uses proto2 only features, by making them opaque
  // bytes instead.
  repeated bytes file_descriptor_proto = 1;
}

// A list of extension numbers sent by the server answering
// all_extension_numbers_of_type request.
message ExtensionNumberResponse {
  // Full name of the base type, including the package name. The format
  // is <package>.<type>
  string base_type_name = 1;
  repeated int32 extension_number = 2;
}

// A list of ServiceResponse sent by the server answering list_services request.
message ListServiceResponse {
  // The information of each service may be expanded in the future, so we use
  // ServiceResponse message to encapsulate it.
  repeated ServiceResponse service = 1;
}

// The information of a single service used by ListServiceResponse to answer
// list_services request.
message ServiceResponse {
  // Full name of a registered service, including its package name. The format
  // is <package>.<service>
  string name = 1;
}

// The error code and error message sent by the server when an error occurs.
message ErrorResponse {
  // This field uses the error codes defined in grpc::StatusCode.
  int32 error_code = 1;
  string error_message = 2;
}
"#;

/// `grpc.reflection.v1alpha` schema text — the v1 text above with only the
/// `package`/`go_package`/`java_package` lines changed to the `v1alpha`
/// namespace (see module docs for the field-by-field shape-identity
/// justification). Not vendored as a separate fixture file: this module is
/// the only caller of the v1alpha schema today, and embedding keeps the
/// "single offline, filesystem-free schema source" property the v1 const
/// above already has.
const V1ALPHA_PROTO: &str = r#"// Copyright 2016 The gRPC Authors
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

// Service exported by server reflection. A more complete description of how
// server reflection works can be found at
// https://github.com/grpc/grpc/blob/master/doc/server-reflection.md
//
// The canonical version of this proto can be found at
// https://github.com/grpc/grpc-proto/blob/master/grpc/reflection/v1alpha/reflection.proto

syntax = "proto3";

package grpc.reflection.v1alpha;

option go_package = "google.golang.org/grpc/reflection/grpc_reflection_v1alpha";
option java_multiple_files = true;
option java_package = "io.grpc.reflection.v1alpha";
option java_outer_classname = "ServerReflectionProto";

service ServerReflection {
  // The reflection service is structured as a bidirectional stream, ensuring
  // all related requests go to a single server.
  rpc ServerReflectionInfo(stream ServerReflectionRequest)
      returns (stream ServerReflectionResponse);
}

// The message sent by the client when calling ServerReflectionInfo method.
message ServerReflectionRequest {
  string host = 1;
  // To use reflection service, the client should set one of the following
  // fields in message_request. The server distinguishes requests by their
  // defined field and then handles them using corresponding methods.
  oneof message_request {
    // Find a proto file by the file name.
    string file_by_filename = 3;

    // Find the proto file that declares the given fully-qualified symbol name.
    // This field should be a fully-qualified symbol name
    // (e.g. <package>.<service>[.<method>] or <package>.<type>).
    string file_containing_symbol = 4;

    // Find the proto file which defines an extension extending the given
    // message type with the given field number.
    ExtensionRequest file_containing_extension = 5;

    // Finds the tag numbers used by all known extensions of the given message
    // type, and appends them to ExtensionNumberResponse in an undefined order.
    // Its corresponding method is best-effort: it's not guaranteed that the
    // reflection service will implement this method, and it's not guaranteed
    // that this method will provide all extensions. Returns
    // StatusCode::UNIMPLEMENTED if it's not implemented.
    // This field should be a fully-qualified type name. The format is
    // <package>.<type>
    string all_extension_numbers_of_type = 6;

    // List the full names of registered services. The content will not be
    // checked.
    string list_services = 7;
  }
}

// The type name and extension number sent by the client when requesting
// file_containing_extension.
message ExtensionRequest {
  // Fully-qualified type name. The format should be <package>.<type>
  string containing_type = 1;
  int32 extension_number = 2;
}

// The message sent by the server to answer ServerReflectionInfo method.
message ServerReflectionResponse {
  string valid_host = 1;
  ServerReflectionRequest original_request = 2;
  // The server sets one of the following fields according to the message_request
  // in the request.
  oneof message_response {
    // This message is used to answer file_by_filename, file_containing_symbol,
    // file_containing_extension requests with transitive dependencies.
    // As the repeated label is not allowed in oneof fields, we use a
    // FileDescriptorResponse message to encapsulate the repeated fields.
    // The reflection service is allowed to avoid sending FileDescriptorProtos
    // that were previously sent in response to earlier requests in the stream.
    FileDescriptorResponse file_descriptor_response = 4;

    // This message is used to answer all_extension_numbers_of_type requests.
    ExtensionNumberResponse all_extension_numbers_response = 5;

    // This message is used to answer list_services requests.
    ListServiceResponse list_services_response = 6;

    // This message is used when an error occurs.
    ErrorResponse error_response = 7;
  }
}

// Serialized FileDescriptorProto messages sent by the server answering
// a file_by_filename, file_containing_symbol, or file_containing_extension
// request.
message FileDescriptorResponse {
  // Serialized FileDescriptorProto messages. We avoid taking a dependency on
  // descriptor.proto, which uses proto2 only features, by making them opaque
  // bytes instead.
  repeated bytes file_descriptor_proto = 1;
}

// A list of extension numbers sent by the server answering
// all_extension_numbers_of_type request.
message ExtensionNumberResponse {
  // Full name of the base type, including the package name. The format
  // is <package>.<type>
  string base_type_name = 1;
  repeated int32 extension_number = 2;
}

// A list of ServiceResponse sent by the server answering list_services request.
message ListServiceResponse {
  // The information of each service may be expanded in the future, so we use
  // ServiceResponse message to encapsulate it.
  repeated ServiceResponse service = 1;
}

// The information of a single service used by ListServiceResponse to answer
// list_services request.
message ServiceResponse {
  // Full name of a registered service, including its package name. The format
  // is <package>.<service>
  string name = 1;
}

// The error code and error message sent by the server when an error occurs.
message ErrorResponse {
  // This field uses the error codes defined in grpc::StatusCode.
  int32 error_code = 1;
  string error_message = 2;
}
"#;

fn proto_text(version: ReflectionVersion) -> &'static str {
    match version {
        ReflectionVersion::V1 => V1_PROTO,
        ReflectionVersion::V1Alpha => V1ALPHA_PROTO,
    }
}

/// Compiles the embedded reflection schema for `version` into a
/// `DescriptorPool`, entirely in-memory (no filesystem, no network) via
/// `schema::compile_proto_set` — the same primitive #27 built for runtime
/// `.proto` uploads.
#[allow(dead_code)] // also called by request/response helpers below
pub(crate) fn reflection_descriptor_pool(version: ReflectionVersion) -> AppResult<DescriptorPool> {
    let mut files: ProtoFileSet = BTreeMap::new();
    files.insert("reflection.proto".to_string(), proto_text(version).to_string());
    compile_proto_set(&files, &["reflection.proto".to_string()])
}

fn request_descriptor(version: ReflectionVersion) -> AppResult<MessageDescriptor> {
    let pool = reflection_descriptor_pool(version)?;
    pool.get_message_by_name(&version.request_message_name())
        .ok_or_else(|| {
            AppError::Other(format!(
                "{} missing from its own compiled descriptor pool (embedded schema bug)",
                version.request_message_name()
            ))
        })
}

fn response_descriptor(version: ReflectionVersion) -> AppResult<MessageDescriptor> {
    let pool = reflection_descriptor_pool(version)?;
    pool.get_message_by_name(&version.response_message_name())
        .ok_or_else(|| {
            AppError::Other(format!(
                "{} missing from its own compiled descriptor pool (embedded schema bug)",
                version.response_message_name()
            ))
        })
}

/// Builds a `list_services` `ServerReflectionRequest` `DynamicMessage` for
/// `version`. `host` is optional per the proto's own comments (the server is
/// not required to validate it); an empty string is the conventional "don't
/// care" value real clients (e.g. `grpcurl`) send.
#[allow(dead_code)] // caller lands in #28 (RPC drive loop)
pub(crate) fn build_list_services_request(
    version: ReflectionVersion,
    host: &str,
) -> AppResult<DynamicMessage> {
    let desc = request_descriptor(version)?;
    let mut msg = DynamicMessage::new(desc);
    msg.set_field_by_name("host", Value::String(host.to_string()));
    msg.set_field_by_name("list_services", Value::String(String::new()));
    Ok(msg)
}

/// Builds a `file_containing_symbol` `ServerReflectionRequest`
/// `DynamicMessage` for `version`. `symbol` should be a fully-qualified
/// `<package>.<service>[.<method>]` or `<package>.<type>` name, per the
/// proto's own field documentation.
#[allow(dead_code)] // caller lands in #28 (RPC drive loop)
pub(crate) fn build_file_containing_symbol_request(
    version: ReflectionVersion,
    host: &str,
    symbol: &str,
) -> AppResult<DynamicMessage> {
    let desc = request_descriptor(version)?;
    let mut msg = DynamicMessage::new(desc);
    msg.set_field_by_name("host", Value::String(host.to_string()));
    msg.set_field_by_name(
        "file_containing_symbol",
        Value::String(symbol.to_string()),
    );
    Ok(msg)
}

/// A single registered service name, as reported by a `list_services`
/// response (`ServiceResponse.name`, repeated inside
/// `ListServiceResponse.service`).
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)] // caller lands in #28/#29
pub(crate) struct ServiceInfo {
    pub(crate) name: String,
}

/// The error shape carried by `ServerReflectionResponse.error_response` —
/// `error_code` uses `grpc::StatusCode` numbering (e.g. `12` ==
/// `UNIMPLEMENTED`, the value [`should_retry_on_v1alpha`] keys off).
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)] // caller lands in #28/#29
pub(crate) struct ReflectionError {
    pub(crate) code: i32,
    pub(crate) message: String,
}

/// A parsed `ServerReflectionResponse`, mirroring its `message_response`
/// oneof so callers don't poke at `DynamicMessage` directly. `Unset` covers
/// the (spec-legal but pathological) case where the server didn't populate
/// any oneof member — callers should treat it like an error, not panic.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)] // caller lands in #28/#29
pub(crate) enum ReflectionResponse {
    /// `file_descriptor_response`: serialized `FileDescriptorProto` bytes,
    /// one per transitive dependency the server chose to include. Still raw
    /// bytes here (parsing/assembling into a `DescriptorPool` is
    /// [`descriptor_pool_from_file_descriptors`], kept separate so a caller
    /// that just wants the byte count, or wants to merge several responses
    /// before building a pool, isn't forced through a pool build first).
    FileDescriptors(Vec<Vec<u8>>),
    /// `list_services_response`: the registered service names.
    ListServices(Vec<ServiceInfo>),
    /// `all_extension_numbers_response`: base type name plus its known
    /// extension field numbers.
    ExtensionNumbers {
        base_type_name: String,
        extension_number: Vec<i32>,
    },
    /// `error_response`.
    Error(ReflectionError),
    /// No `message_response` oneof member was set.
    Unset,
}

/// Parses a `ServerReflectionResponse` `DynamicMessage` (as produced by
/// decoding bytes received from a live reflection RPC, or — in tests — built
/// directly) into a [`ReflectionResponse`]. `prost_reflect` has no
/// `which_oneof`-style accessor on `DynamicMessage` (confirmed against the
/// vendored crate source), so this checks each `message_response` member by
/// name via `has_field_by_name`, in the field-number order the proto
/// declares them.
#[allow(dead_code)] // caller lands in #28/#29
pub(crate) fn parse_response(msg: &DynamicMessage) -> ReflectionResponse {
    if msg.has_field_by_name("file_descriptor_response") {
        if let Some(value) = msg.get_field_by_name("file_descriptor_response") {
            if let Some(inner) = value.as_message() {
                let protos = inner
                    .get_field_by_name("file_descriptor_proto")
                    .and_then(|v| v.as_list().map(|list| list.to_vec()))
                    .unwrap_or_default();
                let bytes: Vec<Vec<u8>> = protos
                    .iter()
                    .filter_map(|v| v.as_bytes().map(|b| b.to_vec()))
                    .collect();
                return ReflectionResponse::FileDescriptors(bytes);
            }
        }
        return ReflectionResponse::FileDescriptors(Vec::new());
    }

    if msg.has_field_by_name("list_services_response") {
        if let Some(value) = msg.get_field_by_name("list_services_response") {
            if let Some(inner) = value.as_message() {
                let services = inner
                    .get_field_by_name("service")
                    .and_then(|v| v.as_list().map(|list| list.to_vec()))
                    .unwrap_or_default();
                let names: Vec<ServiceInfo> = services
                    .iter()
                    .filter_map(|v| v.as_message())
                    .filter_map(|service_msg| {
                        service_msg
                            .get_field_by_name("name")
                            .and_then(|n| n.as_str().map(|s| s.to_string()))
                    })
                    .map(|name| ServiceInfo { name })
                    .collect();
                return ReflectionResponse::ListServices(names);
            }
        }
        return ReflectionResponse::ListServices(Vec::new());
    }

    if msg.has_field_by_name("all_extension_numbers_response") {
        if let Some(value) = msg.get_field_by_name("all_extension_numbers_response") {
            if let Some(inner) = value.as_message() {
                let base_type_name = inner
                    .get_field_by_name("base_type_name")
                    .and_then(|v| v.as_str().map(|s| s.to_string()))
                    .unwrap_or_default();
                let extension_number = inner
                    .get_field_by_name("extension_number")
                    .and_then(|v| v.as_list().map(|list| list.to_vec()))
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|v| v.as_i32())
                    .collect();
                return ReflectionResponse::ExtensionNumbers {
                    base_type_name,
                    extension_number,
                };
            }
        }
        return ReflectionResponse::ExtensionNumbers {
            base_type_name: String::new(),
            extension_number: Vec::new(),
        };
    }

    if msg.has_field_by_name("error_response") {
        if let Some(value) = msg.get_field_by_name("error_response") {
            if let Some(inner) = value.as_message() {
                let code = inner
                    .get_field_by_name("error_code")
                    .and_then(|v| v.as_i32())
                    .unwrap_or_default();
                let message = inner
                    .get_field_by_name("error_message")
                    .and_then(|v| v.as_str().map(|s| s.to_string()))
                    .unwrap_or_default();
                return ReflectionResponse::Error(ReflectionError { code, message });
            }
        }
        return ReflectionResponse::Error(ReflectionError {
            code: 0,
            message: String::new(),
        });
    }

    ReflectionResponse::Unset
}

/// Decodes raw `ServerReflectionResponse` bytes (as received off the wire)
/// for `version`, then parses them via [`parse_response`]. Split from
/// `parse_response` so callers that already have a `DynamicMessage` (e.g.
/// tests constructing one directly) don't need to round-trip through bytes
/// first.
#[allow(dead_code)] // caller lands in #28 (RPC drive loop)
pub(crate) fn decode_response(version: ReflectionVersion, bytes: &[u8]) -> AppResult<ReflectionResponse> {
    let desc = response_descriptor(version)?;
    let msg = DynamicMessage::decode(desc, bytes)
        .map_err(|e| AppError::Other(format!("failed to decode ServerReflectionResponse: {e}")))?;
    Ok(parse_response(&msg))
}

/// `grpc::StatusCode::UNIMPLEMENTED` — the status a server returns when an
/// RPC method (here, `v1`'s `ServerReflectionInfo`) doesn't exist at all.
/// Matches the canonical gRPC status code table
/// (`https://github.com/grpc/grpc/blob/master/doc/statuscodes.md`). This is
/// the unambiguous "this service version isn't registered" signal — the
/// trailers-level status a server's RPC dispatch returns when it has no
/// handler for `grpc.reflection.v1.ServerReflection/ServerReflectionInfo` at
/// all, which is exactly the case an older server implementing only
/// `v1alpha` produces.
const GRPC_STATUS_UNIMPLEMENTED: i32 = 12;

/// An error/unsupported signal observed while attempting reflection against
/// one version, fed into [`should_retry_on_v1alpha`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // caller lands in #28/#29 (the retry-with-a-live-connection loop)
pub(crate) enum ReflectionAttemptOutcome {
    /// The RPC itself could not be reached/completed at the transport level
    /// (e.g. the server has no `grpc.reflection.v1.ServerReflection`
    /// service registered at all, so even opening the stream fails).
    ServiceUnavailable,
    /// A `ServerReflectionResponse.error_response` came back with the given
    /// `grpc::StatusCode`.
    ErrorResponse { code: i32 },
    /// A response was parsed successfully and was not an error — reflection
    /// is working on this version, no fallback needed.
    Success,
}

/// Pure decision function: given what happened on a `v1` reflection attempt,
/// should the caller retry on `v1alpha`? This does not perform any retry or
/// network call itself — driving a second live connection on a fallback
/// version is #28/#29's job (see module docs); this only captures the
/// decision logic so it's independently testable.
///
/// Retries on:
/// - `ServiceUnavailable` (the `v1` service isn't reachable at all — the
///   textbook case an older server only implementing `v1alpha` produces).
/// - `ErrorResponse` carrying `UNIMPLEMENTED` (12) — the status code a
///   server's RPC dispatch returns when it has no handler for the `v1`
///   service/method at all, per the gRPC status code table.
///
/// Does not retry on `Success`, nor on an `ErrorResponse` carrying any other
/// status code. In particular, `NOT_FOUND` (5) is deliberately excluded even
/// though it sounds plausible: in a `ServerReflectionInfo` `error_response`,
/// `NOT_FOUND` means the *queried symbol/file* doesn't exist — a legitimate
/// answer from a fully-working `v1` service, not a sign that `v1` itself is
/// unsupported. Treating it as a fallback trigger would cause a spurious
/// `v1alpha` retry that returns the exact same `NOT_FOUND` for the exact
/// same reason. More generally, any other status code (e.g.
/// `INVALID_ARGUMENT`, `PERMISSION_DENIED`) means the `v1` service exists
/// and answered, just unfavorably; retrying on `v1alpha` would not change
/// that outcome and would mask the real error.
#[allow(dead_code)] // caller lands in #28/#29
pub(crate) fn should_retry_on_v1alpha(outcome: ReflectionAttemptOutcome) -> bool {
    match outcome {
        ReflectionAttemptOutcome::ServiceUnavailable => true,
        ReflectionAttemptOutcome::ErrorResponse { code } => code == GRPC_STATUS_UNIMPLEMENTED,
        ReflectionAttemptOutcome::Success => false,
    }
}

/// Decodes a list of raw `FileDescriptorProto` bytes (as carried by
/// `ReflectionResponse::FileDescriptors`) into a single `DescriptorPool`.
/// This is the uniformity hook called out in the 17d-4 task brief: a
/// reflection-discovered schema and a `schema::compile_proto_set`-discovered
/// (upload) schema both end up as a `prost_reflect::DescriptorPool`, so #29
/// can treat them the same way regardless of discovery method.
///
/// Per `prost_reflect::DescriptorPool::from_file_descriptor_set`, the
/// `FileDescriptorProto`s must be supplied in dependency order (a file
/// before anything that imports it) — real servers are expected to send
/// transitive dependencies that way (the proto's own comments describe the
/// response as carrying "transitive dependencies"), but enforcing or
/// reordering that is out of scope here; this function decodes and assembles
/// exactly what it's given.
#[allow(dead_code)] // caller lands in #29 (treats reflection/upload schemas uniformly)
pub(crate) fn descriptor_pool_from_file_descriptors(
    file_descriptor_protos: &[Vec<u8>],
) -> AppResult<DescriptorPool> {
    let files: AppResult<Vec<FileDescriptorProto>> = file_descriptor_protos
        .iter()
        .map(|bytes| {
            FileDescriptorProto::decode(bytes.as_slice()).map_err(|e| {
                AppError::Other(format!("failed to decode FileDescriptorProto: {e}"))
            })
        })
        .collect();
    let fds = prost_types::FileDescriptorSet { file: files? };
    DescriptorPool::from_file_descriptor_set(fds).map_err(|e| {
        AppError::Other(format!(
            "reflection-discovered file descriptors produced an invalid descriptor pool: {e}"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::grpc::testsupport::{compile_reflection_proto, FIXTURES_ROOT};
    use prost_reflect::DescriptorPool as TestDescriptorPool;

    /// Locks the embedded [`V1_PROTO`] against drift from the real vendored
    /// fixture: both should compile to byte-identical descriptor pools (same
    /// message full names reachable either way).
    #[test]
    fn embedded_v1_proto_matches_vendored_fixture() {
        let fixture_fds = compile_reflection_proto();
        let fixture_pool = TestDescriptorPool::from_file_descriptor_set(fixture_fds)
            .expect("fixture should compile");
        let embedded_pool = reflection_descriptor_pool(ReflectionVersion::V1)
            .expect("embedded V1_PROTO should compile");

        for name in [
            "grpc.reflection.v1.ServerReflectionRequest",
            "grpc.reflection.v1.ServerReflectionResponse",
            "grpc.reflection.v1.FileDescriptorResponse",
            "grpc.reflection.v1.ListServiceResponse",
            "grpc.reflection.v1.ServiceResponse",
            "grpc.reflection.v1.ErrorResponse",
        ] {
            assert!(
                fixture_pool.get_message_by_name(name).is_some(),
                "fixture pool missing {name}"
            );
            assert!(
                embedded_pool.get_message_by_name(name).is_some(),
                "embedded pool missing {name}"
            );
        }
        // Sanity: FIXTURES_ROOT really is a test-only path, never touched by
        // the embedded-schema path above (which used include_str! at compile
        // time, not a runtime fixture read).
        assert!(std::path::Path::new(FIXTURES_ROOT).exists());
    }

    #[test]
    fn v1alpha_proto_compiles_and_has_matching_field_numbers() {
        let pool = reflection_descriptor_pool(ReflectionVersion::V1Alpha)
            .expect("embedded V1ALPHA_PROTO should compile");
        let req = pool
            .get_message_by_name("grpc.reflection.v1alpha.ServerReflectionRequest")
            .expect("v1alpha ServerReflectionRequest should be present");

        let host = req.get_field_by_name("host").expect("host field");
        assert_eq!(host.number(), 1);
        let file_containing_symbol = req
            .get_field_by_name("file_containing_symbol")
            .expect("file_containing_symbol field");
        assert_eq!(file_containing_symbol.number(), 4);
        let list_services = req
            .get_field_by_name("list_services")
            .expect("list_services field");
        assert_eq!(list_services.number(), 7);

        let resp = pool
            .get_message_by_name("grpc.reflection.v1alpha.ServerReflectionResponse")
            .expect("v1alpha ServerReflectionResponse should be present");
        assert_eq!(
            resp.get_field_by_name("file_descriptor_response")
                .expect("file_descriptor_response field")
                .number(),
            4
        );
        assert_eq!(
            resp.get_field_by_name("list_services_response")
                .expect("list_services_response field")
                .number(),
            6
        );
        assert_eq!(
            resp.get_field_by_name("error_response")
                .expect("error_response field")
                .number(),
            7
        );
    }

    #[test]
    fn list_services_request_round_trips_through_encode_decode_v1() {
        let msg = build_list_services_request(ReflectionVersion::V1, "example.com")
            .expect("request should build");
        let bytes = msg.encode_to_vec();

        let desc = request_descriptor(ReflectionVersion::V1).expect("descriptor");
        let decoded = DynamicMessage::decode(desc, bytes.as_slice())
            .expect("encoded request should decode cleanly");

        assert_eq!(
            decoded.get_field_by_name("host").unwrap().as_str(),
            Some("example.com")
        );
        assert!(decoded.has_field_by_name("list_services"));
    }

    #[test]
    fn list_services_request_round_trips_through_encode_decode_v1alpha() {
        let msg = build_list_services_request(ReflectionVersion::V1Alpha, "example.com")
            .expect("request should build");
        let bytes = msg.encode_to_vec();

        let desc = request_descriptor(ReflectionVersion::V1Alpha).expect("descriptor");
        let decoded = DynamicMessage::decode(desc, bytes.as_slice())
            .expect("encoded request should decode cleanly");

        assert_eq!(
            decoded.get_field_by_name("host").unwrap().as_str(),
            Some("example.com")
        );
        assert!(decoded.has_field_by_name("list_services"));
    }

    #[test]
    fn file_containing_symbol_request_round_trips_v1() {
        let msg = build_file_containing_symbol_request(
            ReflectionVersion::V1,
            "",
            "grpc.reflection.v1.ServerReflection",
        )
        .expect("request should build");
        let bytes = msg.encode_to_vec();

        let desc = request_descriptor(ReflectionVersion::V1).expect("descriptor");
        let decoded = DynamicMessage::decode(desc, bytes.as_slice())
            .expect("encoded request should decode cleanly");

        assert_eq!(
            decoded
                .get_field_by_name("file_containing_symbol")
                .unwrap()
                .as_str(),
            Some("grpc.reflection.v1.ServerReflection")
        );
    }

    #[test]
    fn file_containing_symbol_request_round_trips_v1alpha() {
        let msg = build_file_containing_symbol_request(
            ReflectionVersion::V1Alpha,
            "",
            "grpc.reflection.v1alpha.ServerReflection",
        )
        .expect("request should build");
        let bytes = msg.encode_to_vec();

        let desc = request_descriptor(ReflectionVersion::V1Alpha).expect("descriptor");
        let decoded = DynamicMessage::decode(desc, bytes.as_slice())
            .expect("encoded request should decode cleanly");

        assert_eq!(
            decoded
                .get_field_by_name("file_containing_symbol")
                .unwrap()
                .as_str(),
            Some("grpc.reflection.v1alpha.ServerReflection")
        );
    }

    fn build_list_services_response(
        version: ReflectionVersion,
        service_names: &[&str],
    ) -> DynamicMessage {
        let pool = reflection_descriptor_pool(version).expect("pool");
        let resp_desc = response_descriptor(version).expect("response descriptor");
        let list_resp_desc = pool
            .get_message_by_name(&format!("{}.ListServiceResponse", version.package()))
            .expect("ListServiceResponse descriptor");
        let service_desc = pool
            .get_message_by_name(&format!("{}.ServiceResponse", version.package()))
            .expect("ServiceResponse descriptor");

        let services: Vec<Value> = service_names
            .iter()
            .map(|name| {
                let mut svc = DynamicMessage::new(service_desc.clone());
                svc.set_field_by_name("name", Value::String(name.to_string()));
                Value::Message(svc)
            })
            .collect();

        let mut list_resp = DynamicMessage::new(list_resp_desc);
        list_resp.set_field_by_name("service", Value::List(services));

        let mut resp = DynamicMessage::new(resp_desc);
        resp.set_field_by_name("list_services_response", Value::Message(list_resp));
        resp
    }

    #[test]
    fn parses_list_services_response() {
        let resp = build_list_services_response(ReflectionVersion::V1, &["pkg.Foo", "pkg.Bar"]);
        let parsed = parse_response(&resp);
        assert_eq!(
            parsed,
            ReflectionResponse::ListServices(vec![
                ServiceInfo {
                    name: "pkg.Foo".to_string()
                },
                ServiceInfo {
                    name: "pkg.Bar".to_string()
                },
            ])
        );
    }

    #[test]
    fn decode_response_round_trips_list_services_v1alpha() {
        let resp = build_list_services_response(ReflectionVersion::V1Alpha, &["pkg.Svc"]);
        let bytes = resp.encode_to_vec();

        let parsed = decode_response(ReflectionVersion::V1Alpha, &bytes)
            .expect("decode should succeed");
        assert_eq!(
            parsed,
            ReflectionResponse::ListServices(vec![ServiceInfo {
                name: "pkg.Svc".to_string()
            }])
        );
    }

    #[test]
    fn parses_file_descriptor_response() {
        let pool = reflection_descriptor_pool(ReflectionVersion::V1).expect("pool");
        let resp_desc = response_descriptor(ReflectionVersion::V1).expect("response descriptor");
        let fdr_desc = pool
            .get_message_by_name("grpc.reflection.v1.FileDescriptorResponse")
            .expect("FileDescriptorResponse descriptor");

        let payload_a = vec![0xAAu8, 0xBB];
        let payload_b = vec![0xCCu8, 0xDD, 0xEE];

        let mut fdr = DynamicMessage::new(fdr_desc);
        fdr.set_field_by_name(
            "file_descriptor_proto",
            Value::List(vec![
                Value::Bytes(payload_a.clone().into()),
                Value::Bytes(payload_b.clone().into()),
            ]),
        );

        let mut resp = DynamicMessage::new(resp_desc);
        resp.set_field_by_name("file_descriptor_response", Value::Message(fdr));

        let parsed = parse_response(&resp);
        assert_eq!(
            parsed,
            ReflectionResponse::FileDescriptors(vec![payload_a, payload_b])
        );
    }

    #[test]
    fn parses_error_response() {
        let pool = reflection_descriptor_pool(ReflectionVersion::V1).expect("pool");
        let resp_desc = response_descriptor(ReflectionVersion::V1).expect("response descriptor");
        let err_desc = pool
            .get_message_by_name("grpc.reflection.v1.ErrorResponse")
            .expect("ErrorResponse descriptor");

        let mut err = DynamicMessage::new(err_desc);
        err.set_field_by_name("error_code", Value::I32(12));
        err.set_field_by_name(
            "error_message",
            Value::String("unimplemented".to_string()),
        );

        let mut resp = DynamicMessage::new(resp_desc);
        resp.set_field_by_name("error_response", Value::Message(err));

        let parsed = parse_response(&resp);
        assert_eq!(
            parsed,
            ReflectionResponse::Error(ReflectionError {
                code: 12,
                message: "unimplemented".to_string(),
            })
        );
    }

    #[test]
    fn parses_extension_numbers_response() {
        let pool = reflection_descriptor_pool(ReflectionVersion::V1).expect("pool");
        let resp_desc = response_descriptor(ReflectionVersion::V1).expect("response descriptor");
        let ext_desc = pool
            .get_message_by_name("grpc.reflection.v1.ExtensionNumberResponse")
            .expect("ExtensionNumberResponse descriptor");

        let mut ext = DynamicMessage::new(ext_desc);
        ext.set_field_by_name(
            "base_type_name",
            Value::String("pkg.Base".to_string()),
        );
        ext.set_field_by_name(
            "extension_number",
            Value::List(vec![Value::I32(100), Value::I32(101)]),
        );

        let mut resp = DynamicMessage::new(resp_desc);
        resp.set_field_by_name("all_extension_numbers_response", Value::Message(ext));

        let parsed = parse_response(&resp);
        assert_eq!(
            parsed,
            ReflectionResponse::ExtensionNumbers {
                base_type_name: "pkg.Base".to_string(),
                extension_number: vec![100, 101],
            }
        );
    }

    #[test]
    fn parses_unset_response_without_panicking() {
        let resp_desc = response_descriptor(ReflectionVersion::V1).expect("response descriptor");
        let resp = DynamicMessage::new(resp_desc);
        assert_eq!(parse_response(&resp), ReflectionResponse::Unset);
    }

    #[test]
    fn retries_on_v1alpha_when_v1_service_is_unavailable() {
        assert!(should_retry_on_v1alpha(
            ReflectionAttemptOutcome::ServiceUnavailable
        ));
    }

    #[test]
    fn retries_on_v1alpha_when_v1_reports_unimplemented() {
        assert!(should_retry_on_v1alpha(
            ReflectionAttemptOutcome::ErrorResponse {
                code: GRPC_STATUS_UNIMPLEMENTED
            }
        ));
    }

    #[test]
    fn does_not_retry_on_success() {
        assert!(!should_retry_on_v1alpha(ReflectionAttemptOutcome::Success));
    }

    #[test]
    fn does_not_retry_on_not_found() {
        // NOT_FOUND (5) from a ServerReflectionInfo error_response means the
        // queried symbol/file doesn't exist — a legitimate answer from a
        // fully-working v1 service, not "v1 is unsupported." Retrying on
        // v1alpha here would just reproduce the same NOT_FOUND.
        assert!(!should_retry_on_v1alpha(
            ReflectionAttemptOutcome::ErrorResponse { code: 5 }
        ));
    }

    #[test]
    fn does_not_retry_on_unrelated_error_codes() {
        // INVALID_ARGUMENT (3) and PERMISSION_DENIED (7): the v1 service
        // exists and answered, just unfavorably — retrying on v1alpha
        // wouldn't help and would mask the real error.
        assert!(!should_retry_on_v1alpha(
            ReflectionAttemptOutcome::ErrorResponse { code: 3 }
        ));
        assert!(!should_retry_on_v1alpha(
            ReflectionAttemptOutcome::ErrorResponse { code: 7 }
        ));
    }

    #[test]
    fn service_path_is_correct_per_version() {
        assert_eq!(
            ReflectionVersion::V1.service_path(),
            "/grpc.reflection.v1.ServerReflection/ServerReflectionInfo"
        );
        assert_eq!(
            ReflectionVersion::V1Alpha.service_path(),
            "/grpc.reflection.v1alpha.ServerReflection/ServerReflectionInfo"
        );
    }

    #[test]
    fn descriptor_pool_from_file_descriptors_builds_a_queryable_pool() {
        // Build a tiny, self-contained FileDescriptorProto by compiling a
        // throwaway one-message schema, then feed its serialized bytes back
        // through descriptor_pool_from_file_descriptors — exercising the
        // same bytes-to-DescriptorPool path #29 will use on reflection
        // responses, without needing a live server.
        let mut files: ProtoFileSet = BTreeMap::new();
        files.insert(
            "tiny.proto".to_string(),
            "syntax = \"proto3\";\npackage tiny;\nmessage Tiny { string name = 1; }\n"
                .to_string(),
        );
        let source_pool = compile_proto_set(&files, &["tiny.proto".to_string()])
            .expect("tiny schema should compile");
        let tiny_file = source_pool
            .files()
            .find(|f| f.name() == "tiny.proto")
            .expect("tiny.proto should be in the source pool");
        let fdp_bytes = tiny_file.file_descriptor_proto().encode_to_vec();

        let rebuilt = descriptor_pool_from_file_descriptors(&[fdp_bytes])
            .expect("rebuilding a pool from one FileDescriptorProto's bytes should succeed");
        assert!(rebuilt.get_message_by_name("tiny.Tiny").is_some());
    }

    #[test]
    fn descriptor_pool_from_file_descriptors_errors_cleanly_on_garbage_bytes() {
        let err = descriptor_pool_from_file_descriptors(&[vec![0xFF, 0xFF, 0xFF]])
            .expect_err("garbage bytes should fail to decode as a FileDescriptorProto, not panic");
        assert!(err.to_string().contains("FileDescriptorProto"));
    }
}
