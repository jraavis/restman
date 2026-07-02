//! Tests for the restman-native export/import section (Settings → Data).

import { describe, it, expect, vi, beforeEach } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { ExportImportSection } from "./ExportImportSection";
import { ipc } from "../../lib/ipc";
import { save } from "@tauri-apps/plugin-dialog";
import type { FullImportPreview, FullImportReport } from "../../lib/types";

vi.mock("../../lib/ipc", () => ({
  ipc: {
    listWorkspaces: vi.fn().mockResolvedValue([
      { id: "ws-1", name: "My Workspace", createdAt: 0, updatedAt: 0, isActive: true },
      { id: "ws-2", name: "Second", createdAt: 0, updatedAt: 0, isActive: false },
    ]),
    exportRestman: vi.fn().mockResolvedValue("{}"),
    writeFileBytes: vi.fn().mockResolvedValue(undefined),
    previewRestmanImport: vi.fn(),
    applyRestmanImport: vi.fn(),
  },
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  save: vi.fn().mockResolvedValue("/tmp/export.restman.json"),
}));

const PREVIEW: FullImportPreview = {
  version: 1,
  appVersion: "0.3.0",
  includesSecrets: false,
  workspaces: [
    { name: "My Workspace", exists: true, collections: 2, requests: 5, environments: 1, variables: 3 },
    { name: "Third", exists: false, collections: 1, requests: 1, environments: 0, variables: 0 },
  ],
  globalVariables: 1,
  maskedSecrets: 2,
  warnings: ["2 secret(s) in this file are masked and cannot be recovered — re-enter them after import"],
};

const REPORT: FullImportReport = {
  workspacesCreated: 1,
  createdCollections: 3,
  createdRequests: 6,
  skipped: 0,
  overwritten: 0,
  environmentsCreated: 1,
  variablesCreated: 4,
  variablesOverwritten: 0,
  variablesSkipped: 0,
  warnings: [],
};

function renderSection() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <ExportImportSection />
    </QueryClientProvider>,
  );
}

async function loadImportFile(json: string) {
  const input = document.querySelector('input[type="file"]') as HTMLInputElement;
  const file = new File([json], "export.restman.json", { type: "application/json" });
  // jsdom's File lacks text() in some versions — patch deterministically.
  Object.defineProperty(file, "text", { value: () => Promise.resolve(json) });
  fireEvent.change(input, { target: { files: [file] } });
}

describe("ExportImportSection", () => {
  beforeEach(() => {
    vi.mocked(ipc.exportRestman).mockClear();
    vi.mocked(ipc.writeFileBytes).mockClear();
    vi.mocked(ipc.previewRestmanImport).mockReset();
    vi.mocked(ipc.applyRestmanImport).mockReset();
    vi.mocked(save).mockClear();
  });

  it("export button stays disabled until a workspace is checked", async () => {
    renderSection();
    const button = await screen.findByText("Export…");
    expect(button.closest("button")).toBeDisabled();
    fireEvent.click(await screen.findByLabelText("Second"));
    expect(button.closest("button")).not.toBeDisabled();
  });

  it("exports checked workspace ids with the secrets flag and writes the file", async () => {
    renderSection();
    fireEvent.click(await screen.findByLabelText("Second"));
    fireEvent.click(screen.getByLabelText("Include secrets"));
    fireEvent.click(screen.getByText("Export…"));

    await waitFor(() => expect(ipc.writeFileBytes).toHaveBeenCalled());
    expect(ipc.exportRestman).toHaveBeenCalledWith(["ws-2"], true, true);
    expect(save).toHaveBeenCalled();
    expect(screen.getByText("Export saved.")).toBeTruthy();
  });

  it("shows a plaintext warning only when secrets are included", async () => {
    renderSection();
    expect(screen.queryByText(/plaintext/)).toBeNull();
    fireEvent.click(await screen.findByLabelText("Include secrets"));
    expect(screen.getByText(/plaintext/)).toBeTruthy();
  });

  it("import shows preview counts, collision badge, and warnings", async () => {
    vi.mocked(ipc.previewRestmanImport).mockResolvedValue(PREVIEW);
    renderSection();
    await loadImportFile('{"restmanExportVersion":1}');

    await screen.findByText("exists — will merge");
    expect(ipc.previewRestmanImport).toHaveBeenCalledWith('{"restmanExportVersion":1}');
    expect(screen.getByText(/2 collections · 5 requests/)).toBeTruthy();
    expect(screen.getByText(/cannot be recovered/)).toBeTruthy();
    expect(screen.getByText("1 global variable(s)")).toBeTruthy();
  });

  it("applies the import with the chosen conflict mode and reports counts", async () => {
    vi.mocked(ipc.previewRestmanImport).mockResolvedValue(PREVIEW);
    vi.mocked(ipc.applyRestmanImport).mockResolvedValue(REPORT);
    renderSection();
    await loadImportFile('{"restmanExportVersion":1}');
    await screen.findByText("exists — will merge");

    fireEvent.change(screen.getByDisplayValue("Skip existing"), { target: { value: "overwrite" } });
    fireEvent.click(screen.getByText("Import"));

    await waitFor(() =>
      expect(ipc.applyRestmanImport).toHaveBeenCalledWith('{"restmanExportVersion":1}', "overwrite"),
    );
    await screen.findByText(/Imported 1 new workspace\(s\), 3 collection\(s\)/);
    // Preview panel is gone after a successful apply.
    expect(screen.queryByText("exists — will merge")).toBeNull();
  });

  it("surfaces a preview error for an invalid file", async () => {
    vi.mocked(ipc.previewRestmanImport).mockRejectedValue("not a restman export");
    renderSection();
    await loadImportFile("{}");
    await screen.findByText(/Import failed: not a restman export/);
  });
});
