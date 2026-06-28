//! MOCK gRPC schema discovery. Temporarily stands in for the real Tauri IPC
//! wrapper that task #33 will add to `src/lib/ipc.ts` (e.g.
//! `ipc.grpcDiscoverSchema`). DO NOT touch `ipc.ts` from this task. Returns
//! hardcoded fake `GrpcSchema` data after a tiny delay to simulate latency,
//! so the component and tests can ship before the reflection (#26) and
//! proto-upload (#27) backends exist.

import type {
  GrpcSchema,
  GrpcSchemaDiscoveryArgs,
} from "./grpcSchemaTypes";

/** Fake schema returned for "reflection" mode. */
export const REFLECTION_FAKE_SCHEMA: GrpcSchema = {
  source: "reflection",
  services: [
    {
      name: "grpc.reflection.v1.ServerReflection",
      methods: [
        {
          serviceName: "grpc.reflection.v1.ServerReflection",
          methodName: "ServerReflectionInfo",
          fullName: "grpc.reflection.v1.ServerReflection/ServerReflectionInfo",
          streamingType: "bidi",
          inputFields: [
            { name: "host", type: "string", repeated: false },
            { name: "list_services", type: "string", repeated: true },
          ],
          outputFields: [
            { name: "valid_host", type: "string", repeated: false },
            { name: "service", type: "message", repeated: true, messageTypeName: "ServiceResponse" },
          ],
        },
      ],
    },
    {
      name: "grpc.health.v1.Health",
      methods: [
        {
          serviceName: "grpc.health.v1.Health",
          methodName: "Check",
          fullName: "grpc.health.v1.Health/Check",
          streamingType: "unary",
          inputFields: [{ name: "service", type: "string", repeated: false }],
          outputFields: [{ name: "status", type: "string", repeated: false }],
        },
      ],
    },
  ],
};

/** Fake schema returned for "proto-upload" mode. */
export const PROTO_FAKE_SCHEMA: GrpcSchema = {
  source: "proto-upload",
  services: [
    {
      name: "example.Greeter",
      methods: [
        {
          serviceName: "example.Greeter",
          methodName: "SayHello",
          fullName: "example.Greeter/SayHello",
          streamingType: "unary",
          inputFields: [{ name: "name", type: "string", repeated: false }],
          outputFields: [{ name: "message", type: "string", repeated: false }],
        },
        {
          serviceName: "example.Greeter",
          methodName: "SayHelloStream",
          fullName: "example.Greeter/SayHelloStream",
          streamingType: "server-streaming",
          inputFields: [{ name: "name", type: "string", repeated: false }],
          outputFields: [{ name: "message", type: "string", repeated: true }],
        },
      ],
    },
  ],
};

/**
 * Mock discovery entry point. Task #33 will replace this with a real
 * `invoke("grpc_discover_schema", args)` and move it into `ipc.ts`. Returns
 * hardcoded fake data based on `args.mode`.
 */
export async function discoverGrpcSchema(
  args: GrpcSchemaDiscoveryArgs,
): Promise<GrpcSchema> {
  await new Promise((r) => setTimeout(r, 50));
  if (args.mode === "proto-upload") return PROTO_FAKE_SCHEMA;
  return REFLECTION_FAKE_SCHEMA;
}