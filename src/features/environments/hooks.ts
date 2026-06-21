//! TanStack Query hooks for environments and scoped variables.

import { useMemo } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ipc } from "../../lib/ipc";
import type { VarScope, VariableInput } from "../../lib/types";

export const environmentKeys = {
  all: (workspaceId: string) => ["environments", workspaceId] as const,
  active: (workspaceId: string) => ["environments", workspaceId, "active"] as const,
};
export const variableKeys = {
  scope: (scope: VarScope) => ["variables", scope] as const,
};

export function scopeKey(scope: VarScope): string {
  return scope.kind === "global" ? "global" : `${scope.kind}:${scope.id}`;
}

// Environments

export function useEnvironments(workspaceId: string | undefined) {
  return useQuery({
    queryKey: environmentKeys.all(workspaceId ?? ""),
    queryFn: () => ipc.listEnvironments(workspaceId as string),
    enabled: !!workspaceId,
  });
}

export function useActiveEnvironment(workspaceId: string | undefined) {
  return useQuery({
    queryKey: environmentKeys.active(workspaceId ?? ""),
    queryFn: () => ipc.activeEnvironment(workspaceId as string),
    enabled: !!workspaceId,
  });
}

export function useCreateEnvironment(workspaceId: string | undefined) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({
      collectionId,
      name,
      groupName,
    }: {
      collectionId: string | null;
      name: string;
      groupName?: string | null;
    }) => ipc.createEnvironment(workspaceId as string, collectionId, name, groupName),
    onSuccess: () => {
      if (workspaceId) qc.invalidateQueries({ queryKey: environmentKeys.all(workspaceId) });
    },
  });
}

export function useUpdateEnvironment(workspaceId: string | undefined) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ id, name, groupName }: { id: string; name: string; groupName?: string | null }) =>
      ipc.updateEnvironment(id, name, groupName),
    onSuccess: () => {
      if (workspaceId) qc.invalidateQueries({ queryKey: environmentKeys.all(workspaceId) });
    },
  });
}

export function useDeleteEnvironment(workspaceId: string | undefined) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => ipc.deleteEnvironment(id),
    onSuccess: () => {
      if (workspaceId) {
        qc.invalidateQueries({ queryKey: environmentKeys.all(workspaceId) });
        qc.invalidateQueries({ queryKey: environmentKeys.active(workspaceId) });
      }
    },
  });
}

export function useSetActiveEnvironment(workspaceId: string | undefined) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string | null) => ipc.setActiveEnvironment(workspaceId as string, id),
    onSuccess: () => {
      if (workspaceId) {
        qc.invalidateQueries({ queryKey: environmentKeys.all(workspaceId) });
        qc.invalidateQueries({ queryKey: environmentKeys.active(workspaceId) });
      }
    },
  });
}

// Variables — note `list_variables`/`create_variable`/`update_variable` mask secret
// values at the IPC layer (see `commands::variables`); the masked sentinel is the
// only form this hook layer ever sees, by design.

export function useVariables(scope: VarScope, enabled: boolean = true) {
  return useQuery({
    queryKey: variableKeys.scope(scope),
    queryFn: () => ipc.listVariables(scope),
    enabled,
  });
}

/**
 * Union of enabled variable keys across every scope that can affect the
 * current draft — global, workspace, the draft's linked collection (if any),
 * and the active environment — for `{{var}}` autocomplete. Resolution
 * priority at send time is unaffected; this is just "what names exist".
 */
export function useResolvedVariableKeys(
  workspaceId: string | undefined,
  collectionId: string | null | undefined,
) {
  const { data: activeEnv } = useActiveEnvironment(workspaceId);
  const global = useVariables({ kind: "global" });
  const workspace = useVariables({ kind: "workspace", id: workspaceId ?? "" }, !!workspaceId);
  const collection = useVariables({ kind: "collection", id: collectionId ?? "" }, !!collectionId);
  const environment = useVariables({ kind: "environment", id: activeEnv?.id ?? "" }, !!activeEnv);

  return useMemo(() => {
    const keys = new Set<string>();
    for (const list of [global.data, workspace.data, collection.data, environment.data]) {
      for (const v of list ?? []) {
        if (v.enabled) keys.add(v.key);
      }
    }
    return [...keys].sort();
  }, [global.data, workspace.data, collection.data, environment.data]);
}

export function useCreateVariable(scope: VarScope) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (input: VariableInput) => ipc.createVariable(scope, input),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: variableKeys.scope(scope) });
    },
  });
}

export function useUpdateVariable(scope: VarScope) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ id, input }: { id: string; input: VariableInput }) => ipc.updateVariable(id, input),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: variableKeys.scope(scope) });
    },
  });
}

export function useDeleteVariable(scope: VarScope) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => ipc.deleteVariable(id),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: variableKeys.scope(scope) });
    },
  });
}
