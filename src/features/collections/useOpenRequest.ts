//! Opens a saved request. HTTP requests open into a tab: switches to its tab
//! if one is already open, otherwise creates a new tab linked to it (see
//! `useTabSync`, which picks up the newly-active tab and loads its
//! now-linked draft). SSE/WS/gRPC requests aren't tab-backed — they open the
//! matching standalone panel via `streamingPanelStore`, prefilled from the
//! saved request's `streamConfig`.

import { useTabs, useCreateTab, useSetActiveTab } from "../tabs/hooks";
import { useStreamingPanelStore } from "../../stores/streamingPanelStore";
import type { SavedRequest } from "../../lib/types";

export function useOpenRequest(workspaceId: string | undefined) {
  const { data: tabs } = useTabs(workspaceId);
  const createTab = useCreateTab(workspaceId);
  const setActiveTab = useSetActiveTab(workspaceId);
  const openStreamingPanel = useStreamingPanelStore((s) => s.openStreamingPanel);

  function open(request: SavedRequest) {
    if (request.kind !== "http") {
      openStreamingPanel(request.kind, request);
      return;
    }
    const existing = tabs?.find((t) => t.requestId === request.id);
    if (existing) {
      setActiveTab.mutate(existing.id);
      return;
    }
    createTab.mutate({
      requestId: request.id,
      title: request.name,
      draft: {
        method: request.method,
        url: request.url,
        headers: request.headers,
        query: request.query,
        body: request.body,
        options: request.options,
      },
    });
  }

  return { open };
}
