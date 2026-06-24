import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, expect, it, vi, beforeEach } from "vitest";
import { ImportDialog } from "./ImportDialog";
import { ipc } from "../../lib/ipc";
import type {
  EnvironmentPreview,
  ImportPreview,
  ImportReport,
  ImportedNode,
} from "../../lib/types";

vi.mock("../../lib/ipc", () => ({
  ipc: {
    previewImport: vi.fn(),
    applyCollectionImport: vi.fn(),
    previewEnvironmentImport: vi.fn(),
    applyEnvironmentImport: vi.fn(),
  },
}));

function makeNode(overrides: Partial<ImportedNode> = {}): ImportedNode {
  return {
    name: "Pet Store",
    description: null,
    auth: { type: "none" },
    requests: [],
    children: [],
    ...overrides,
  };
}

function makePreview(overrides: Partial<ImportPreview> = {}): ImportPreview {
  return {
    root: makeNode(),
    warnings: [],
    stats: { folders: 0, requests: 0, warnings: 0 },
    ...overrides,
  };
}

function pasteJson(value: string) {
  const textarea = screen.getByPlaceholderText(/paste collection json/i);
  fireEvent.change(textarea, { target: { value } });
  fireEvent.blur(textarea);
}

describe("ImportDialog", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders the paste/upload input step initially", () => {
    render(<ImportDialog workspaceId="ws-1" parentId={null} onClose={() => {}} />);
    expect(screen.getByPlaceholderText(/paste collection json/i)).toBeInTheDocument();
  });

  it("parses pasted JSON on blur and shows the preview tree + stats", async () => {
    const preview = makePreview({
      root: makeNode({
        name: "Pet Store",
        requests: [
          {
            name: "List pets",
            method: "GET",
            url: "https://pets.test",
            headers: [],
            query: [],
            body: { mode: "none" },
            options: { timeoutSecs: 30, followRedirects: true, verifySsl: true, maxRedirects: 10, sendCookies: false },
            auth: { mode: "inherit" },
            preRequestScript: "",
            postResponseScript: "",
          },
        ],
      }),
      stats: { folders: 1, requests: 1, warnings: 0 },
    });
    vi.mocked(ipc.previewImport).mockResolvedValue(preview);

    render(<ImportDialog workspaceId="ws-1" parentId={null} onClose={() => {}} />);
    pasteJson('{"info":{}}');

    await waitFor(() => expect(ipc.previewImport).toHaveBeenCalledWith("postman", '{"info":{}}'));
    expect(await screen.findByText("Pet Store")).toBeInTheDocument();
    expect(screen.getByText("List pets")).toBeInTheDocument();
    expect(screen.getByText("1 folders")).toBeInTheDocument();
    expect(screen.getByText("1 requests")).toBeInTheDocument();
  });

  it("shows warnings from the preview", async () => {
    vi.mocked(ipc.previewImport).mockResolvedValue(
      makePreview({ warnings: ["unsupported auth type: digest"], stats: { folders: 0, requests: 0, warnings: 1 } }),
    );
    render(<ImportDialog workspaceId="ws-1" parentId={null} onClose={() => {}} />);
    pasteJson("{}");

    expect(await screen.findByText(/unsupported auth type: digest/)).toBeInTheDocument();
  });

  it("confirms import with the selected conflict mode and shows the report", async () => {
    vi.mocked(ipc.previewImport).mockResolvedValue(makePreview());
    const report: ImportReport = {
      createdCollections: 2,
      createdRequests: 5,
      skipped: 1,
      overwritten: 0,
      warnings: [],
    };
    vi.mocked(ipc.applyCollectionImport).mockResolvedValue(report);

    render(<ImportDialog workspaceId="ws-1" parentId="col-parent" onClose={() => {}} />);
    pasteJson("{}");
    await screen.findByText("Pet Store");

    fireEvent.change(screen.getByRole("combobox"), { target: { value: "merge" } });
    fireEvent.click(screen.getByRole("button", { name: /^import$/i }));

    await waitFor(() =>
      expect(ipc.applyCollectionImport).toHaveBeenCalledWith(
        "ws-1",
        "col-parent",
        expect.objectContaining({ name: "Pet Store" }),
        "merge",
      ),
    );
    expect(await screen.findByText(/Import complete/i)).toBeInTheDocument();
    expect(screen.getByText("2 folders created")).toBeInTheDocument();
    expect(screen.getByText("5 requests created")).toBeInTheDocument();
    expect(screen.getByText(/1 skipped/)).toBeInTheDocument();
  });

  it("shows an error message when preview parsing fails", async () => {
    vi.mocked(ipc.previewImport).mockRejectedValue("invalid JSON: missing field `info`");
    render(<ImportDialog workspaceId="ws-1" parentId={null} onClose={() => {}} />);
    pasteJson("not json");

    expect(await screen.findByText(/invalid JSON/)).toBeInTheDocument();
  });

  it("offers OpenAPI as an import format and routes it to previewImport with the snake_case id", async () => {
    vi.mocked(ipc.previewImport).mockResolvedValue(makePreview());
    render(<ImportDialog workspaceId="ws-1" parentId={null} onClose={() => {}} />);
    fireEvent.change(screen.getByRole("combobox", { name: /Format/i }), { target: { value: "open_api" } });
    const textarea = screen.getByPlaceholderText(/OpenAPI\/Swagger JSON or YAML/i);
    fireEvent.change(textarea, { target: { value: "openapi: 3.0.0" } });
    fireEvent.blur(textarea);

    await waitFor(() => expect(ipc.previewImport).toHaveBeenCalledWith("open_api", "openapi: 3.0.0"));
  });

  describe("environment mode", () => {
    function makeEnvPreview(): EnvironmentPreview {
      return {
        name: "Production",
        variables: [{ key: "baseUrl", value: "https://api.prod", enabled: true, isSecret: false }],
        warnings: [],
      };
    }

    it("defaults to environment when defaultKind is set, parses via previewEnvironmentImport, and renders the variable table", async () => {
      vi.mocked(ipc.previewEnvironmentImport).mockResolvedValue(makeEnvPreview());
      render(<ImportDialog workspaceId="ws-1" parentId={null} defaultKind="environment" onClose={() => {}} />);
      const textarea = screen.getByPlaceholderText(/paste environment JSON/i);
      fireEvent.change(textarea, { target: { value: '{"name":"Production"}' } });
      fireEvent.blur(textarea);

      await waitFor(() => expect(ipc.previewEnvironmentImport).toHaveBeenCalledWith('{"name":"Production"}'));
      expect(await screen.findByText("Production")).toBeInTheDocument();
      expect(screen.getByText("baseUrl")).toBeInTheDocument();
    });

    it("confirms environment import via applyEnvironmentImport and shows the variable-created count", async () => {
      vi.mocked(ipc.previewEnvironmentImport).mockResolvedValue(makeEnvPreview());
      vi.mocked(ipc.applyEnvironmentImport).mockResolvedValue({ createdVariables: 1, overwritten: 0, warnings: [] });
      render(<ImportDialog workspaceId="ws-1" parentId="parent-col" defaultKind="environment" onClose={() => {}} />);
      const textarea = screen.getByPlaceholderText(/paste environment JSON/i);
      fireEvent.change(textarea, { target: { value: "{}" } });
      fireEvent.blur(textarea);
      await screen.findByText("Production");

      fireEvent.click(screen.getByRole("button", { name: /^import$/i }));
      await waitFor(() =>
        expect(ipc.applyEnvironmentImport).toHaveBeenCalledWith(
          "ws-1",
          "parent-col",
          expect.objectContaining({ name: "Production" }),
          false,
        ),
      );
      expect(await screen.findByText(/1 variables created/i)).toBeInTheDocument();
    });
  });
});
