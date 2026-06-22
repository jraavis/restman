//! Requests inside one collection. Only ever mounted while that collection
//! is expanded — that's what makes the `useRequests` fetch lazy (fires on
//! first expand, cached after) without an eager per-collection fan-out and
//! without breaking the rules of hooks in `CollectionNode`'s `.map()`.

import type { DragEvent } from "react";
import { Loader2 } from "lucide-react";
import {
  useDeleteRequest,
  useDuplicateRequest,
  useMoveRequest,
  useReorderRequests,
  useRequests,
  useUpdateRequest,
} from "./hooks";
import { useOpenRequest } from "./useOpenRequest";
import { useRequestStore } from "../../stores/requestStore";
import type { DragRef } from "./dragState";
import type { SavedRequest } from "../../lib/types";
import { sortRequests, type SortMode } from "./tree";
import { RequestRow } from "./RequestRow";

export function RequestList({
  collectionId,
  workspaceId,
  dragRef,
  sortMode,
}: {
  collectionId: string;
  workspaceId: string | undefined;
  dragRef: DragRef;
  sortMode: SortMode;
}) {
  const { data: requests, isLoading } = useRequests(collectionId);
  const sortedRequests = requests ? sortRequests(requests, sortMode) : requests;
  const { open } = useOpenRequest(workspaceId);
  const updateRequest = useUpdateRequest();
  const duplicateRequest = useDuplicateRequest(collectionId);
  const deleteRequest = useDeleteRequest(collectionId);
  const moveRequest = useMoveRequest();
  const reorderRequests = useReorderRequests(collectionId);
  const activeRequestId = useRequestStore((s) => s.requestId);

  // Dropped on the list itself (not on a specific row, which stops
  // propagation first) — append into this collection from elsewhere.
  function handleDropOnList() {
    const drag = dragRef.current;
    dragRef.current = null;
    if (!drag || drag.kind !== "request" || drag.collectionId === collectionId) return;
    moveRequest.mutate({ id: drag.id, collectionId });
  }

  // Dropped directly on `target` — reorder if it's already in this
  // collection, otherwise move in and land at that position.
  function handleDropOnRequest(target: SavedRequest) {
    const drag = dragRef.current;
    dragRef.current = null;
    if (!drag || drag.kind !== "request" || drag.id === target.id || !requests) return;
    if (drag.collectionId !== collectionId) {
      moveRequest.mutate({ id: drag.id, collectionId });
      return;
    }
    const ids = requests.map((r) => r.id);
    const fromIndex = ids.indexOf(drag.id);
    const toIndex = ids.indexOf(target.id);
    if (fromIndex === -1 || toIndex === -1) return;
    ids.splice(fromIndex, 1);
    ids.splice(toIndex, 0, drag.id);
    reorderRequests.mutate(ids);
  }

  function handleDragStart(e: DragEvent, request: SavedRequest) {
    e.stopPropagation();
    dragRef.current = { kind: "request", id: request.id, collectionId };
  }

  if (isLoading) {
    return (
      <div className="flex items-center gap-1.5 py-1 pl-6 text-xs text-slate-400">
        <Loader2 size={11} className="animate-spin" /> Loading…
      </div>
    );
  }

  return (
    <div onDragOver={(e) => e.preventDefault()} onDrop={handleDropOnList}>
      {(!sortedRequests || sortedRequests.length === 0) && (
        <p className="py-1 pl-6 text-xs text-slate-400">No requests yet.</p>
      )}
      {sortedRequests?.map((request) => (
        <div
          key={request.id}
          className="pl-6"
          draggable={sortMode === "manual"}
          onDragStart={(e) => handleDragStart(e, request)}
          onDragOver={(e) => e.preventDefault()}
          onDrop={(e) => {
            e.stopPropagation();
            handleDropOnRequest(request);
          }}
        >
          <RequestRow
            request={request}
            workspaceId={workspaceId}
            collectionId={collectionId}
            isOpen={request.id === activeRequestId}
            onOpen={() => open(request)}
            onRename={(name) =>
              updateRequest.mutate({
                id: request.id,
                input: {
                  name,
                  method: request.method,
                  url: request.url,
                  headers: request.headers,
                  query: request.query,
                  body: request.body,
                  options: request.options,
                  auth: request.auth,
                },
              })
            }
            onDuplicate={() => duplicateRequest.mutate({ id: request.id })}
            onDelete={() => {
              if (window.confirm(`Delete "${request.name}"? This can't be undone.`)) {
                deleteRequest.mutate(request.id);
              }
            }}
          />
        </div>
      ))}
    </div>
  );
}
