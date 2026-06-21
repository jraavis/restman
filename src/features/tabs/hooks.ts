//! TanStack Query hooks for the tabs table — the source of truth for which
//! tabs exist and their persisted draft. The active tab's live edits stay in
//! `requestStore` and get debounce-flushed here; see `TabsBar`.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ipc } from "../../lib/ipc";
import type { HttpRequest } from "../../lib/http";

export const tabKeys = {
  all: (workspaceId: string) => ["tabs", workspaceId] as const,
};

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
    mutationFn: ({ requestId, title, draft }: { requestId: string | null; title: string; draft: HttpRequest }) =>
      ipc.createTab(workspaceId as string, requestId, title, draft),
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
    mutationFn: (id: string) => ipc.setActiveTab(workspaceId as string, id),
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
