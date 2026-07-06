//! Small brand mark in the bottom-right of the workspace — decorative only.

export function Watermark() {
  return (
    <div
      aria-hidden
      className="pointer-events-none absolute right-4 bottom-4 z-0 select-none"
    >
      <img
        src="/favicon.svg"
        alt=""
        className="size-8 opacity-[0.04] mix-blend-multiply dark:opacity-[0.06] dark:mix-blend-screen"
      />
    </div>
  );
}