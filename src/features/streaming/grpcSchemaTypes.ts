//! Provisional gRPC schema-discovery types. Task #33 will mirror these into
//! `src/lib/types.ts` from the Rust `model::grpc` DTOs once the backend lands
//! (#26 reflection / #27 proto upload). Kept local to this feature folder so
//! the `GrpcSchemaPicker` can ship against a mock IPC before the backend
//! exists. DO NOT edit `src/lib/types.ts` from this task.

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
 * Mock IPC surface — task #33 replaces the local dispatcher with a real
 * `invoke("grpc_discover_schema", ...)`. Kept here, not in `ipc.ts`, so the
 * frontend can evolve before the backend contract is frozen.
 */
export interface GrpcSchemaDiscoveryArgs {
  mode: GrpcSchemaSource;
  /** Required for "reflection" (e.g. "localhost:50051"). */
  target?: string;
  /** Required for "proto-upload" (raw `.proto` text). */
  protoContent?: string;
  protoFileName?: string;
}