//! TanStack Query hooks for history, backed by Tauri IPC.

import { useCallback } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ipc } from "../../lib/ipc";
import type { HistoryEntry, HistoryFilter } from "../../lib/types";
import { useRequestStore } from "../../stores/requestStore";

export const historyKeys = {
  list: (workspaceId: string, filter: HistoryFilter) => ["history", workspaceId, filter] as const,
  all: (workspaceId: string) => ["history", workspaceId] as const,
  retention: ["history", "retention"] as const,
};

export function useHistory(workspaceId: string | undefined, filter: HistoryFilter) {
  return useQuery({
    queryKey: historyKeys.list(workspaceId ?? "", filter),
    queryFn: () => ipc.listHistory(workspaceId as string, filter),
    enabled: !!workspaceId,
  });
}

/** Imperative invalidation for call sites that aren't themselves hooks-rendered alongside the list, e.g. `useSend`. */
export function useHistoryInvalidation() {
  const qc = useQueryClient();
  return useCallback(
    (workspaceId: string) => {
      qc.invalidateQueries({ queryKey: historyKeys.all(workspaceId) });
    },
    [qc],
  );
}

export function useDeleteHistoryEntry(workspaceId: string | undefined) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => ipc.deleteHistoryEntry(id),
    onSuccess: () => {
      if (workspaceId) qc.invalidateQueries({ queryKey: historyKeys.all(workspaceId) });
    },
  });
}

export function useClearHistory(workspaceId: string | undefined) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: () => ipc.clearHistory(workspaceId as string),
    onSuccess: () => {
      if (workspaceId) qc.invalidateQueries({ queryKey: historyKeys.all(workspaceId) });
    },
  });
}

export function useHistoryRetention() {
  return useQuery({
    queryKey: historyKeys.retention,
    queryFn: () => ipc.getHistoryRetention(),
  });
}

export function useSetHistoryRetention() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (count: number) => ipc.setHistoryRetention(count),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: historyKeys.retention });
      // Lowering retention deletes rows for every workspace immediately, not
      // just the active one — invalidate all history lists, not just one.
      qc.invalidateQueries({ queryKey: ["history"] });
    },
  });
}

export function useReplayHistoryEntry(workspaceId: string | undefined) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => ipc.replayHistoryEntry(id),
    onSuccess: () => {
      if (workspaceId) qc.invalidateQueries({ queryKey: historyKeys.all(workspaceId) });
    },
  });
}

/**
 * Replay a history entry into the live draft so the request builder reflects
 * what's being resent, then fire it and route the result into the same
 * response panel as a normal send — replaying shouldn't show a response that
 * doesn't match what's on screen.
 */
export function useReplayIntoDraft(workspaceId: string | undefined) {
  const replay = useReplayHistoryEntry(workspaceId);
  const loadDraft = useRequestStore((s) => s.loadDraft);
  const beginSend = useRequestStore((s) => s.beginSend);
  const setResponse = useRequestStore((s) => s.setResponse);
  const setError = useRequestStore((s) => s.setError);

  return useCallback(
    async (entry: HistoryEntry) => {
      loadDraft(entry.request, entry.name);
      beginSend();
      try {
        const response = await replay.mutateAsync(entry.id);
        setResponse(response);
      } catch (e) {
        setError(typeof e === "string" ? e : String(e));
      }
    },
    [replay, loadDraft, beginSend, setResponse, setError],
  );
}
