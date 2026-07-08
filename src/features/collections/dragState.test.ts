import type { DragEvent } from "react";
import { describe, expect, it } from "vitest";
import {
  DRAG_MIME,
  beginDrag,
  beginTabDrag,
  clearDrag,
  clearTabDrag,
  finishDrag,
  finishTabDrag,
  resolveDragItem,
  resolveTabIndex,
  type DragItem,
  type DragRef,
} from "./dragState";

function makeDragEvent(dropEffect = "none"): DragEvent {
  const data = new Map<string, string>();
  return {
    stopPropagation: () => {},
    dataTransfer: {
      setData: (type: string, value: string) => {
        data.set(type, value);
      },
      getData: (type: string) => data.get(type) ?? "",
      effectAllowed: "none",
      dropEffect,
    },
  } as unknown as DragEvent;
}

describe("dragState", () => {
  it("beginDrag stores the item and sets dataTransfer", () => {
    const dragRef: DragRef = { current: null };
    const item: DragItem = { kind: "request", id: "req-1", collectionId: "col-1" };
    const e = makeDragEvent();

    beginDrag(e, item, dragRef);

    expect(dragRef.current).toEqual(item);
    expect(e.dataTransfer!.getData("text/plain")).toBe("req-1");
    expect(e.dataTransfer!.getData(DRAG_MIME)).toBe(JSON.stringify(item));
    expect(e.dataTransfer!.effectAllowed).toBe("move");
  });

  it("beginTabDrag stores the index and sets dataTransfer", () => {
    const dragIndex = { current: null as number | null };
    const e = makeDragEvent();

    beginTabDrag(e, 2, dragIndex);

    expect(dragIndex.current).toBe(2);
    expect(e.dataTransfer!.getData("text/plain")).toBe("tab:2");
    expect(e.dataTransfer!.getData(DRAG_MIME)).toBe(JSON.stringify({ kind: "tab", index: 2 }));
  });

  it("resolveDragItem prefers the ref and falls back to dataTransfer", () => {
    const dragRef: DragRef = { current: null };
    const item: DragItem = { kind: "collection", id: "c-1", parentId: null };
    const e = makeDragEvent();
    beginDrag(e, item, dragRef);

    expect(resolveDragItem(e, dragRef)).toEqual(item);

    dragRef.current = null;
    expect(resolveDragItem(e, dragRef)).toEqual(item);
  });

  it("resolveTabIndex prefers the ref and falls back to dataTransfer", () => {
    const dragIndex = { current: null as number | null };
    const e = makeDragEvent();
    beginTabDrag(e, 4, dragIndex);

    expect(resolveTabIndex(e, dragIndex)).toBe(4);

    dragIndex.current = null;
    expect(resolveTabIndex(e, dragIndex)).toBe(4);
  });

  it("clearDrag resets the ref", () => {
    const dragRef: DragRef = { current: { kind: "collection", id: "c-1", parentId: null } };
    clearDrag(dragRef);
    expect(dragRef.current).toBeNull();
  });

  it("finishDrag clears only cancelled drags", () => {
    const dragRef: DragRef = { current: { kind: "request", id: "r-1", collectionId: "c-1" } };

    finishDrag(dragRef, makeDragEvent("move"));
    expect(dragRef.current).not.toBeNull();

    finishDrag(dragRef, makeDragEvent("none"));
    expect(dragRef.current).toBeNull();
  });

  it("finishTabDrag clears only cancelled drags", () => {
    const dragIndex = { current: 1 as number | null };

    finishTabDrag(dragIndex, makeDragEvent("move"));
    expect(dragIndex.current).toBe(1);

    finishTabDrag(dragIndex, makeDragEvent("none"));
    expect(dragIndex.current).toBeNull();
  });

  it("clearTabDrag resets the tab index ref", () => {
    const dragIndex = { current: 3 as number | null };
    clearTabDrag(dragIndex);
    expect(dragIndex.current).toBeNull();
  });
});