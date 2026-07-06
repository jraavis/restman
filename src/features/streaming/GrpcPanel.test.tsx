//! Tests for the GrpcPanel component.
//!
//! Mirrors the testing-library + mocked-ipc pattern from
//! `WsPanel.test.tsx`/`SsePanel.test.tsx`, plus a mock of `./grpcSchemaIpc`
//! since `GrpcPanel` renders the real `GrpcSchemaPicker`, which runs its own
//! (separately mocked) discovery flow.

import type { ReactElement, ReactNode } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ipc } from "../../lib/ipc";
import type { GrpcEvent, SavedRequest } from "../../lib/types";
import { GrpcPanel } from "./GrpcPanel";
import { discoverGrpcSchema } from "./grpcSchemaIpc";
import type { GrpcSchema } from "./grpcSchemaTypes";

vi.mock("../../lib/ipc", () => ({
  ipc: {
    grpcConnect: vi.fn(),
    grpcSend: vi.fn(),
    grpcFinishSending: vi.fn(),
    streamDisconnect: vi.fn(),
    listCollections: vi.fn().mockResolvedValue([]),
    createRequest: vi.fn(),
    updateRequest: vi.fn(),
  },
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

const REFLECTION_SCHEMA: GrpcSchema = {
  source: "reflection",
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
          descriptorSet: "ZmFrZS1kZXNjcmlwdG9yLXNldA==",
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
  vi.mocked(ipc.listCollections).mockReset().mockResolvedValue([]);
});

function renderWithClient(ui: ReactElement) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={qc}>{children}</QueryClientProvider>
  );
  return render(ui, { wrapper });
}

function makeSavedGrpcRequest(overrides: Partial<SavedRequest> = {}): SavedRequest {
  return {
    id: "req-1",
    collectionId: "col-1",
    name: "Saved gRPC",
    method: "GRPC",
    url: "",
    headers: [],
    query: [],
    body: { mode: "none" },
    options: { timeoutSecs: 30, followRedirects: true, verifySsl: true, maxRedirects: 10, sendCookies: false },
    auth: { mode: "inherit" },
    preRequestScript: "",
    postResponseScript: "",
    kind: "grpc",
    streamConfig: {
      url: "grpc://saved.example.com:50051",
      methodFullName: "example.Greeter/SayHello",
      protoSource: 'syntax = "proto3"; service Greeter {}',
      protoFileName: "greeter.proto",
    },
    tags: [],
    sortOrder: 0,
    createdAt: 0,
    updatedAt: 0,
    lastUsedAt: null,
    ...overrides,
  };
}

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

/** Drives the embedded GrpcSchemaPicker's (default) reflection mode to select `schema`'s first method. */
async function selectMethodViaReflection(schema: GrpcSchema) {
  vi.mocked(discoverGrpcSchema).mockResolvedValue(schema);
  fireEvent.change(screen.getByPlaceholderText("localhost:50051"), {
    target: { value: "localhost:50051" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Discover" }));
  const method = schema.services[0].methods[0];
  const row = await screen.findByText(method.fullName);
  fireEvent.click(row);
}

describe("GrpcPanel", () => {
  it("renders closed (idle) by default with no transcript", () => {
    renderWithClient(<GrpcPanel workspaceId="ws1" onClose={() => {}} />);
    expect(screen.getByText("Idle")).toBeInTheDocument();
    expect(screen.getByText("No events yet.")).toBeInTheDocument();
    // No method picked yet, so the message builder (and therefore any
    // Connect/Send button) isn't rendered at all.
    expect(screen.queryByRole("button", { name: "Connect" })).toBeNull();
  });

  it("keeps Connect disabled until url, method, and proto source are all set", async () => {
    renderWithClient(<GrpcPanel workspaceId="ws1" onClose={() => {}} />);

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
    renderWithClient(<GrpcPanel workspaceId="ws1" onClose={() => {}} />);

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

  it("enables Connect for a reflection-discovered method without any proto source", async () => {
    renderWithClient(<GrpcPanel workspaceId="ws1" onClose={() => {}} />);

    await selectMethodViaReflection(REFLECTION_SCHEMA);
    typeUrl("grpc://localhost:50051");

    // No proto-source textarea should even render for a reflection-
    // discovered method (see module doc comment) — it isn't just optional.
    expect(
      screen.queryByPlaceholderText(/Paste the \.proto source to connect with/),
    ).toBeNull();
    expect(screen.getByRole("button", { name: "Connect" })).toBeEnabled();
  });

  it("calls grpcConnect with descriptorSet (not protoFiles) for a reflection-discovered method", async () => {
    vi.mocked(ipc.grpcConnect).mockResolvedValue("conn-1");
    renderWithClient(<GrpcPanel workspaceId="ws1" onClose={() => {}} />);

    await selectMethodViaReflection(REFLECTION_SCHEMA);
    typeUrl("grpc://localhost:50051");
    fireEvent.change(screen.getByTestId("grpc-field-name"), { target: { value: "Ada" } });
    fireEvent.click(screen.getByRole("button", { name: "Connect" }));

    await waitFor(() =>
      expect(ipc.grpcConnect).toHaveBeenCalledWith(
        "ws1",
        {
          url: "grpc://localhost:50051",
          methodFullName: "example.Greeter/SayHello",
          request: { name: "Ada" },
          descriptorSet: "ZmFrZS1kZXNjcmlwdG9yLXNldA==",
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
    renderWithClient(<GrpcPanel workspaceId="ws1" onClose={() => {}} />);

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
    renderWithClient(<GrpcPanel workspaceId="ws1" onClose={() => {}} />);

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
    renderWithClient(<GrpcPanel workspaceId="ws1" onClose={() => {}} />);

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
    renderWithClient(<GrpcPanel workspaceId="ws1" onClose={() => {}} />);

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
    renderWithClient(<GrpcPanel workspaceId="ws1" onClose={() => {}} />);

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
    renderWithClient(<GrpcPanel workspaceId="ws1" onClose={() => {}} />);

    await selectMethodViaProtoUpload(UNARY_SCHEMA);
    typeUrl("grpc://localhost:50051");
    typeProtoSource('syntax = "proto3";');
    fireEvent.click(screen.getByRole("button", { name: "Connect" }));
    await screen.findByText("Open");

    fireEvent.click(screen.getByRole("button", { name: "Disconnect" }));
    expect(ipc.streamDisconnect).toHaveBeenCalledWith("conn-1");
  });

  it("prefills url and proto source from a saved request's streamConfig on reopen (method still requires re-discovery)", () => {
    const saved = makeSavedGrpcRequest();
    renderWithClient(<GrpcPanel workspaceId="ws1" savedRequest={saved} onClose={() => {}} />);

    expect(screen.getByPlaceholderText("grpc://localhost:50051")).toHaveValue(
      "grpc://saved.example.com:50051",
    );
    expect(screen.getByRole("heading", { name: /Saved gRPC/ })).toBeInTheDocument();
    expect(
      screen.getByPlaceholderText(/Paste the \.proto source to connect with/),
    ).toHaveValue('syntax = "proto3"; service Greeter {}');
    expect(screen.getByPlaceholderText(/Entry point filename/)).toHaveValue("greeter.proto");

    // `GrpcStreamConfig` never persists the picked method (or a reflection
    // descriptor set) — re-opening always requires re-discovery, regardless
    // of which mode originally discovered it — so the message builder (and
    // any Connect button) doesn't render until the user re-picks it.
    expect(screen.queryByRole("button", { name: "Connect" })).toBeNull();
  });

  it("updates the linked saved request in place instead of opening the save dialog", async () => {
    vi.mocked(ipc.updateRequest).mockResolvedValue(makeSavedGrpcRequest());
    const saved = makeSavedGrpcRequest();
    renderWithClient(<GrpcPanel workspaceId="ws1" savedRequest={saved} onClose={() => {}} />);

    typeUrl("grpc://changed.example.com:50051");
    fireEvent.click(screen.getByRole("button", { name: /Save/ }));

    await waitFor(() =>
      expect(ipc.updateRequest).toHaveBeenCalledWith(
        "req-1",
        expect.objectContaining({
          kind: "grpc",
          streamConfig: {
            url: "grpc://changed.example.com:50051",
            methodFullName: null,
            protoSource: 'syntax = "proto3"; service Greeter {}',
            protoFileName: "greeter.proto",
          },
        }),
      ),
    );
    expect(screen.queryByText("Save request")).toBeNull();
  });
});
