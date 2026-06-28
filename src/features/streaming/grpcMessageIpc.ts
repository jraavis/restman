//! MOCK gRPC message building IPC. Temporarily stands in for the real Tauri
//! IPC wrapper that task #33 will add to `src/lib/ipc.ts` (e.g.
//! `ipc.grpcGetMethodDescriptor` / `ipc.grpcInvokeMethod`). DO NOT touch
//! `ipc.ts` from this task. Returns hardcoded fake descriptors and canned
//! responses after a tiny delay to simulate latency, so the
//! `GrpcMessageBuilder` component and its tests can ship before the
//! reflection (#26) / proto-upload (#27) / invoke (#34) backends exist.

import type { GrpcMethodDescriptor } from "./grpcSchemaTypes";

/**
 * Fake descriptor for `example.Greeter/SayHello` (unary). Used by callers
 * that don't yet have a real descriptor from `discoverGrpcSchema`. The
 * nested `details` message field is left as a JSON sub-editor — its
 * sub-fields are NOT expanded in this mock (out of scope for #32).
 */
export const FAKE_SAY_HELLO_DESCRIPTOR: GrpcMethodDescriptor = {
  serviceName: "example.Greeter",
  methodName: "SayHello",
  fullName: "example.Greeter/SayHello",
  streamingType: "unary",
  inputFields: [
    { name: "name", type: "string", repeated: false },
    { name: "greeting_prefix", type: "string", repeated: false },
    {
      name: "details",
      type: "message",
      repeated: false,
      messageTypeName: "example.HelloDetails",
    },
  ],
  outputFields: [{ name: "message", type: "string", repeated: false }],
};

/**
 * Mock descriptor fetch. Task #33 replaces this with a real
 * `invoke("grpc_get_method_descriptor", { methodFullName })`.
 */
export async function getMockMethodDescriptor(
  methodFullName: string,
): Promise<GrpcMethodDescriptor> {
  await new Promise((r) => setTimeout(r, 50));
  if (methodFullName === "example.Greeter/SayHello") return FAKE_SAY_HELLO_DESCRIPTOR;
  // Fall back to an Empty unary method for unknown names.
  return {
    serviceName: methodFullName.split("/")[0] ?? methodFullName,
    methodName: methodFullName.split("/")[1] ?? methodFullName,
    fullName: methodFullName,
    streamingType: "unary",
    inputFields: [],
    outputFields: [],
  };
}

/**
 * Mock invoke. Task #33 replaces this with a real
 * `invoke("grpc_invoke_method", { methodFullName, requestJson })` and wires
 * it to the streaming transport. The `GrpcMessageBuilder` itself does NOT
 * call this — its "Invoke" button emits JSON via the `onSend` prop callback
 * — but it's included here for completeness so callers outside the builder
 * have a single mocked IPC surface for #32.
 */
export async function invokeGrpcMethod(
  _methodFullName: string,
  _requestJson: string,
): Promise<{ status: string; responseJson: string }> {
  await new Promise((r) => setTimeout(r, 50));
  return {
    status: "ok",
    responseJson: JSON.stringify({ message: "Hello, mock!" }, null, 2),
  };
}