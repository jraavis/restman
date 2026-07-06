//! gRPC schema discovery dispatch. `"reflection"` mode calls the real
//! `ipc.grpcDiscoverSchema` (the reflection-to-connect handoff) and maps its
//! result onto the local `GrpcSchema` shape, stamping the discovered
//! `descriptorSet` onto every method so `GrpcPanel` can connect from a
//! picked method alone. `"proto-upload"` mode is still mocked — it already
//! has a working connect path via `GrpcPanel`'s own proto-source textarea,
//! so wiring it to a real discovery round-trip is out of this task's scope.

import { ipc } from "../../lib/ipc";
import type {
  GrpcMethodDescriptor,
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
 * Discovery entry point. `"proto-upload"` mode still returns mocked data (see
 * module doc comment); `"reflection"` mode calls the real
 * `ipc.grpcDiscoverSchema` and maps its result onto the local `GrpcSchema`
 * shape, stamping the returned `descriptorSet` onto every discovered method.
 */
export async function discoverGrpcSchema(
  args: GrpcSchemaDiscoveryArgs,
  workspaceId: string,
): Promise<GrpcSchema> {
  if (args.mode === "proto-upload") {
    await new Promise((r) => setTimeout(r, 50));
    return PROTO_FAKE_SCHEMA;
  }

  const result = await ipc.grpcDiscoverSchema(workspaceId, args.target ?? "");
  return {
    source: "reflection",
    services: result.services.map((service) => ({
      name: service.name,
      methods: service.methods.map(
        (method): GrpcMethodDescriptor => ({
          serviceName: method.serviceName,
          methodName: method.methodName,
          fullName: method.fullName,
          streamingType: method.streamingType,
          inputFields: method.inputFields.map((f) => ({
            name: f.name,
            type: f.type,
            repeated: f.repeated,
            messageTypeName: f.messageTypeName ?? undefined,
          })),
          outputFields: method.outputFields.map((f) => ({
            name: f.name,
            type: f.type,
            repeated: f.repeated,
            messageTypeName: f.messageTypeName ?? undefined,
          })),
          descriptorSet: result.descriptorSet,
        }),
      ),
    })),
  };
}