//! TanStack Query hooks for workspaces, backed by Tauri IPC.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ipc } from "../../lib/ipc";
import type { WorkspaceSettings } from "../../lib/types";

export const workspaceKeys = {
  all: ["workspaces"] as const,
  active: ["workspaces", "active"] as const,
  settings: (workspaceId: string) => ["workspaces", "settings", workspaceId] as const,
};

export function useWorkspaces() {
  return useQuery({ queryKey: workspaceKeys.all, queryFn: ipc.listWorkspaces });
}

export function useActiveWorkspace() {
  return useQuery({ queryKey: workspaceKeys.active, queryFn: ipc.activeWorkspace });
}

export function useCreateWorkspace() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (name: string) => ipc.createWorkspace(name),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: workspaceKeys.all });
    },
  });
}

export function useSetActiveWorkspace() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => ipc.setActiveWorkspace(id),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: workspaceKeys.all });
      qc.invalidateQueries({ queryKey: workspaceKeys.active });
    },
  });
}

export function useUpdateWorkspace() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ id, name }: { id: string; name: string }) => ipc.updateWorkspace(id, name),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: workspaceKeys.all });
      qc.invalidateQueries({ queryKey: workspaceKeys.active });
    },
  });
}

export function useDeleteWorkspace() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => ipc.deleteWorkspace(id),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: workspaceKeys.all });
      qc.invalidateQueries({ queryKey: workspaceKeys.active });
    },
  });
}

export function useWorkspaceSettings(workspaceId: string | undefined) {
  return useQuery({
    queryKey: workspaceKeys.settings(workspaceId ?? ""),
    queryFn: () => ipc.getWorkspaceSettings(workspaceId as string),
    enabled: Boolean(workspaceId),
  });
}

export function useSetWorkspaceSettings(workspaceId: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (settings: WorkspaceSettings) => ipc.setWorkspaceSettings(settings),
    onSuccess: (saved) => {
      qc.setQueryData(workspaceKeys.settings(workspaceId), saved);
    },
  });
}
