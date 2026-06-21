import type { ReactNode } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { defaultRequest } from "../../lib/http";
import { ipc } from "../../lib/ipc";
import type { SavedRequest, Tab } from "../../lib/types";
import { useRequestStore } from "../../stores/requestStore";
import { useTabSync } from "./useTabSync";

vi.mock("../../lib/ipc", () => ({
  ipc: {
    listTabs: vi.fn(),
    createTab: vi.fn(),
    getRequest: vi.fn(),
  },
}));

function makeTab(overrides: Partial<Tab> = {}): Tab {
  return {
    id: "tab-a",
    workspaceId: "ws-1",
    requestId: null,
    title: "Untitled",
    draft: defaultRequest(),
    sortOrder: 0,
    isActive: true,
    createdAt: 0,
    updatedAt: 0,
    ...overrides,
  };
}

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
  return { ...renderHook(() => useTabSync(workspaceId), { wrapper }), qc };
}

describe("useTabSync", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useRequestStore.setState({
      activeTabId: null,
      requestId: null,
      collectionId: null,
      title: "Untitled",
      request: defaultRequest(),
      response: null,
      sending: false,
      error: null,
    });
  });

  it("loads the active tab's draft once it (and its linked request) resolves — the cold-restart path", async () => {
    const tabA = makeTab({
      requestId: "req-1",
      title: "A",
      draft: { ...defaultRequest(), url: "https://a.test" },
    });
    vi.mocked(ipc.listTabs).mockResolvedValue([tabA]);
    vi.mocked(ipc.getRequest).mockResolvedValue(makeSavedRequest({ collectionId: "col-1" }));

    renderWithClient("ws-1");

    await waitFor(() => {
      expect(useRequestStore.getState().activeTabId).toBe("tab-a");
    });
    expect(useRequestStore.getState().collectionId).toBe("col-1");
    expect(useRequestStore.getState().request.url).toBe("https://a.test");
  });

  it("does not reload — and so does not clobber an in-progress edit — when a refetch returns the same active tab id", async () => {
    vi.mocked(ipc.listTabs).mockImplementation(() => Promise.resolve([makeTab()]));

    const { qc } = renderWithClient("ws-1");

    await waitFor(() => {
      expect(useRequestStore.getState().activeTabId).toBe("tab-a");
    });

    useRequestStore.getState().setUrl("https://edited.test");

    const callsBefore = vi.mocked(ipc.listTabs).mock.calls.length;
    await act(async () => {
      await qc.invalidateQueries({ queryKey: ["tabs", "ws-1"] });
    });
    await waitFor(() => {
      expect(vi.mocked(ipc.listTabs).mock.calls.length).toBeGreaterThan(callsBefore);
    });

    expect(useRequestStore.getState().request.url).toBe("https://edited.test");
  });

  it("seeds a first tab when the workspace has none", async () => {
    vi.mocked(ipc.listTabs).mockResolvedValue([]);
    vi.mocked(ipc.createTab).mockResolvedValue(makeTab({ id: "tab-new" }));

    renderWithClient("ws-1");

    await waitFor(() => {
      expect(ipc.createTab).toHaveBeenCalledTimes(1);
    });
    expect(ipc.createTab).toHaveBeenCalledWith("ws-1", null, "Untitled", defaultRequest());
  });
});
