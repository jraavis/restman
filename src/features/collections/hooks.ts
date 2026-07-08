//! TanStack Query hooks for collections, saved requests, tags, and search.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ipc } from "../../lib/ipc";
import { triggerLiveSyncIfEnabled } from "../../lib/liveSync";
import type {
  AuthConfig,
  ConflictMode,
  ImportPlacement,
  ImportedNode,
  SavedRequestInput,
} from "../../lib/types";

export const collectionKeys = {
  all: (workspaceId: string) => ["collections", workspaceId] as const,
};
export const requestKeys = {
  list: (collectionId: string) => ["requests", collectionId] as const,
  one: (id: string) => ["requests", "one", id] as const,
};
export const tagKeys = {
  all: (workspaceId: string) => ["tags", workspaceId] as const,
};
export const searchKeys = {
  query: (workspaceId: string, query: string, method: string | null) =>
    ["search", workspaceId, query, method] as const,
};

// Collections

export function useCollections(workspaceId: string | undefined) {
  return useQuery({
    queryKey: collectionKeys.all(workspaceId ?? ""),
    queryFn: () => ipc.listCollections(workspaceId as string),
    enabled: !!workspaceId,
  });
}

export function useCreateCollection(workspaceId: string | undefined) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ parentId, name, description }: { parentId: string | null; name: string; description?: string | null }) =>
      ipc.createCollection(workspaceId as string, parentId, name, description),
    onSuccess: () => {
      if (workspaceId) qc.invalidateQueries({ queryKey: collectionKeys.all(workspaceId) });
      triggerLiveSyncIfEnabled(qc, workspaceId);
    },
  });
}

export function useUpdateCollection(workspaceId: string | undefined) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ id, name, description }: { id: string; name: string; description?: string | null }) =>
      ipc.updateCollection(id, name, description),
    onSuccess: () => {
      if (workspaceId) qc.invalidateQueries({ queryKey: collectionKeys.all(workspaceId) });
      triggerLiveSyncIfEnabled(qc, workspaceId);
    },
  });
}

export function useUpdateCollectionAuth(workspaceId: string | undefined) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ id, auth }: { id: string; auth: AuthConfig }) => ipc.updateCollectionAuth(id, auth),
    onSuccess: () => {
      if (workspaceId) qc.invalidateQueries({ queryKey: collectionKeys.all(workspaceId) });
      triggerLiveSyncIfEnabled(qc, workspaceId);
    },
  });
}

export function useDeleteCollection(workspaceId: string | undefined) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => ipc.deleteCollection(id),
    onSuccess: () => {
      if (workspaceId) qc.invalidateQueries({ queryKey: collectionKeys.all(workspaceId) });
      triggerLiveSyncIfEnabled(qc, workspaceId);
    },
  });
}

export function useMoveCollection(workspaceId: string | undefined) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ id, newParentId }: { id: string; newParentId: string | null }) =>
      ipc.moveCollection(id, newParentId),
    onSuccess: () => {
      if (workspaceId) qc.invalidateQueries({ queryKey: collectionKeys.all(workspaceId) });
      triggerLiveSyncIfEnabled(qc, workspaceId);
    },
  });
}

export function useReorderCollections(workspaceId: string | undefined) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (ids: string[]) => ipc.reorderCollections(ids),
    onSuccess: () => {
      if (workspaceId) qc.invalidateQueries({ queryKey: collectionKeys.all(workspaceId) });
      triggerLiveSyncIfEnabled(qc, workspaceId);
    },
  });
}

export function useDuplicateCollection(workspaceId: string | undefined) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ id, newName }: { id: string; newName?: string | null }) => ipc.duplicateCollection(id, newName),
    onSuccess: () => {
      if (workspaceId) qc.invalidateQueries({ queryKey: collectionKeys.all(workspaceId) });
      triggerLiveSyncIfEnabled(qc, workspaceId);
    },
  });
}

// Requests

export function useRequests(collectionId: string | undefined) {
  return useQuery({
    queryKey: requestKeys.list(collectionId ?? ""),
    queryFn: () => ipc.listRequests(collectionId as string),
    enabled: !!collectionId,
  });
}

export function useRequest(id: string | undefined) {
  return useQuery({
    queryKey: requestKeys.one(id ?? ""),
    queryFn: () => ipc.getRequest(id as string),
    enabled: !!id,
  });
}

export function useCreateRequest(workspaceId: string | undefined) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ collectionId, input }: { collectionId: string; input: SavedRequestInput }) =>
      ipc.createRequest(collectionId, input),
    onSuccess: (saved) => {
      qc.invalidateQueries({ queryKey: requestKeys.list(saved.collectionId) });
      if (workspaceId) qc.invalidateQueries({ queryKey: collectionKeys.all(workspaceId) });
      triggerLiveSyncIfEnabled(qc, workspaceId);
    },
  });
}

/** `workspaceId` is only used to key the live-sync trigger (see
 * `triggerLiveSyncIfEnabled`) — request-list cache invalidation is scoped
 * to the request's own `collectionId`, unaffected by whether it's passed. */
export function useUpdateRequest(workspaceId?: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ id, input }: { id: string; input: SavedRequestInput }) => ipc.updateRequest(id, input),
    onSuccess: (saved) => {
      qc.invalidateQueries({ queryKey: requestKeys.list(saved.collectionId) });
      qc.invalidateQueries({ queryKey: requestKeys.one(saved.id) });
      triggerLiveSyncIfEnabled(qc, workspaceId);
    },
  });
}

export function useDeleteRequest(collectionId: string | undefined) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => ipc.deleteRequest(id),
    onSuccess: () => {
      if (collectionId) qc.invalidateQueries({ queryKey: requestKeys.list(collectionId) });
      // No workspaceId in this hook's signature — liveSync resolves the
      // active workspace itself (same for move/reorder/duplicate below).
      triggerLiveSyncIfEnabled(qc);
    },
  });
}

export function useMoveRequest() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({
      id,
      collectionId,
    }: {
      id: string;
      collectionId: string;
      fromCollectionId?: string;
    }) => ipc.moveRequest(id, collectionId),
    onSuccess: (saved, vars) => {
      qc.invalidateQueries({ queryKey: requestKeys.list(saved.collectionId) });
      if (vars.fromCollectionId && vars.fromCollectionId !== saved.collectionId) {
        qc.invalidateQueries({ queryKey: requestKeys.list(vars.fromCollectionId) });
      }
      qc.invalidateQueries({ queryKey: requestKeys.one(vars.id) });
      triggerLiveSyncIfEnabled(qc);
    },
  });
}

export function useApplyCollectionImport(workspaceId: string | undefined) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({
      parentId,
      root,
      mode,
      placement,
    }: {
      parentId: string | null;
      root: ImportedNode;
      mode: ConflictMode;
      placement: ImportPlacement;
    }) => ipc.applyCollectionImport(workspaceId as string, parentId, root, mode, placement),
    onSuccess: (_report, vars) => {
      if (workspaceId) {
        qc.invalidateQueries({ queryKey: collectionKeys.all(workspaceId) });
        qc.invalidateQueries({ queryKey: ["search", workspaceId] });
      }
      if (vars.parentId) {
        qc.invalidateQueries({ queryKey: requestKeys.list(vars.parentId) });
      }
      qc.invalidateQueries({ queryKey: ["requests"] });
      triggerLiveSyncIfEnabled(qc, workspaceId);
    },
  });
}

export function useReorderRequests(collectionId: string | undefined) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (ids: string[]) => ipc.reorderRequests(ids),
    onSuccess: () => {
      if (collectionId) qc.invalidateQueries({ queryKey: requestKeys.list(collectionId) });
      triggerLiveSyncIfEnabled(qc);
    },
  });
}

export function useDuplicateRequest(collectionId: string | undefined) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ id, newName }: { id: string; newName?: string | null }) => ipc.duplicateRequest(id, newName),
    onSuccess: () => {
      if (collectionId) qc.invalidateQueries({ queryKey: requestKeys.list(collectionId) });
      triggerLiveSyncIfEnabled(qc);
    },
  });
}

export function useSetRequestTags(collectionId: string | undefined) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ requestId, tagIds }: { requestId: string; tagIds: string[] }) =>
      ipc.setRequestTags(requestId, tagIds),
    onSuccess: (_data, vars) => {
      if (collectionId) qc.invalidateQueries({ queryKey: requestKeys.list(collectionId) });
      qc.invalidateQueries({ queryKey: requestKeys.one(vars.requestId) });
    },
  });
}

// Tags

export function useTags(workspaceId: string | undefined) {
  return useQuery({
    queryKey: tagKeys.all(workspaceId ?? ""),
    queryFn: () => ipc.listTags(workspaceId as string),
    enabled: !!workspaceId,
  });
}

export function useCreateTag(workspaceId: string | undefined) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ name, color }: { name: string; color: string }) =>
      ipc.createTag(workspaceId as string, name, color),
    onSuccess: () => {
      if (workspaceId) qc.invalidateQueries({ queryKey: tagKeys.all(workspaceId) });
    },
  });
}

export function useUpdateTag(workspaceId: string | undefined) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ id, name, color }: { id: string; name: string; color: string }) => ipc.updateTag(id, name, color),
    onSuccess: () => {
      if (workspaceId) qc.invalidateQueries({ queryKey: tagKeys.all(workspaceId) });
    },
  });
}

export function useDeleteTag(workspaceId: string | undefined) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => ipc.deleteTag(id),
    onSuccess: () => {
      if (workspaceId) qc.invalidateQueries({ queryKey: tagKeys.all(workspaceId) });
    },
  });
}

// Search

export function useSearchRequests(workspaceId: string | undefined, query: string, method: string | null = null) {
  return useQuery({
    queryKey: searchKeys.query(workspaceId ?? "", query, method),
    queryFn: () => ipc.searchRequests(workspaceId as string, query, method),
    // Blank query + no method is still a valid call (e.g. tag-only
    // filtering) — the caller (SearchResults) only mounts once there's a
    // query/method/tag reason to search, so no extra gate is needed here.
    enabled: !!workspaceId,
  });
}
