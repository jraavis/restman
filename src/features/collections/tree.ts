//! Pure tree helpers shared by the collections panel: grouping collections by
//! parent and the descendant check that guards drag-drop reparenting.

import type { Collection, SavedRequest } from "../../lib/types";

export type SortMode = "manual" | "name" | "created" | "used";

export function childrenOf(
  collections: Collection[],
  parentId: string | null,
  mode: SortMode = "manual",
): Collection[] {
  const filtered = collections.filter((c) => c.parentId === parentId);
  switch (mode) {
    case "name":
      return filtered.sort((a, b) => a.name.localeCompare(b.name));
    case "created":
      return filtered.sort((a, b) => b.createdAt - a.createdAt);
    case "used":
      return filtered.sort((a, b) => b.updatedAt - a.updatedAt);
    default:
      return filtered.sort((a, b) => a.sortOrder - b.sortOrder);
  }
}

export function sortRequests(requests: SavedRequest[], mode: SortMode): SavedRequest[] {
  const copy = [...requests];
  switch (mode) {
    case "name":
      return copy.sort((a, b) => a.name.localeCompare(b.name));
    case "created":
      return copy.sort((a, b) => b.createdAt - a.createdAt);
    case "used":
      return copy.sort((a, b) => (b.lastUsedAt ?? 0) - (a.lastUsedAt ?? 0));
    default:
      return copy.sort((a, b) => a.sortOrder - b.sortOrder);
  }
}

/**
 * True if `maybeDescendantId` is `ancestorId` itself, or nested anywhere
 * under it. Used to block dropping a collection into its own subtree —
 * the backend may or may not reject that, but the UI shouldn't offer it.
 */
export function isDescendant(
  maybeDescendantId: string,
  ancestorId: string,
  collections: Collection[],
): boolean {
  let current: Collection | undefined = collections.find((c) => c.id === maybeDescendantId);
  while (current) {
    if (current.id === ancestorId) return true;
    current = current.parentId === null ? undefined : collections.find((c) => c.id === current!.parentId);
  }
  return false;
}
