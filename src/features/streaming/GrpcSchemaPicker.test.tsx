//! Tests for the GrpcSchemaPicker component. Mocks the local
//! `grpcSchemaIpc` module (the stand-in for `ipc.ts` until task #33) so the
//! tests never touch the Tauri backend. Setup mirrors `WsPanel.test.tsx`.

import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  PROTO_FAKE_SCHEMA,
  REFLECTION_FAKE_SCHEMA,
  discoverGrpcSchema,
} from "./grpcSchemaIpc";
import { GrpcSchemaPicker } from "./GrpcSchemaPicker";

vi.mock("./grpcSchemaIpc", () => ({
  REFLECTION_FAKE_SCHEMA: {
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
            inputFields: [{ name: "host", type: "string", repeated: false }],
            outputFields: [],
          },
        ],
      },
    ],
  },
  PROTO_FAKE_SCHEMA: {
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
            inputFields: [],
            outputFields: [],
          },
        ],
      },
    ],
  },
  discoverGrpcSchema: vi.fn(),
}));

beforeEach(() => {
  vi.mocked(discoverGrpcSchema).mockReset();
});

function switchToProto() {
  fireEvent.click(screen.getByRole("button", { name: ".proto Upload" }));
}

describe("GrpcSchemaPicker", () => {
  it("renders the Reflection mode by default and disables Discover until target is set", () => {
    render(<GrpcSchemaPicker workspaceId="ws1" onMethodSelected={() => {}} />);
    expect(screen.getByPlaceholderText("localhost:50051")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Discover" })).toBeDisabled();

    fireEvent.change(screen.getByPlaceholderText("localhost:50051"), {
      target: { value: "localhost:50051" },
    });
    expect(screen.getByRole("button", { name: "Discover" })).toBeEnabled();
  });

  it("runs discovery and renders the discovered service + methods", async () => {
    vi.mocked(discoverGrpcSchema).mockResolvedValue(REFLECTION_FAKE_SCHEMA);
    render(<GrpcSchemaPicker workspaceId="ws1" onMethodSelected={() => {}} />);

    fireEvent.change(screen.getByPlaceholderText("localhost:50051"), {
      target: { value: "localhost:50051" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Discover" }));

    await waitFor(() =>
      expect(discoverGrpcSchema).toHaveBeenCalledWith(
        {
          mode: "reflection",
          target: "localhost:50051",
          protoContent: undefined,
          protoFileName: undefined,
        },
        "ws1",
      ),
    );
    expect(await screen.findByText("grpc.reflection.v1.ServerReflection")).toBeInTheDocument();
    expect(
      screen.getByText("grpc.reflection.v1.ServerReflection/ServerReflectionInfo"),
    ).toBeInTheDocument();
  });

  it("calls onMethodSelected with the clicked method descriptor", async () => {
    vi.mocked(discoverGrpcSchema).mockResolvedValue(REFLECTION_FAKE_SCHEMA);
    const onMethodSelected = vi.fn();
    render(<GrpcSchemaPicker workspaceId="ws1" onMethodSelected={onMethodSelected} />);

    fireEvent.change(screen.getByPlaceholderText("localhost:50051"), {
      target: { value: "localhost:50051" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Discover" }));

    const row = await screen.findByText(
      "grpc.reflection.v1.ServerReflection/ServerReflectionInfo",
    );
    fireEvent.click(row);

    expect(onMethodSelected).toHaveBeenCalledTimes(1);
    const arg = onMethodSelected.mock.calls[0][0];
    expect(arg.fullName).toBe("grpc.reflection.v1.ServerReflection/ServerReflectionInfo");
    expect(arg.streamingType).toBe("bidi");
  });

  it("proto-upload mode compiles the pasted content and renders its services", async () => {
    vi.mocked(discoverGrpcSchema).mockResolvedValue(PROTO_FAKE_SCHEMA);
    render(<GrpcSchemaPicker workspaceId="ws1" onMethodSelected={() => {}} />);

    switchToProto();

    fireEvent.change(screen.getByPlaceholderText(/syntax = "proto3"/), {
      target: { value: 'syntax = "proto3"; service Greeter {}' },
    });
    expect(screen.getByRole("button", { name: "Compile" })).toBeEnabled();

    fireEvent.click(screen.getByRole("button", { name: "Compile" }));

    await waitFor(() =>
      expect(discoverGrpcSchema).toHaveBeenCalledWith(
        expect.objectContaining({
          mode: "proto-upload",
          protoContent: 'syntax = "proto3"; service Greeter {}',
        }),
        "ws1",
      ),
    );
    expect(await screen.findByText("example.Greeter")).toBeInTheDocument();
    expect(screen.getByText("example.Greeter/SayHello")).toBeInTheDocument();
  });

  it("shows the discovering label and disables the button while loading", async () => {
    let resolveDiscovery: (v: typeof REFLECTION_FAKE_SCHEMA) => void = () => {};
    vi.mocked(discoverGrpcSchema).mockImplementation(
      () =>
        new Promise((resolve) => {
          resolveDiscovery = resolve;
        }),
    );
    render(<GrpcSchemaPicker workspaceId="ws1" onMethodSelected={() => {}} />);

    fireEvent.change(screen.getByPlaceholderText("localhost:50051"), {
      target: { value: "localhost:50051" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Discover" }));

    expect(await screen.findByText("Discovering…")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Discover" })).toBeDisabled();
    expect(screen.getByPlaceholderText("localhost:50051")).toBeDisabled();

    resolveDiscovery(REFLECTION_FAKE_SCHEMA);
    await waitFor(() => expect(screen.queryByText("Discovering…")).toBeNull());
  });

  it("renders an inline error when discovery throws", async () => {
    vi.mocked(discoverGrpcSchema).mockRejectedValue(new Error("reflection offline"));
    render(<GrpcSchemaPicker workspaceId="ws1" onMethodSelected={() => {}} />);

    fireEvent.change(screen.getByPlaceholderText("localhost:50051"), {
      target: { value: "localhost:50051" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Discover" }));

    expect(await screen.findByText("reflection offline")).toBeInTheDocument();
  });

  it("renders a file input in proto-upload mode", () => {
    render(<GrpcSchemaPicker workspaceId="ws1" onMethodSelected={() => {}} />);
    switchToProto();
    const fileInput = document.querySelector('input[type="file"]');
    expect(fileInput).not.toBeNull();
  });

  it("reads a selected .proto file into the textarea via FileReader", async () => {
    // Stub FileReader so the test is deterministic across jsdom/node.
    const fileReaderMock = {
      onerror: null as ((e: unknown) => void) | null,
      onload: null as ((e: unknown) => void) | null,
      result: "" as string,
      readAsText(this: { result: string; onload: ((e: unknown) => void) | null }, _file: File) {
        this.result = "syntax = proto3 read from disk;";
        this.onload?.({} as ProgressEvent<FileReader>);
      },
    };
    const originalFileReader = globalThis.FileReader;
    // @ts-expect-error — temporary test stub
    globalThis.FileReader = function () {
      return fileReaderMock;
    };

    try {
      render(<GrpcSchemaPicker workspaceId="ws1" onMethodSelected={() => {}} />);
      switchToProto();

      const fileInput = document.querySelector('input[type="file"]') as HTMLInputElement;
      const fakeFile = new File(["syntax = proto3 read from disk;"], "greeter.proto", {
        type: "text/plain",
      });
      fireEvent.change(fileInput, { target: { files: [fakeFile] } });

      await waitFor(() =>
        expect(screen.getByPlaceholderText(/syntax = "proto3"/)).toHaveValue(
          "syntax = proto3 read from disk;",
        ),
      );
    } finally {
      globalThis.FileReader = originalFileReader;
    }
  });
});