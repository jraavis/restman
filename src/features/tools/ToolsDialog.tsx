//! Developer tools dialog: encode/decode utilities for API debugging.

import { useState } from "react";
import { TOOLS, type ToolId } from "./registry";

export function ToolsDialog({ onClose }: { onClose: () => void }) {
  const [tab, setTab] = useState<ToolId>("base64");
  const active = TOOLS.find((t) => t.id === tab) ?? TOOLS[0];
  const ActiveTool = active.component;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/30 p-4" onClick={onClose}>
      <div
        onClick={(e) => e.stopPropagation()}
        className="flex h-[min(40rem,90vh)] w-[min(52rem,95vw)] overflow-hidden rounded-lg border border-slate-200 bg-white shadow-xl dark:border-slate-700 dark:bg-slate-800"
      >
        <nav className="w-36 shrink-0 overflow-y-auto border-r border-slate-100 p-2 dark:border-slate-700">
          <p className="mb-2 px-2.5 py-1 text-xs font-semibold tracking-wide text-slate-400 uppercase dark:text-slate-500">
            Tools
          </p>
          {TOOLS.map((t) => (
            <button
              key={t.id}
              type="button"
              onClick={() => setTab(t.id)}
              className={
                "block w-full rounded-md px-2.5 py-1.5 text-left text-sm " +
                (tab === t.id
                  ? "bg-accent/10 font-medium text-accent"
                  : "text-slate-600 hover:bg-slate-100 dark:text-slate-300 dark:hover:bg-slate-700")
              }
            >
              {t.label}
            </button>
          ))}
        </nav>
        <div className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden p-4">
          <h2 className="mb-3 shrink-0 text-sm font-semibold text-slate-800 dark:text-slate-100">{active.label}</h2>
          <div className="min-h-0 min-w-0 flex-1 overflow-hidden">
            <ActiveTool />
          </div>
        </div>
      </div>
    </div>
  );
}