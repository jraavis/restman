//! Keeps the DB-backed tabs list in sync with the live editing draft in
//! `requestStore`: seeds a first tab when a workspace has none (including
//! right after "close all", not just on first load), and loads a newly
//! active tab's content into the draft once it — and its linked saved
//! request, if any — has resolved.

import { useEffect, useRef } from "react";
import { defaultRequest } from "../../lib/http";
import type { Tab } from "../../lib/types";
import { useRequestStore } from "../../stores/requestStore";
import { useRequest } from "../collections/hooks";
import { useCreateTab, useTabs } from "./hooks";

export function useTabSync(workspaceId: string | undefined) {
  const { data: tabs, isLoading } = useTabs(workspaceId);
  const createTab = useCreateTab(workspaceId);

  // Tracks the workspace a bootstrap create is in flight (or just landed) for,
  // so the empty-tabs check below fires once per empty spell rather than once
  // ever — it's cleared again as soon as that workspace has a tab.
  const bootstrappingFor = useRef<string | null>(null);

  useEffect(() => {
    if (!workspaceId || isLoading || !tabs) return;
    if (tabs.length > 0) {
      if (bootstrappingFor.current === workspaceId) bootstrappingFor.current = null;
      return;
    }
    if (bootstrappingFor.current === workspaceId || createTab.isPending) return;
    bootstrappingFor.current = workspaceId;
    createTab.mutate({ requestId: null, title: "Untitled", draft: defaultRequest() });
  }, [workspaceId, isLoading, tabs, createTab]);

  const activeTab: Tab | null = tabs?.find((t) => t.isActive) ?? null;
  const { data: linkedRequest } = useRequest(activeTab?.requestId ?? undefined);

  const storeActiveTabId = useRequestStore((s) => s.activeTabId);
  const loadTab = useRequestStore((s) => s.loadTab);

  // Load the active tab into the draft whenever a *different* tab becomes
  // active — comparing ids, not object identity, since every debounced draft
  // flush invalidates and refetches `tabs`, producing a new (same-id) object
  // on every keystroke. Waits for `linkedRequest` to resolve (it starts
  // `undefined`, e.g. right after a cold restart) so a tab with a saved-request
  // link doesn't briefly flash an empty draft while that request is still loading.
  useEffect(() => {
    if (!activeTab || activeTab.id === storeActiveTabId) return;
    const ready = !activeTab.requestId || !!linkedRequest;
    if (!ready) return;
    loadTab({
      tabId: activeTab.id,
      requestId: activeTab.requestId,
      collectionId: linkedRequest?.collectionId ?? null,
      title: activeTab.title,
      draft: activeTab.draft,
    });
  }, [activeTab, storeActiveTabId, linkedRequest, loadTab]);

  return { tabs: tabs ?? [], activeTab, isLoadingTabs: isLoading };
}
