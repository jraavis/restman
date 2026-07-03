//! TanStack Query hooks for the tabs table — the source of truth for which
//! tabs exist and their persisted draft. The active tab's live edits stay in
//! `requestStore` and get debounce-flushed here; see `TabsBar`.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ipc } from "../../lib/ipc";
import { useRequestStore } from "../../stores/requestStore";
import type { HttpRequest } from "../../lib/http";

export const tabKeys = {
  all: (workspaceId: string) => ["tabs", workspaceId] as const,
};

/** Persist the live draft to its tab row right now, bypassing the 500ms
 * debounce in `TabsBar`. Every path that switches away from the active tab
 * (tab strip, sidebar request click, history open, new tab) must flush
 * first — otherwise `useTabSync` reloads the incoming tab over the store
 * and the newest edits (classically: a just-typed body) are silently lost. */
async function flushLiveDraft(): Promise<void> {
  const { activeTabId, title, request } = useRequestStore.getState();
  if (!activeTabId) return;
  try {
    await ipc.updateTabDraft(activeTabId, title, request);
  } catch {
    // Tab row may already be gone (e.g. switching because it was closed) —
    // losing this flush is then correct, not an error worth surfacing.
  }
}

export function useTabs(workspaceId: string | undefined) {
  return useQuery({
    queryKey: tabKeys.all(workspaceId ?? ""),
    queryFn: () => ipc.listTabs(workspaceId as string),
    enabled: !!workspaceId,
  });
}

export function useCreateTab(workspaceId: string | undefined) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: async ({ requestId, title, draft }: { requestId: string | null; title: string; draft: HttpRequest }) => {
      // Creating a tab also activates it — same switch-away hazard as
      // `useSetActiveTab`, so the outgoing draft flushes first.
      await flushLiveDraft();
      return ipc.createTab(workspaceId as string, requestId, title, draft);
    },
    onSuccess: () => {
      if (workspaceId) qc.invalidateQueries({ queryKey: tabKeys.all(workspaceId) });
    },
  });
}

export function useUpdateTabDraft(workspaceId: string | undefined) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ id, title, draft }: { id: string; title: string; draft: HttpRequest }) =>
      ipc.updateTabDraft(id, title, draft),
    onSuccess: () => {
      if (workspaceId) qc.invalidateQueries({ queryKey: tabKeys.all(workspaceId) });
    },
  });
}

export function useSetTabRequestId(workspaceId: string | undefined) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ id, requestId }: { id: string; requestId: string }) => ipc.setTabRequestId(id, requestId),
    onSuccess: () => {
      if (workspaceId) qc.invalidateQueries({ queryKey: tabKeys.all(workspaceId) });
    },
  });
}

export function useSetActiveTab(workspaceId: string | undefined) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: async (id: string) => {
      await flushLiveDraft();
      return ipc.setActiveTab(workspaceId as string, id);
    },
    onSuccess: () => {
      if (workspaceId) qc.invalidateQueries({ queryKey: tabKeys.all(workspaceId) });
    },
  });
}

export function useReorderTabs(workspaceId: string | undefined) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (ids: string[]) => ipc.reorderTabs(ids),
    onSuccess: () => {
      if (workspaceId) qc.invalidateQueries({ queryKey: tabKeys.all(workspaceId) });
    },
  });
}

export function useCloseTab(workspaceId: string | undefined) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => ipc.closeTab(workspaceId as string, id),
    onSuccess: () => {
      if (workspaceId) qc.invalidateQueries({ queryKey: tabKeys.all(workspaceId) });
    },
  });
}

export function useCloseOtherTabs(workspaceId: string | undefined) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (keepId: string) => ipc.closeOtherTabs(workspaceId as string, keepId),
    onSuccess: () => {
      if (workspaceId) qc.invalidateQueries({ queryKey: tabKeys.all(workspaceId) });
    },
  });
}

export function useCloseAllTabs(workspaceId: string | undefined) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: () => ipc.closeAllTabs(workspaceId as string),
    onSuccess: () => {
      if (workspaceId) qc.invalidateQueries({ queryKey: tabKeys.all(workspaceId) });
    },
  });
}
