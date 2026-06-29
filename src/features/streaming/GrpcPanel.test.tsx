//! Tests for the GrpcPanel component.
//!
//! NOTE: this file is hand-traced against `GrpcPanel.tsx`'s logic, not
//! run — `npx vitest run` cannot start in this sandbox (`ERR_REQUIRE_ESM`
//! from `html-encoding-sniffer`/`@exodus/bytes`, pre-existing in
//! `node_modules`, unrelated to this task; see PLAN.md "How to resume in a
//! new session"). Mirrors the testing-library + mocked-ipc pattern from
//! `WsPanel.test.tsx`/`SsePanel.test.tsx`, plus a mock of `./grpcSchemaIpc`
//! since `GrpcPanel` renders the real `GrpcSchemaPicker`, which runs its own
//! (separately mocked) discovery flow.

import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ipc } from "../../lib/ipc";
import type { GrpcEvent } from "../../lib/types";
import { GrpcPanel } from "./GrpcPanel";
import { discoverGrpcSchema } from "./grpcSchemaIpc";
import type { GrpcSchema } from "./grpcSchemaTypes";

vi.mock("../../lib/ipc", () => ({
  ipc: { grpcConnect: vi.fn(), grpcSend: vi.fn(), grpcFinishSending: vi.fn(), streamDisconnect: vi.fn() },
}));

vi.mock("./grpcSchemaIpc", () => ({
  discoverGrpcSchema: vi.fn(),
}));

const UNARY_SCHEMA: GrpcSchema = {
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
      ],
    },
  ],
};

const BIDI_SCHEMA: GrpcSchema = {
  source: "proto-upload",
  services: [
    {
      name: "example.Chat",
      methods: [
        {
          serviceName: "example.Chat",
          methodName: "Stream",
          fullName: "example.Chat/Stream",
          streamingType: "bidi",
          inputFields: [{ name: "text", type: "string", repeated: false }],
          outputFields: [{ name: "text", type: "string", repeated: false }],
        },
      ],
    },
  ],
};

beforeEach(() => {
  vi.mocked(ipc.grpcConnect).mockReset();
  vi.mocked(ipc.grpcSend).mockReset();
  vi.mocked(ipc.grpcFinishSending).mockReset();
  vi.mocked(ipc.streamDisconnect).mockReset();
  vi.mocked(discoverGrpcSchema).mockReset();
});

function typeUrl(url: string) {
  fireEvent.change(screen.getByPlaceholderText("grpc://localhost:50051"), {
    target: { value: url },
  });
}

function typeProtoSource(text: string) {
  fireEvent.change(
    screen.getByPlaceholderText(/Paste the \.proto source to connect with/),
    { target: { value: text } },
  );
}

/** Drives the embedded GrpcSchemaPicker's proto-upload mode to select `schema`'s first method. */
async function selectMethodViaProtoUpload(schema: GrpcSchema) {
  vi.mocked(discoverGrpcSchema).mockResolvedValue(schema);
  fireEvent.click(screen.getByRole("button", { name: ".proto Upload" }));
  fireEvent.change(screen.getByPlaceholderText(/syntax = "proto3"/), {
    target: { value: "syntax = \"proto3\"; service X {}" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Compile" }));
  const method = schema.services[0].methods[0];
  const row = await screen.findByText(method.fullName);
  fireEvent.click(row);
}

describe("GrpcPanel", () => {
  it("renders closed (idle) by default with no transcript", () => {
    render(<GrpcPanel workspaceId="ws1" onClose={() => {}} />);
    expect(screen.getByText("Idle")).toBeInTheDocument();
    expect(screen.getByText("No events yet.")).toBeInTheDocument();
    // No method picked yet, so the message builder (and therefore any
    // Connect/Send button) isn't rendered at all.
    expect(screen.queryByRole("button", { name: "Connect" })).toBeNull();
  });

  it("keeps Connect disabled until url, method, and proto source are all set", async () => {
    render(<GrpcPanel workspaceId="ws1" onClose={() => {}} />);

    // Two embedded proto-upload textareas exist (the picker's own discovery
    // textarea, and the panel's connect-time proto-source textarea) once
    // proto-upload mode is selected — disambiguate via the picker's mode
    // toggle vs. the panel's own "(set)" indicator below.
    await selectMethodViaProtoUpload(UNARY_SCHEMA);

    // Method is now selected, so GrpcMessageBuilder (and its Connect-labeled
    // button) renders — but url is empty and the panel's own proto-source
    // textarea is empty, so Connect must stay disabled.
    expect(screen.getByRole("button", { name: "Connect" })).toBeDisabled();

    typeUrl("grpc://localhost:50051");
    expect(screen.getByRole("button", { name: "Connect" })).toBeDisabled();

    // Fill the panel's own connect-time proto source (the textarea inside
    // the panel's ".proto source for connect" <details>, distinct from the
    // picker's own proto-upload textarea above it).
    typeProtoSource('syntax = "proto3"; package example; service Greeter { ... }');
    expect(screen.getByRole("button", { name: "Connect" })).toBeEnabled();
  });

  it("calls grpcConnect with protoFiles/entryPoint/methodFullName/parsed request on Connect", async () => {
    vi.mocked(ipc.grpcConnect).mockResolvedValue("conn-1");
    render(<GrpcPanel workspaceId="ws1" onClose={() => {}} />);

    await selectMethodViaProtoUpload(UNARY_SCHEMA);
    typeUrl("grpc://localhost:50051");
    typeProtoSource('syntax = "proto3";');

    // GrpcMessageBuilder renders one text input per inputField
    // (`data-testid="grpc-field-name"`) — fill it, then click the builder's
    // own button (now labeled "Connect" since status !== "open").
    fireEvent.change(screen.getByTestId("grpc-field-name"), { target: { value: "Ada" } });
    fireEvent.click(screen.getByRole("button", { name: "Connect" }));

    await waitFor(() =>
      expect(ipc.grpcConnect).toHaveBeenCalledWith(
        "ws1",
        {
          url: "grpc://localhost:50051",
          methodFullName: "example.Greeter/SayHello",
          request: { name: "Ada" },
          protoFiles: { "main.proto": 'syntax = "proto3";' },
          entryPoint: "main.proto",
        },
        expect.any(Function),
      ),
    );
  });

  it("renders Open/Response/Status/Error/Closed events distinctly in the transcript", async () => {
    vi.mocked(ipc.grpcConnect).mockImplementation(async (_ws, _args, onEvent) => {
      onEvent({ type: "open" } satisfies GrpcEvent);
      onEvent({ type: "response", message: { message: "hi" } } satisfies GrpcEvent);
      onEvent({ type: "status", code: 0, message: null } satisfies GrpcEvent);
      onEvent({ type: "closed" } satisfies GrpcEvent);
      return "conn-1";
    });
    render(<GrpcPanel workspaceId="ws1" onClose={() => {}} />);

    await selectMethodViaProtoUpload(UNARY_SCHEMA);
    typeUrl("grpc://localhost:50051");
    typeProtoSource('syntax = "proto3";');
    fireEvent.change(screen.getByTestId("grpc-field-name"), { target: { value: "Ada" } });
    fireEvent.click(screen.getByRole("button", { name: "Connect" }));

    expect(await screen.findByText("Connection opened")).toBeInTheDocument();
    expect(screen.getByText(/"message": "hi"/)).toBeInTheDocument();
    expect(screen.getByText("Status OK")).toBeInTheDocument();
    expect(screen.getByText("Connection closed")).toBeInTheDocument();
  });

  it("renders a non-zero Status code with its message, marked as an error row", async () => {
    vi.mocked(ipc.grpcConnect).mockImplementation(async (_ws, _args, onEvent) => {
      onEvent({ type: "open" } satisfies GrpcEvent);
      onEvent({ type: "status", code: 5, message: "not found" } satisfies GrpcEvent);
      onEvent({ type: "closed" } satisfies GrpcEvent);
      return "conn-1";
    });
    render(<GrpcPanel workspaceId="ws1" onClose={() => {}} />);

    await selectMethodViaProtoUpload(UNARY_SCHEMA);
    typeUrl("grpc://localhost:50051");
    typeProtoSource('syntax = "proto3";');
    fireEvent.click(screen.getByRole("button", { name: "Connect" }));

    expect(await screen.findByText("Status 5: not found")).toBeInTheDocument();
  });

  it("renders an Error event with no preceding Open (transport-level send failure)", async () => {
    vi.mocked(ipc.grpcConnect).mockImplementation(async (_ws, _args, onEvent) => {
      onEvent({ type: "error", message: "gRPC TCP connect failed" } satisfies GrpcEvent);
      return "conn-1";
    });
    render(<GrpcPanel workspaceId="ws1" onClose={() => {}} />);

    await selectMethodViaProtoUpload(UNARY_SCHEMA);
    typeUrl("grpc://localhost:50051");
    typeProtoSource('syntax = "proto3";');
    fireEvent.click(screen.getByRole("button", { name: "Connect" }));

    expect(await screen.findByText("Error: gRPC TCP connect failed")).toBeInTheDocument();
    expect(screen.queryByText("Connection opened")).toBeNull();
    expect(await screen.findByText("Error")).toBeInTheDocument(); // status badge
  });

  it("hides the send composer for a unary exchange once open", async () => {
    vi.mocked(ipc.grpcConnect).mockImplementation(async (_ws, _args, onEvent) => {
      onEvent({ type: "open" } satisfies GrpcEvent);
      return "conn-1";
    });
    render(<GrpcPanel workspaceId="ws1" onClose={() => {}} />);

    await selectMethodViaProtoUpload(UNARY_SCHEMA);
    typeUrl("grpc://localhost:50051");
    typeProtoSource('syntax = "proto3";');
    fireEvent.click(screen.getByRole("button", { name: "Connect" }));
    await screen.findByText("Open");

    // Builder is still rendered (relabeled to "Send" now that status ===
    // "open"), but `canSend` is false for unary (supportsSend is false), so
    // the Send button must be disabled, and "Finish sending" must not
    // render at all.
    expect(screen.getByRole("button", { name: "Send" })).toBeDisabled();
    expect(screen.queryByRole("button", { name: "Finish sending" })).toBeNull();
  });

  it("shows an enabled send composer + Finish sending for a bidi exchange once open", async () => {
    vi.mocked(ipc.grpcConnect).mockImplementation(async (_ws, _args, onEvent) => {
      onEvent({ type: "open" } satisfies GrpcEvent);
      return "conn-1";
    });
    vi.mocked(ipc.grpcSend).mockResolvedValue(undefined);
    vi.mocked(ipc.grpcFinishSending).mockResolvedValue(undefined);
    render(<GrpcPanel workspaceId="ws1" onClose={() => {}} />);

    await selectMethodViaProtoUpload(BIDI_SCHEMA);
    typeUrl("grpc://localhost:50051");
    typeProtoSource('syntax = "proto3";');
    fireEvent.click(screen.getByRole("button", { name: "Connect" }));
    await screen.findByText("Open");

    // Now status === "open" and streamingType === "bidi" -> supportsSend
    // true -> canSend true -> Send enabled, Finish sending rendered.
    expect(screen.getByRole("button", { name: "Send" })).toBeEnabled();
    const finishBtn = screen.getByRole("button", { name: "Finish sending" });
    expect(finishBtn).toBeInTheDocument();

    fireEvent.change(screen.getByTestId("grpc-field-text"), { target: { value: "hello" } });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));

    await waitFor(() =>
      expect(ipc.grpcSend).toHaveBeenCalledWith("conn-1", { request: { text: "hello" } }),
    );

    fireEvent.click(finishBtn);
    await waitFor(() => expect(ipc.grpcFinishSending).toHaveBeenCalledWith("conn-1"));
  });

  it("disconnects via the generic streamDisconnect command", async () => {
    vi.mocked(ipc.grpcConnect).mockImplementation(async (_ws, _args, onEvent) => {
      onEvent({ type: "open" } satisfies GrpcEvent);
      return "conn-1";
    });
    render(<GrpcPanel workspaceId="ws1" onClose={() => {}} />);

    await selectMethodViaProtoUpload(UNARY_SCHEMA);
    typeUrl("grpc://localhost:50051");
    typeProtoSource('syntax = "proto3";');
    fireEvent.click(screen.getByRole("button", { name: "Connect" }));
    await screen.findByText("Open");

    fireEvent.click(screen.getByRole("button", { name: "Disconnect" }));
    expect(ipc.streamDisconnect).toHaveBeenCalledWith("conn-1");
  });
});
