//! Shared input/output shell for developer tool tabs.

import type { ReactNode } from "react";
import { Copy } from "lucide-react";

export function ToolLayout({
  input,
  onInputChange,
  inputLabel = "Input",
  output,
  error,
  mode,
  modes,
  onModeChange,
  extra,
  actions,
}: {
  input: string;
  onInputChange: (value: string) => void;
  inputLabel?: string;
  output: string;
  error?: string;
  mode?: string;
  modes?: { id: string; label: string }[];
  onModeChange?: (id: string) => void;
  extra?: ReactNode;
  actions?: ReactNode;
}) {
  const copy = () => {
    if (output) void navigator.clipboard.writeText(output);
  };

  return (
    <div className="flex h-full min-h-0 min-w-0 flex-col gap-3 overflow-hidden">
      {(modes || actions) && (
        <div className="flex shrink-0 flex-wrap items-center gap-2">
          {modes && onModeChange && (
            <div className="flex rounded-md border border-slate-200 p-0.5 dark:border-slate-600">
              {modes.map((m) => (
                <button
                  key={m.id}
                  type="button"
                  onClick={() => onModeChange(m.id)}
                  className={
                    "rounded px-2.5 py-1 text-xs font-medium transition-colors " +
                    (mode === m.id
                      ? "bg-accent/10 text-accent"
                      : "text-slate-500 hover:text-slate-700 dark:text-slate-400 dark:hover:text-slate-200")
                  }
                >
                  {m.label}
                </button>
              ))}
            </div>
          )}
          {actions}
        </div>
      )}

      {extra && <div className="min-w-0 shrink-0">{extra}</div>}

      <label className="flex min-w-0 shrink-0 flex-col gap-1">
        <span className="text-xs font-medium text-slate-500 dark:text-slate-400">{inputLabel}</span>
        <textarea
          value={input}
          onChange={(e) => onInputChange(e.target.value)}
          spellCheck={false}
          className="h-[clamp(6rem,18vh,12rem)] w-full min-w-0 resize-none overflow-auto rounded-md border border-slate-200 bg-slate-50 px-3 py-2 font-mono text-sm break-all text-slate-800 focus:border-accent/50 focus:outline-none dark:border-slate-600 dark:bg-slate-900 dark:text-slate-100"
        />
      </label>

      <div className="flex min-h-0 min-w-0 flex-1 flex-col gap-1 overflow-hidden">
        <div className="flex shrink-0 items-center justify-between">
          <span className="text-xs font-medium text-slate-500 dark:text-slate-400">Output</span>
          <button
            type="button"
            onClick={copy}
            disabled={!output}
            title="Copy output"
            className="flex items-center gap-1 text-xs text-slate-400 hover:text-slate-600 disabled:opacity-40 dark:hover:text-slate-200"
          >
            <Copy size={12} /> Copy
          </button>
        </div>
        {error ? (
          <p className="shrink-0 rounded-md border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-600 dark:border-red-900/50 dark:bg-red-950/30 dark:text-red-400">
            {error}
          </p>
        ) : (
          <pre className="min-h-0 flex-1 overflow-auto whitespace-pre-wrap break-all rounded-md border border-slate-200 bg-slate-50 px-3 py-2 font-mono text-sm text-slate-800 dark:border-slate-600 dark:bg-slate-900 dark:text-slate-100">
            {output || " "}
          </pre>
        )}
      </div>
    </div>
  );
}