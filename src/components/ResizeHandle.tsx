//! A draggable divider between two panes. Reports relative drag delta in
//! pixels via onResize; the caller owns the actual size state (so it can
//! clamp/persist it). Pointer Events so a single listener covers mouse + touch.

import { useRef } from "react";

interface Props {
  orientation: "vertical" | "horizontal";
  onResize: (deltaPx: number) => void;
  className?: string;
}

export function ResizeHandle({ orientation, onResize, className = "" }: Props) {
  const last = useRef(0);
  const dragging = useRef(false);

  const onPointerDown = (e: React.PointerEvent) => {
    dragging.current = true;
    last.current = orientation === "vertical" ? e.clientX : e.clientY;
    (e.target as Element).setPointerCapture(e.pointerId);
  };
  const onPointerMove = (e: React.PointerEvent) => {
    if (!dragging.current) return;
    const pos = orientation === "vertical" ? e.clientX : e.clientY;
    onResize(pos - last.current);
    last.current = pos;
  };
  const onPointerUp = (e: React.PointerEvent) => {
    dragging.current = false;
    (e.target as Element).releasePointerCapture(e.pointerId);
  };

  return (
    <div
      role="separator"
      aria-orientation={orientation === "vertical" ? "vertical" : "horizontal"}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      className={
        (orientation === "vertical"
          ? "resize-handle-x w-1 shrink-0"
          : "resize-handle-y h-1 shrink-0") +
        " group relative shrink-0 bg-transparent " +
        className
      }
    >
      <div
        className={
          "absolute bg-transparent transition-colors group-hover:bg-accent/40 group-active:bg-accent/60 " +
          (orientation === "vertical"
            ? "inset-y-0 left-1/2 w-px -translate-x-1/2"
            : "inset-x-0 top-1/2 h-px -translate-y-1/2")
        }
      />
    </div>
  );
}
