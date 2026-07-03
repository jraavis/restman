//! Non-intrusive update prompt: fixed bottom-right card shown when the
//! startup auto-check (or any check) finds a release. "Install & Restart"
//! runs the shared download flow with a progress bar; "Later" hides it for
//! the session. Mounted once in `AppShell`.

import { X } from "lucide-react";
import { formatBytes, UpdateProgressBar } from "./UpdateProgressBar";
import { useUpdaterStore } from "./useUpdater";

export function UpdateBanner() {
  const phase = useUpdaterStore((s) => s.phase);
  const update = useUpdaterStore((s) => s.update);
  const progress = useUpdaterStore((s) => s.progress);
  const dismissed = useUpdaterStore((s) => s.dismissed);
  const installAndRestart = useUpdaterStore((s) => s.installAndRestart);
  const dismiss = useUpdaterStore((s) => s.dismiss);

  const visible =
    !dismissed && update != null && (phase === "available" || phase === "downloading" || phase === "installing");
  if (!visible) return null;

  const busy = phase !== "available";

  return (
    <div className="fixed right-4 bottom-4 z-50 w-80 rounded-lg border border-slate-200 bg-white p-3 shadow-xl dark:border-slate-700 dark:bg-slate-800">
      <div className="flex items-start justify-between gap-2">
        <div>
          <p className="text-sm font-medium text-slate-800 dark:text-slate-100">
            Update available
          </p>
          <p className="mt-0.5 font-mono text-xs text-slate-500 dark:text-slate-400">
            v{update.currentVersion} → v{update.version}
          </p>
          <p className="mt-0.5 text-xs text-slate-400">
            {phase === "installing"
              ? "Installing…"
              : phase === "downloading"
                ? progress.total != null
                  ? `Downloading… ${formatBytes(progress.downloaded)} of ${formatBytes(progress.total)}`
                  : "Downloading…"
                : "A new version of Restman is ready to install."}
          </p>
        </div>
        {!busy && (
          <button
            type="button"
            title="Dismiss"
            onClick={dismiss}
            className="flex h-6 w-6 shrink-0 items-center justify-center rounded-md text-slate-400 hover:bg-slate-100 dark:hover:bg-slate-700"
          >
            <X size={13} />
          </button>
        )}
      </div>

      {busy ? (
        <div className="mt-2.5">
          <UpdateProgressBar progress={progress} />
        </div>
      ) : (
        <div className="mt-2.5 flex items-center gap-2">
          <button
            type="button"
            onClick={() => void installAndRestart()}
            className="rounded-lg bg-accent px-3 py-1.5 text-xs font-medium text-white hover:bg-accent-hover"
          >
            Install &amp; Restart
          </button>
          <button
            type="button"
            onClick={dismiss}
            className="rounded-lg border border-slate-200 px-3 py-1.5 text-xs text-slate-600 hover:bg-slate-100 dark:border-slate-700 dark:text-slate-300 dark:hover:bg-slate-700"
          >
            Later
          </button>
        </div>
      )}
    </div>
  );
}
