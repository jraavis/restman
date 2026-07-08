import type { ReactElement, ReactNode } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
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

function renderWithClient(ui: ReactElement) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={qc}>{children}</QueryClientProvider>
  );
  return render(ui, { wrapper });
}

vi.mock("../../lib/ipc", () => ({
  ipc: {
    previewImport: vi.fn(),
    applyCollectionImport: vi.fn(),
    previewEnvironmentImport: vi.fn(),
    applyEnvironmentImport: vi.fn(),
    listPlugins: vi.fn().mockResolvedValue([]),
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
    renderWithClient(<ImportDialog workspaceId="ws-1" parentId={null} onClose={() => {}} />);
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

    renderWithClient(<ImportDialog workspaceId="ws-1" parentId={null} onClose={() => {}} />);
    pasteJson('{"info":{}}');

    await waitFor(() => expect(ipc.previewImport).toHaveBeenCalledWith('{"info":{}}', { format: "postman" }));
    expect(await screen.findByText("Pet Store")).toBeInTheDocument();
    expect(screen.getByText("List pets")).toBeInTheDocument();
    expect(screen.getByText("1 folders")).toBeInTheDocument();
    expect(screen.getByText("1 requests")).toBeInTheDocument();
  });

  it("shows warnings from the preview", async () => {
    vi.mocked(ipc.previewImport).mockResolvedValue(
      makePreview({ warnings: ["unsupported auth type: digest"], stats: { folders: 0, requests: 0, warnings: 1 } }),
    );
    renderWithClient(<ImportDialog workspaceId="ws-1" parentId={null} onClose={() => {}} />);
    pasteJson("{}");

    expect(await screen.findByText(/unsupported auth type: digest/)).toBeInTheDocument();
  });

  it("calls onImported after a successful collection import", async () => {
    vi.mocked(ipc.previewImport).mockResolvedValue(makePreview());
    vi.mocked(ipc.applyCollectionImport).mockResolvedValue({
      createdCollections: 1,
      createdRequests: 1,
      skipped: 0,
      overwritten: 0,
      warnings: [],
    });
    const onImported = vi.fn();

    renderWithClient(
      <ImportDialog workspaceId="ws-1" parentId="col-parent" onClose={() => {}} onImported={onImported} />,
    );
    pasteJson("{}");
    await screen.findByText("Pet Store");
    fireEvent.click(screen.getByRole("button", { name: /^import$/i }));

    await waitFor(() => expect(onImported).toHaveBeenCalledTimes(1));
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

    renderWithClient(<ImportDialog workspaceId="ws-1" parentId="col-parent" onClose={() => {}} />);
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
        "as_subfolder",
      ),
    );
    expect(await screen.findByText(/Import complete/i)).toBeInTheDocument();
    expect(screen.getByText("2 folders created")).toBeInTheDocument();
    expect(screen.getByText("5 requests created")).toBeInTheDocument();
    expect(screen.getByText(/1 skipped/)).toBeInTheDocument();
  });

  it("shows placement options when importing into a folder and defaults curl to into_folder", async () => {
    renderWithClient(
      <ImportDialog workspaceId="ws-1" parentId="col-parent" parentName="API" onClose={() => {}} />,
    );
    expect(screen.getByText(/Add to "API"/)).toBeInTheDocument();
    expect(screen.getByText("Import as subfolder")).toBeInTheDocument();
    expect(screen.getByLabelText(/Add to "API"/)).not.toBeChecked();

    fireEvent.change(screen.getByRole("combobox", { name: /Format/i }), { target: { value: "native:curl" } });
    expect(screen.getByLabelText(/Add to "API"/)).toBeChecked();
  });

  it("shows an error message when preview parsing fails", async () => {
    vi.mocked(ipc.previewImport).mockRejectedValue("invalid JSON: missing field `info`");
    renderWithClient(<ImportDialog workspaceId="ws-1" parentId={null} onClose={() => {}} />);
    pasteJson("not json");

    expect(await screen.findByText(/invalid JSON/)).toBeInTheDocument();
  });

  it("offers OpenAPI as an import format and routes it to previewImport with the snake_case id", async () => {
    vi.mocked(ipc.previewImport).mockResolvedValue(makePreview());
    renderWithClient(<ImportDialog workspaceId="ws-1" parentId={null} onClose={() => {}} />);
    fireEvent.change(screen.getByRole("combobox", { name: /Format/i }), { target: { value: "native:open_api" } });
    const textarea = screen.getByPlaceholderText(/OpenAPI\/Swagger JSON or YAML/i);
    fireEvent.change(textarea, { target: { value: "openapi: 3.0.0" } });
    fireEvent.blur(textarea);

    await waitFor(() => expect(ipc.previewImport).toHaveBeenCalledWith("openapi: 3.0.0", { format: "open_api" }));
  });

  it("offers import plugins in the format picker and routes them to previewImport by id", async () => {
    vi.mocked(ipc.listPlugins).mockResolvedValue([
      {
        id: "plug-1",
        workspaceId: "ws-1",
        name: "My Format",
        kind: "import",
        languageLabel: "My Format",
        source: "",
        enabled: true,
        createdAt: 0,
        updatedAt: 0,
      },
    ]);
    vi.mocked(ipc.previewImport).mockResolvedValue(makePreview());
    renderWithClient(<ImportDialog workspaceId="ws-1" parentId={null} onClose={() => {}} />);
    await screen.findByText("My Format");
    fireEvent.change(screen.getByRole("combobox", { name: /Format/i }), { target: { value: "plugin:plug-1" } });
    const textarea = screen.getByPlaceholderText(/paste content here/i);
    fireEvent.change(textarea, { target: { value: "anything" } });
    fireEvent.blur(textarea);

    await waitFor(() => expect(ipc.previewImport).toHaveBeenCalledWith("anything", { pluginId: "plug-1" }));
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
      renderWithClient(<ImportDialog workspaceId="ws-1" parentId={null} defaultKind="environment" onClose={() => {}} />);
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
      renderWithClient(<ImportDialog workspaceId="ws-1" parentId="parent-col" defaultKind="environment" onClose={() => {}} />);
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
