//! Tests for the tab mutation hooks' flush-before-switch behavior: any
//! path that changes the active tab must persist the live draft first, or
//! the newest edits (classically a just-typed body) are silently dropped
//! when `useTabSync` reloads the incoming tab over the store.

import type { ReactNode } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { defaultRequest } from "../../lib/http";
import { ipc } from "../../lib/ipc";
import { useRequestStore } from "../../stores/requestStore";
import { useCreateTab, useSetActiveTab } from "./hooks";

vi.mock("../../lib/ipc", () => ({
  ipc: {
    updateTabDraft: vi.fn().mockResolvedValue(undefined),
    setActiveTab: vi.fn().mockResolvedValue(undefined),
    createTab: vi.fn().mockResolvedValue({ id: "tab-new" }),
  },
}));

function renderWithClient<T>(hook: () => T) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={qc}>{children}</QueryClientProvider>
  );
  return renderHook(hook, { wrapper });
}

const draftWithBody = {
  ...defaultRequest(),
  body: { mode: "json" as const, data: '{"typed":"but not yet debounced"}' },
};

describe("tab switch draft flushing", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useRequestStore.setState({ activeTabId: "tab-a", title: "Editing", request: draftWithBody });
  });

  it("setActiveTab flushes the live draft before switching", async () => {
    const { result } = renderWithClient(() => useSetActiveTab("ws-1"));
    await act(() => result.current.mutateAsync("tab-b"));

    expect(ipc.updateTabDraft).toHaveBeenCalledWith("tab-a", "Editing", draftWithBody);
    const flushOrder = vi.mocked(ipc.updateTabDraft).mock.invocationCallOrder[0];
    const switchOrder = vi.mocked(ipc.setActiveTab).mock.invocationCallOrder[0];
    expect(flushOrder).toBeLessThan(switchOrder);
  });

  it("createTab flushes the outgoing draft before creating (creation activates the new tab)", async () => {
    const { result } = renderWithClient(() => useCreateTab("ws-1"));
    await act(() => result.current.mutateAsync({ requestId: null, title: "New", draft: defaultRequest() }));

    expect(ipc.updateTabDraft).toHaveBeenCalledWith("tab-a", "Editing", draftWithBody);
    const flushOrder = vi.mocked(ipc.updateTabDraft).mock.invocationCallOrder[0];
    const createOrder = vi.mocked(ipc.createTab).mock.invocationCallOrder[0];
    expect(flushOrder).toBeLessThan(createOrder);
  });

  it("skips the flush when no tab is loaded yet", async () => {
    useRequestStore.setState({ activeTabId: null });
    const { result } = renderWithClient(() => useSetActiveTab("ws-1"));
    await act(() => result.current.mutateAsync("tab-b"));
    expect(ipc.updateTabDraft).not.toHaveBeenCalled();
  });

  it("a failed flush does not block the switch", async () => {
    vi.mocked(ipc.updateTabDraft).mockRejectedValueOnce("tab gone");
    const { result } = renderWithClient(() => useSetActiveTab("ws-1"));
    await act(() => result.current.mutateAsync("tab-b"));
    expect(ipc.setActiveTab).toHaveBeenCalledWith("ws-1", "tab-b");
  });
});
