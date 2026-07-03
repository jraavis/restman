//! Centered brand mark behind the workspace — decorative only.

export function Watermark() {
  return (
    <div
      aria-hidden
      className="pointer-events-none absolute inset-0 z-0 flex items-center justify-center select-none"
    >
      <img
        src="/favicon.svg"
        alt=""
        className="w-[min(40vw,360px)] opacity-[0.04] mix-blend-multiply dark:opacity-[0.06] dark:mix-blend-screen"
      />
    </div>
  );
}