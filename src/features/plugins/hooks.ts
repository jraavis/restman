//! TanStack Query hooks for workspace-scoped JS plugins, backed by Tauri IPC.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ipc } from "../../lib/ipc";
import type { PluginInput, PluginKind } from "../../lib/types";

export const pluginKeys = {
  all: (workspaceId: string) => ["plugins", workspaceId] as const,
};

export function usePlugins(workspaceId: string | undefined, kind?: PluginKind) {
  return useQuery({
    queryKey: [...pluginKeys.all(workspaceId ?? ""), kind ?? "all"],
    queryFn: () => ipc.listPlugins(workspaceId as string, kind ?? null),
    enabled: Boolean(workspaceId),
  });
}

export function useCreatePlugin(workspaceId: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (input: PluginInput) => ipc.createPlugin(workspaceId, input),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: pluginKeys.all(workspaceId) });
    },
  });
}

export function useUpdatePlugin(workspaceId: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ id, input }: { id: string; input: PluginInput }) => ipc.updatePlugin(id, input),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: pluginKeys.all(workspaceId) });
    },
  });
}

export function useDeletePlugin(workspaceId: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => ipc.deletePlugin(id),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: pluginKeys.all(workspaceId) });
    },
  });
}

export function useExportPlugin() {
  return useMutation({ mutationFn: (id: string) => ipc.exportPlugin(id) });
}

export function useImportPlugin(workspaceId: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (content: string) => ipc.importPlugin(workspaceId, content),
    onSuccess: () => qc.invalidateQueries({ queryKey: pluginKeys.all(workspaceId) }),
  });
}
