//! Shared native drag-and-drop helpers for the collections tree and tabs bar.
//! Tauri's WKWebView requires `dataTransfer.setData()` for HTML5 drag to start;
//! a mutable ref carries the live payload without re-renders, with dataTransfer
//! as a fallback when drop handlers run after dragend clears the ref.

import type { DragEvent, MutableRefObject } from "react";

export const DRAG_MIME = "application/x-restman-drag";

export type DragItem =
  | { kind: "collection"; id: string; parentId: string | null }
  | { kind: "request"; id: string; collectionId: string };

export type TabDragPayload = { kind: "tab"; index: number };

export type NativeDragPayload = DragItem | TabDragPayload;

export type DragRef = MutableRefObject<DragItem | null>;

function encodePayload(payload: NativeDragPayload): string {
  return JSON.stringify(payload);
}

function decodePayload(raw: string): NativeDragPayload | null {
  if (!raw) return null;
  try {
    const parsed: unknown = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object" || !("kind" in parsed)) return null;
    const kind = (parsed as { kind: string }).kind;
    if (kind === "tab" && "index" in parsed && typeof (parsed as TabDragPayload).index === "number") {
      return parsed as TabDragPayload;
    }
    if (kind === "collection" && "id" in parsed && typeof (parsed as DragItem & { kind: "collection" }).id === "string") {
      return parsed as Extract<DragItem, { kind: "collection" }>;
    }
    if (kind === "request" && "id" in parsed && typeof (parsed as DragItem & { kind: "request" }).id === "string") {
      return parsed as Extract<DragItem, { kind: "request" }>;
    }
    return null;
  } catch {
    return null;
  }
}

function readPayload(e: DragEvent): NativeDragPayload | null {
  return decodePayload(e.dataTransfer.getData(DRAG_MIME));
}

/** Start a native drag — sets dataTransfer (required by Tauri's WebView). */
export function beginNativeDrag(e: DragEvent, payload: NativeDragPayload, plainFallback: string) {
  e.stopPropagation();
  e.dataTransfer.setData("text/plain", plainFallback);
  e.dataTransfer.setData(DRAG_MIME, encodePayload(payload));
  e.dataTransfer.effectAllowed = "move";
}

/** Start dragging a collection-tree item. */
export function beginDrag(e: DragEvent, item: DragItem, dragRef: DragRef) {
  dragRef.current = item;
  beginNativeDrag(e, item, item.id);
}

/** Start dragging a tab by index. */
export function beginTabDrag(e: DragEvent, index: number, dragIndexRef: MutableRefObject<number | null>) {
  dragIndexRef.current = index;
  beginNativeDrag(e, { kind: "tab", index }, `tab:${index}`);
}

/** Resolve a collection-tree drag item from the ref or dataTransfer fallback. */
export function resolveDragItem(e: DragEvent, dragRef: DragRef): DragItem | null {
  if (dragRef.current) return dragRef.current;
  const payload = readPayload(e);
  if (payload && payload.kind !== "tab") return payload;
  return null;
}

/** Resolve a tab drag index from the ref or dataTransfer fallback. */
export function resolveTabIndex(e: DragEvent, dragIndexRef: MutableRefObject<number | null>): number | null {
  if (dragIndexRef.current !== null) return dragIndexRef.current;
  const payload = readPayload(e);
  if (payload?.kind === "tab") return payload.index;
  const plain = e.dataTransfer.getData("text/plain");
  const match = /^tab:(\d+)$/.exec(plain);
  return match ? Number(match[1]) : null;
}

export function clearDrag(dragRef: DragRef) {
  dragRef.current = null;
}

export function clearTabDrag(dragIndexRef: MutableRefObject<number | null>) {
  dragIndexRef.current = null;
}

/** Clear refs only when the drag was cancelled (dropEffect stays "none"). */
export function finishDrag(dragRef: DragRef, e?: DragEvent) {
  if (!e || e.dataTransfer.dropEffect === "none") {
    dragRef.current = null;
  }
}

export function finishTabDrag(dragIndexRef: MutableRefObject<number | null>, e?: DragEvent) {
  if (!e || e.dataTransfer.dropEffect === "none") {
    dragIndexRef.current = null;
  }
}
