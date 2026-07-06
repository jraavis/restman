//! TanStack Query hooks for mock servers, backed by Tauri IPC.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ipc } from "../../lib/ipc";
import type { MockRuleInput, MockServerInput } from "../../lib/types";

export const mockServerKeys = {
  all: (workspaceId: string) => ["mockServers", workspaceId] as const,
  running: ["mockServers", "running"] as const,
};

export const mockRuleKeys = {
  all: (mockServerId: string) => ["mockRules", mockServerId] as const,
};

export function useMockServers(workspaceId: string | undefined) {
  return useQuery({
    queryKey: mockServerKeys.all(workspaceId ?? ""),
    queryFn: () => ipc.listMockServers(workspaceId as string),
    enabled: Boolean(workspaceId),
  });
}

/** Which mock server ids are currently serving — cross-referenced by the
 * caller against `useMockServers`' result. Running state isn't part of the
 * server's own config row (see `MockServer` in `lib/types.ts`). */
export function useRunningMockServerIds() {
  return useQuery({
    queryKey: mockServerKeys.running,
    queryFn: () => ipc.listRunningMockServerIds(),
  });
}

export function useCreateMockServer(workspaceId: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (input: MockServerInput) => ipc.createMockServer(workspaceId, input),
    onSuccess: () => qc.invalidateQueries({ queryKey: mockServerKeys.all(workspaceId) }),
  });
}

export function useCreateMockServerFromCollection(workspaceId: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ collectionId, name, port }: { collectionId: string; name: string; port: number }) =>
      ipc.createMockServerFromCollection(workspaceId, collectionId, name, port),
    onSuccess: () => qc.invalidateQueries({ queryKey: mockServerKeys.all(workspaceId) }),
  });
}

export function useUpdateMockServer(workspaceId: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ id, input }: { id: string; input: MockServerInput }) => ipc.updateMockServer(id, input),
    onSuccess: () => qc.invalidateQueries({ queryKey: mockServerKeys.all(workspaceId) }),
  });
}

export function useDeleteMockServer(workspaceId: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => ipc.deleteMockServer(id),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: mockServerKeys.all(workspaceId) });
      qc.invalidateQueries({ queryKey: mockServerKeys.running });
    },
  });
}

export function useStartMockServer() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => ipc.startMockServer(id),
    onSuccess: () => qc.invalidateQueries({ queryKey: mockServerKeys.running }),
  });
}

export function useStopMockServer() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => ipc.stopMockServer(id),
    onSuccess: () => qc.invalidateQueries({ queryKey: mockServerKeys.running }),
  });
}

export function useExportMockServer() {
  return useMutation({ mutationFn: (id: string) => ipc.exportMockServer(id) });
}

export function useImportMockServer(workspaceId: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (content: string) => ipc.importMockServer(workspaceId, content),
    onSuccess: () => qc.invalidateQueries({ queryKey: mockServerKeys.all(workspaceId) }),
  });
}

export function useMockRules(mockServerId: string | undefined) {
  return useQuery({
    queryKey: mockRuleKeys.all(mockServerId ?? ""),
    queryFn: () => ipc.listMockRules(mockServerId as string),
    enabled: Boolean(mockServerId),
  });
}

export function useCreateMockRule(mockServerId: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (input: MockRuleInput) => ipc.createMockRule(mockServerId, input),
    onSuccess: () => qc.invalidateQueries({ queryKey: mockRuleKeys.all(mockServerId) }),
  });
}

export function useUpdateMockRule(mockServerId: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ id, input }: { id: string; input: MockRuleInput }) => ipc.updateMockRule(id, input),
    onSuccess: () => qc.invalidateQueries({ queryKey: mockRuleKeys.all(mockServerId) }),
  });
}

export function useDeleteMockRule(mockServerId: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => ipc.deleteMockRule(id),
    onSuccess: () => qc.invalidateQueries({ queryKey: mockRuleKeys.all(mockServerId) }),
  });
}
