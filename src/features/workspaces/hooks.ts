//! TanStack Query hooks for workspaces, backed by Tauri IPC.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ipc } from "../../lib/ipc";

export const workspaceKeys = {
  all: ["workspaces"] as const,
  active: ["workspaces", "active"] as const,
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
