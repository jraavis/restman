//! Side-by-side diff of two history entries — request (method/url/headers/
//! body) and response (status/headers/body), by section. Pure frontend:
//! both entries already carry full snapshots, nothing to fetch.

import { Copy } from "lucide-react";
import { buildSideBySideDiff, historyDiffSections, type DiffRow } from "../../lib/historyDiff";
import type { HistoryEntry } from "../../lib/types";

interface Props {
  entryA: HistoryEntry;
  entryB: HistoryEntry;
  onClose: () => void;
}

export function HistoryDiffDialog({ entryA, entryB, onClose }: Props) {
  const sections = historyDiffSections(entryA, entryB);

  function copyDiff() {
    const text = sections
      .map((s) => {
        const rows = buildSideBySideDiff(s.before, s.after);
        const lines = rows.map((r) => unifiedLine(r));
        return `## ${s.label}\n${lines.join("\n")}`;
      })
      .join("\n\n");
    void navigator.clipboard.writeText(text);
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/30" onClick={onClose}>
      <div
        onClick={(e) => e.stopPropagation()}
        className="flex h-[36rem] w-[52rem] flex-col overflow-hidden rounded-lg border border-slate-200 bg-white shadow-xl dark:border-slate-700 dark:bg-slate-800"
      >
        <div className="flex items-center justify-between border-b border-slate-100 px-4 py-2.5 dark:border-slate-700">
          <div className="flex min-w-0 gap-4 text-xs">
            <span className="truncate text-slate-500 dark:text-slate-400" title={entryA.name}>
              A: {entryA.name}
            </span>
            <span className="truncate text-slate-500 dark:text-slate-400" title={entryB.name}>
              B: {entryB.name}
            </span>
          </div>
          <button
            type="button"
            onClick={copyDiff}
            className="flex shrink-0 items-center gap-1 rounded-md border border-slate-200 px-2 py-1 text-xs text-slate-600 hover:bg-slate-100 dark:border-slate-700 dark:text-slate-300 dark:hover:bg-slate-700"
          >
            <Copy size={12} /> Copy diff
          </button>
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto p-3">
          {sections.map((s) => (
            <DiffSection key={s.label} label={s.label} before={s.before} after={s.after} />
          ))}
        </div>
      </div>
    </div>
  );
}

function unifiedLine(row: DiffRow): string {
  if (row.kind === "added") return `+ ${row.right}`;
  if (row.kind === "removed") return `- ${row.left}`;
  return `  ${row.left}`;
}

function DiffSection({ label, before, after }: { label: string; before: string; after: string }) {
  const rows = buildSideBySideDiff(before, after);
  const unchanged = before === after;

  return (
    <div className="mb-3">
      <p className="mb-1 text-xs font-semibold tracking-wide text-slate-400 uppercase dark:text-slate-500">
        {label}
        {unchanged && <span className="ml-2 font-normal normal-case text-slate-300 dark:text-slate-600">(no change)</span>}
      </p>
      {!unchanged && (
        <div className="overflow-hidden rounded-md border border-slate-200 font-mono text-xs dark:border-slate-700">
          {rows.map((row, i) => (
            <div key={i} className="grid grid-cols-2">
              <div
                className={
                  "truncate whitespace-pre px-2 py-0.5 " +
                  (row.kind === "removed"
                    ? "bg-red-50 text-red-700 dark:bg-red-950/40 dark:text-red-300"
                    : "text-slate-600 dark:text-slate-300")
                }
              >
                {row.left ?? ""}
              </div>
              <div
                className={
                  "truncate whitespace-pre border-l border-slate-100 px-2 py-0.5 dark:border-slate-800 " +
                  (row.kind === "added"
                    ? "bg-green-50 text-green-700 dark:bg-green-950/40 dark:text-green-300"
                    : "text-slate-600 dark:text-slate-300")
                }
              >
                {row.right ?? ""}
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
