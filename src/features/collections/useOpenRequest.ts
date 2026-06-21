//! Opens a saved request into a tab: switches to its tab if one is already
//! open, otherwise creates a new tab linked to it. `useTabSync` does the rest
//! — picking up the newly-active tab and loading its (now-linked) draft.

import { useTabs, useCreateTab, useSetActiveTab } from "../tabs/hooks";
import type { SavedRequest } from "../../lib/types";

export function useOpenRequest(workspaceId: string | undefined) {
  const { data: tabs } = useTabs(workspaceId);
  const createTab = useCreateTab(workspaceId);
  const setActiveTab = useSetActiveTab(workspaceId);

  function open(request: SavedRequest) {
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
