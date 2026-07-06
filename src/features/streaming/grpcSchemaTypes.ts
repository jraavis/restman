//! gRPC schema-discovery types. Reflection mode now dispatches to the real
//! `grpc_discover_schema` command (see `grpcSchemaIpc.ts`); proto-upload mode
//! remains mocked (deferred to a future task, per the reflection-to-connect
//! handoff's scope — it already has a working connect path via `GrpcPanel`'s
//! own proto-source textarea). Kept local to this feature folder rather than
//! merged into `src/lib/types.ts` since `GrpcSchema`/`GrpcSchemaDiscoveryArgs`
//! are UI-shape types with a still-mocked half; the real backend DTOs live in
//! `src/lib/types.ts` as `GrpcDiscoveredField`/`GrpcDiscoveredMethod`/etc.

export type GrpcSchemaSource = "reflection" | "proto-upload";

/** A single field on a protobuf message (input or output). */
export interface GrpcFieldDescriptor {
  name: string;
  /** e.g. "string", "int32", "message", "bytes", "repeated string"… */
  type: string;
  repeated: boolean;
  /** Present only when `type` is "message" — names the nested message type. */
  messageTypeName?: string;
}

/** A single RPC method exposed by a service. */
export interface GrpcMethodDescriptor {
  /** e.g. "grpc.reflection.v1.ServerReflection" */
  serviceName: string;
  /** e.g. "ServerReflectionInfo" */
  methodName: string;
  /** "{serviceName}/{methodName}" */
  fullName: string;
  streamingType: "unary" | "server-streaming" | "client-streaming" | "bidi";
  inputFields: GrpcFieldDescriptor[];
  outputFields: GrpcFieldDescriptor[];
  /**
   * Present only for methods discovered via reflection: the base64
   * `FileDescriptorSet` bytes backing this schema, passed straight through
   * to `grpcConnect`'s `descriptorSet` — the reflection-to-connect handoff.
   * Denormalized onto every method from the same discovery session (all
   * share one descriptor set) so `GrpcPanel` can connect from a picked
   * method alone, without threading a separate discovery-result prop.
   */
  descriptorSet?: string;
}

/** The full schema returned by discovery — one entry per discovered service. */
export interface GrpcSchema {
  source: GrpcSchemaSource;
  services: Array<{
    name: string;
    methods: GrpcMethodDescriptor[];
  }>;
}

/**
 * Local dispatch args for `discoverGrpcSchema` (`grpcSchemaIpc.ts`):
 * `mode: "reflection"` calls the real `ipc.grpcDiscoverSchema`; `mode:
 * "proto-upload"` still returns mocked data (see that file's doc comment).
 */
export interface GrpcSchemaDiscoveryArgs {
  mode: GrpcSchemaSource;
  /** Required for "reflection" (e.g. "localhost:50051"). */
  target?: string;
  /** Required for "proto-upload" (raw `.proto` text). */
  protoContent?: string;
  protoFileName?: string;
}