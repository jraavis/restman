//! Shared drag payload for the collections tree. A single mutable ref (not
//! React state — a drag shouldn't trigger re-renders) threaded down through
//! `CollectionsPanel` -> `CollectionNode` -> `RequestList`, mirroring the
//! `dragIndex` ref `TabsBar` uses for tab reordering, just carrying enough
//! identity to tell a reorder (same parent) apart from a move (different
//! parent) once it lands on a drop target.

import type { MutableRefObject } from "react";

export type DragItem =
  | { kind: "collection"; id: string; parentId: string | null }
  | { kind: "request"; id: string; collectionId: string };

export type DragRef = MutableRefObject<DragItem | null>;
