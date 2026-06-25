//! Covers the export flow's migration off the Blob+anchor download pattern
//! onto save() + ipc.writeFileBytes — see ResponseViewer's "save to file"
//! for the sibling pattern this mirrors.

import type { ReactNode } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, expect, it, vi, beforeEach } from "vitest";
import { save } from "@tauri-apps/plugin-dialog";
import { textToBase64 } from "../../lib/encoding";
import { ipc } from "../../lib/ipc";
import { CollectionNode } from "./CollectionNode";
import type { Collection } from "../../lib/types";

vi.mock("@tauri-apps/plugin-dialog", () => ({
  save: vi.fn(),
}));

vi.mock("../../lib/ipc", () => ({
  ipc: {
    exportCollection: vi.fn(),
    writeFileBytes: vi.fn(),
  },
}));

function makeCollection(overrides: Partial<Collection> = {}): Collection {
  return {
    id: "col-1",
    workspaceId: "ws-1",
    parentId: null,
    name: "My Collection",
    description: null,
    auth: { type: "none" },
    sortOrder: 0,
    createdAt: 0,
    updatedAt: 0,
    ...overrides,
  };
}

function renderNode(collection: Collection) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={qc}>{children}</QueryClientProvider>
  );
  return render(
    <CollectionNode
      collection={collection}
      collections={[collection]}
      depth={0}
      workspaceId={undefined}
      expandedIds={new Set()}
      onToggleExpand={() => {}}
      dragRef={{ current: null }}
      sortMode="manual"
    />,
    { wrapper },
  );
}

describe("CollectionNode export", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("exports via save() + writeFileBytes, base64-encoding the export content", async () => {
    const collection = makeCollection();
    vi.mocked(ipc.exportCollection).mockResolvedValue('{"info":{}}');
    vi.mocked(save).mockResolvedValue("/tmp/My_Collection.postman_collection.json");

    renderNode(collection);
    fireEvent.click(screen.getByTitle("Collection actions"));
    fireEvent.click(screen.getByRole("button", { name: /export to postman/i }));

    await waitFor(() => expect(ipc.exportCollection).toHaveBeenCalledWith("col-1", "postman"));
    expect(save).toHaveBeenCalledWith({ defaultPath: "My_Collection.postman_collection.json" });
    await waitFor(() =>
      expect(ipc.writeFileBytes).toHaveBeenCalledWith(
        "/tmp/My_Collection.postman_collection.json",
        textToBase64('{"info":{}}'),
      ),
    );
  });

  it("skips writeFileBytes when the save dialog is cancelled", async () => {
    const collection = makeCollection();
    vi.mocked(ipc.exportCollection).mockResolvedValue('{"info":{}}');
    vi.mocked(save).mockResolvedValue(null);

    renderNode(collection);
    fireEvent.click(screen.getByTitle("Collection actions"));
    fireEvent.click(screen.getByRole("button", { name: /export to postman/i }));

    await waitFor(() => expect(save).toHaveBeenCalled());
    expect(ipc.writeFileBytes).not.toHaveBeenCalled();
  });
});
