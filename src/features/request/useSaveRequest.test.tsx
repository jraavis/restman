import type { ReactNode } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { defaultRequest } from "../../lib/http";
import { ipc } from "../../lib/ipc";
import { defaultRequestAuth, type SavedRequest } from "../../lib/types";
import { useRequestStore } from "../../stores/requestStore";
import { useSaveRequest } from "./useSaveRequest";

vi.mock("../../lib/ipc", () => ({
  ipc: {
    createRequest: vi.fn(),
    updateRequest: vi.fn(),
    setTabRequestId: vi.fn(),
  },
}));

function makeSavedRequest(overrides: Partial<SavedRequest> = {}): SavedRequest {
  return {
    id: "req-1",
    collectionId: "col-1",
    name: "Saved",
    method: "GET",
    url: "https://a.test",
    headers: [],
    query: [],
    body: { mode: "none" },
    options: defaultRequest().options,
    auth: defaultRequestAuth(),
    tags: [],
    sortOrder: 0,
    createdAt: 0,
    updatedAt: 0,
    lastUsedAt: null,
    ...overrides,
  };
}

function renderWithClient(workspaceId: string | undefined) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={qc}>{children}</QueryClientProvider>
  );
  return renderHook(() => useSaveRequest(workspaceId), { wrapper });
}

describe("useSaveRequest", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useRequestStore.setState({
      activeTabId: "tab-a",
      requestId: null,
      collectionId: null,
      title: "Old title",
      request: { ...defaultRequest(), url: "https://edited.test" },
      auth: defaultRequestAuth(),
      response: null,
      sending: false,
      error: null,
    });
  });

  it("creates the request and links the active tab on the first save", async () => {
    vi.mocked(ipc.createRequest).mockResolvedValue(makeSavedRequest({ id: "req-1", collectionId: "col-1" }));
    vi.mocked(ipc.setTabRequestId).mockResolvedValue({
      id: "tab-a",
      workspaceId: "ws-1",
      requestId: "req-1",
      title: "Old title",
      draft: defaultRequest(),
      sortOrder: 0,
      isActive: true,
      createdAt: 0,
      updatedAt: 0,
    });

    const { result } = renderWithClient("ws-1");
    await result.current.save("col-1");

    expect(ipc.createRequest).toHaveBeenCalledWith(
      "col-1",
      expect.objectContaining({ name: "Old title", url: "https://edited.test" }),
    );
    expect(ipc.setTabRequestId).toHaveBeenCalledWith("tab-a", "req-1");
    expect(ipc.updateRequest).not.toHaveBeenCalled();

    await waitFor(() => {
      expect(useRequestStore.getState().requestId).toBe("req-1");
    });
    expect(useRequestStore.getState().collectionId).toBe("col-1");
  });

  it("uses an explicit name override instead of the (possibly stale) store title", async () => {
    vi.mocked(ipc.createRequest).mockResolvedValue(makeSavedRequest());
    vi.mocked(ipc.setTabRequestId).mockResolvedValue({
      id: "tab-a",
      workspaceId: "ws-1",
      requestId: "req-1",
      title: "New title",
      draft: defaultRequest(),
      sortOrder: 0,
      isActive: true,
      createdAt: 0,
      updatedAt: 0,
    });

    const { result } = renderWithClient("ws-1");
    await result.current.save("col-1", "New title");

    expect(ipc.createRequest).toHaveBeenCalledWith("col-1", expect.objectContaining({ name: "New title" }));
  });

  it("rejects a first save with no collectionId", async () => {
    const { result } = renderWithClient("ws-1");
    await expect(result.current.save()).rejects.toThrow(/collectionId/);
    expect(ipc.createRequest).not.toHaveBeenCalled();
  });

  it("updates the existing request in place — no re-create, no re-link — once already linked", async () => {
    useRequestStore.setState({ requestId: "req-1", collectionId: "col-1" });
    vi.mocked(ipc.updateRequest).mockResolvedValue(makeSavedRequest());

    const { result } = renderWithClient("ws-1");
    await result.current.save();

    expect(ipc.updateRequest).toHaveBeenCalledWith(
      "req-1",
      expect.objectContaining({ name: "Old title", url: "https://edited.test" }),
    );
    expect(ipc.createRequest).not.toHaveBeenCalled();
    expect(ipc.setTabRequestId).not.toHaveBeenCalled();
  });
});
