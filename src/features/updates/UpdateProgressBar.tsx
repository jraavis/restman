//! Thin accent progress bar for update downloads, shared by `UpdateBanner`
//! and the Settings → About tab. Indeterminate (pulsing full bar) when the
//! release asset's content length is unknown.

import type { UpdateProgress } from "./useUpdater";

export function UpdateProgressBar({ progress }: { progress: UpdateProgress }) {
  const pct =
    progress.total != null && progress.total > 0
      ? Math.min(100, (progress.downloaded / progress.total) * 100)
      : null;

  return (
    <div className="h-1 w-full overflow-hidden rounded-full bg-slate-200 dark:bg-slate-700">
      <div
        className={"h-full rounded-full bg-accent " + (pct == null ? "w-full animate-pulse" : "")}
        style={pct != null ? { width: `${pct}%` } : undefined}
      />
    </div>
  );
}

export function formatBytes(n: number): string {
  if (n >= 1024 * 1024) return `${(n / (1024 * 1024)).toFixed(1)} MB`;
  if (n >= 1024) return `${(n / 1024).toFixed(0)} KB`;
  return `${n} B`;
}
