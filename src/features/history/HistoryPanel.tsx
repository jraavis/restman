//! History panel: search/filter the auto-saved request log, replay or delete
//! entries, clear with confirm.

import { useState } from "react";
import { AlertCircle, GitCompare, Loader2, RotateCcw, Search, Trash2, X } from "lucide-react";
import { useActiveWorkspace } from "../workspaces/hooks";
import { useClearHistory, useDeleteHistoryEntry, useHistory, useReplayIntoDraft } from "./hooks";
import { HTTP_METHODS, methodBadgeClasses, statusColor } from "../../lib/methods";
import { formatMs } from "../../lib/encoding";
import { HistoryDiffDialog } from "./HistoryDiffDialog";
import type { HistoryEntry, HistoryFilter } from "../../lib/types";

type StatusClass = "any" | "2xx" | "3xx" | "4xx" | "5xx";

const STATUS_RANGES: Record<StatusClass, [number, number] | null> = {
  any: null,
  "2xx": [200, 299],
  "3xx": [300, 399],
  "4xx": [400, 499],
  "5xx": [500, 599],
};

export function HistoryPanel() {
  const { data: workspace } = useActiveWorkspace();
  const [text, setText] = useState("");
  const [method, setMethod] = useState<string>("any");
  const [statusClass, setStatusClass] = useState<StatusClass>("any");
  const [dateFrom, setDateFrom] = useState("");
  const [dateTo, setDateTo] = useState("");

  const [statusMin, statusMax] = STATUS_RANGES[statusClass] ?? [null, null];
  const filter: HistoryFilter = {
    text: text.trim() || null,
    method: method === "any" ? null : method,
    statusMin,
    statusMax,
    dateMin: dateFrom ? new Date(`${dateFrom}T00:00:00`).getTime() : null,
    dateMax: dateTo ? new Date(`${dateTo}T23:59:59.999`).getTime() : null,
    limit: 200,
  };
  const hasDateFilter = !!dateFrom || !!dateTo;

  const { data: entries, isLoading } = useHistory(workspace?.id, filter);
  const deleteEntry = useDeleteHistoryEntry(workspace?.id);
  const clearHistory = useClearHistory(workspace?.id);
  const replay = useReplayIntoDraft(workspace?.id);

  // How many times each URL appears in the (filtered) log, to badge repeats.
  const urlCounts = new Map<string, number>();
  for (const e of entries ?? []) urlCounts.set(e.url, (urlCounts.get(e.url) ?? 0) + 1);

  const [compareMode, setCompareMode] = useState(false);
  const [selectedIds, setSelectedIds] = useState<string[]>([]);
  const [diffPair, setDiffPair] = useState<[HistoryEntry, HistoryEntry] | null>(null);

  const handleClear = () => {
    if (!workspace) return;
    if (window.confirm("Clear all history for this workspace? This can't be undone.")) {
      clearHistory.mutate();
    }
  };

  function toggleCompareMode() {
    setCompareMode((m) => !m);
    setSelectedIds([]);
  }

  function toggleSelected(id: string) {
    setSelectedIds((ids) => {
      if (ids.includes(id)) return ids.filter((x) => x !== id);
      if (ids.length >= 2) return [ids[1], id]; // keep the most recent 2 picks
      return [...ids, id];
    });
  }

  function openDiff() {
    if (selectedIds.length !== 2 || !entries) return;
    const a = entries.find((e) => e.id === selectedIds[0]);
    const b = entries.find((e) => e.id === selectedIds[1]);
    if (a && b) setDiffPair([a, b]);
  }

  return (
    <div className="flex h-full flex-col">
      <div className="flex flex-col gap-2 border-b border-slate-100 p-2 dark:border-slate-800">
        <div className="relative">
          <Search size={13} className="absolute left-2 top-1/2 -translate-y-1/2 text-slate-400" />
          <input
            value={text}
            onChange={(e) => setText(e.target.value)}
            placeholder="Search name or URL"
            className="w-full rounded-md border border-slate-200 bg-transparent py-1 pl-6 pr-2 text-xs focus:outline-none focus:ring-2 focus:ring-accent/40 dark:border-slate-700"
          />
        </div>
        <div className="flex gap-1.5">
          <select
            value={method}
            onChange={(e) => setMethod(e.target.value)}
            className="flex-1 rounded-md border border-slate-200 bg-transparent px-1.5 py-1 text-xs focus:outline-none focus:ring-2 focus:ring-accent/40 dark:border-slate-700"
          >
            <option value="any">Any method</option>
            {HTTP_METHODS.map((m) => (
              <option key={m} value={m}>
                {m}
              </option>
            ))}
          </select>
          <select
            value={statusClass}
            onChange={(e) => setStatusClass(e.target.value as StatusClass)}
            className="flex-1 rounded-md border border-slate-200 bg-transparent px-1.5 py-1 text-xs focus:outline-none focus:ring-2 focus:ring-accent/40 dark:border-slate-700"
          >
            <option value="any">Any status</option>
            <option value="2xx">2xx</option>
            <option value="3xx">3xx</option>
            <option value="4xx">4xx</option>
            <option value="5xx">5xx</option>
          </select>
        </div>
        <div className="flex items-center gap-1.5">
          <input
            type="date"
            value={dateFrom}
            onChange={(e) => setDateFrom(e.target.value)}
            max={dateTo || undefined}
            title="From date"
            className="flex-1 rounded-md border border-slate-200 bg-transparent px-1.5 py-1 text-xs focus:outline-none focus:ring-2 focus:ring-accent/40 dark:border-slate-700"
          />
          <span className="text-slate-400">–</span>
          <input
            type="date"
            value={dateTo}
            onChange={(e) => setDateTo(e.target.value)}
            min={dateFrom || undefined}
            title="To date"
            className="flex-1 rounded-md border border-slate-200 bg-transparent px-1.5 py-1 text-xs focus:outline-none focus:ring-2 focus:ring-accent/40 dark:border-slate-700"
          />
          {hasDateFilter && (
            <button
              type="button"
              title="Clear dates"
              onClick={() => {
                setDateFrom("");
                setDateTo("");
              }}
              className="shrink-0 rounded p-1 text-slate-400 hover:bg-slate-100 hover:text-slate-600 dark:hover:bg-slate-800"
            >
              <X size={12} />
            </button>
          )}
        </div>
        <div className="flex items-center justify-between">
          <button
            type="button"
            onClick={handleClear}
            disabled={!entries?.length}
            className="text-xs text-slate-400 hover:text-red-500 disabled:opacity-40 disabled:hover:text-slate-400"
          >
            Clear all
          </button>
          <button
            type="button"
            onClick={toggleCompareMode}
            disabled={!entries || entries.length < 2}
            className={
              "flex items-center gap-1 rounded-md px-2 py-1 text-xs font-medium disabled:opacity-40 " +
              (compareMode
                ? "bg-accent/10 text-accent"
                : "text-slate-500 hover:bg-slate-100 dark:text-slate-400 dark:hover:bg-slate-800")
            }
          >
            <GitCompare size={12} /> Compare
          </button>
        </div>
      </div>

      {compareMode && (
        <div className="flex items-center justify-between border-b border-slate-100 bg-slate-50 px-2 py-1.5 text-xs dark:border-slate-800 dark:bg-slate-900">
          <span className="text-slate-500 dark:text-slate-400">Pick 2 entries to compare ({selectedIds.length}/2)</span>
          <button
            type="button"
            onClick={openDiff}
            disabled={selectedIds.length !== 2}
            className="rounded-md bg-accent px-2 py-1 font-medium text-white disabled:opacity-40"
          >
            Compare selected
          </button>
        </div>
      )}

      <div className="min-h-0 flex-1 overflow-auto">
        {isLoading && (
          <div className="flex items-center justify-center gap-2 p-6 text-sm text-slate-400">
            <Loader2 size={15} className="animate-spin" /> Loading…
          </div>
        )}
        {!isLoading && entries?.length === 0 && (
          <div className="flex flex-col items-center justify-center gap-1 p-6 text-center text-sm text-slate-400">
            <p>No history yet.</p>
            <p className="text-xs">Send a request and it'll show up here.</p>
          </div>
        )}
        {entries?.map((entry) => (
          <HistoryRow
            key={entry.id}
            entry={entry}
            urlCount={urlCounts.get(entry.url) ?? 1}
            onReplay={() => void replay(entry)}
            onDelete={() => deleteEntry.mutate(entry.id)}
            compareMode={compareMode}
            selected={selectedIds.includes(entry.id)}
            onToggleSelected={() => toggleSelected(entry.id)}
          />
        ))}
      </div>

      {diffPair && (
        <HistoryDiffDialog entryA={diffPair[0]} entryB={diffPair[1]} onClose={() => setDiffPair(null)} />
      )}
    </div>
  );
}

function HistoryRow({
  entry,
  urlCount,
  onReplay,
  onDelete,
  compareMode,
  selected,
  onToggleSelected,
}: {
  entry: HistoryEntry;
  urlCount: number;
  onReplay: () => void;
  onDelete: () => void;
  compareMode: boolean;
  selected: boolean;
  onToggleSelected: () => void;
}) {
  return (
    <div
      onClick={compareMode ? onToggleSelected : undefined}
      className={
        "group flex items-start gap-2 border-b border-slate-100 px-2 py-2 text-xs dark:border-slate-800 " +
        (compareMode
          ? "cursor-pointer " + (selected ? "bg-accent/10" : "hover:bg-slate-50 dark:hover:bg-slate-800/50")
          : "hover:bg-slate-50 dark:hover:bg-slate-800/50")
      }
    >
      {compareMode && (
        <input
          type="checkbox"
          checked={selected}
          onChange={onToggleSelected}
          onClick={(e) => e.stopPropagation()}
          className="mt-1 shrink-0"
        />
      )}
      <span
        className={
          "mt-0.5 shrink-0 rounded border px-1.5 py-0.5 font-bold " + methodBadgeClasses(entry.method)
        }
      >
        {entry.method}
      </span>
      <div className="min-w-0 flex-1">
        <p className="flex items-center gap-1 font-medium text-slate-700 dark:text-slate-200" title={entry.url}>
          <span className="min-w-0 truncate">{entry.name}</span>
          {urlCount > 1 && (
            <span
              title={`This URL was used ${urlCount} times`}
              className="shrink-0 rounded-full bg-slate-100 px-1.5 text-[10px] font-semibold text-slate-500 dark:bg-slate-800 dark:text-slate-400"
            >
              ×{urlCount}
            </span>
          )}
        </p>
        <div className="mt-0.5 flex items-center gap-1.5 text-slate-400">
          {entry.status != null ? (
            <span className={"rounded px-1 font-semibold " + statusColor(entry.status)}>{entry.status}</span>
          ) : (
            <AlertCircle size={11} className="text-red-500" />
          )}
          {entry.durationMs != null && <span>{formatMs(entry.durationMs)}</span>}
          <span>{new Date(entry.createdAt).toLocaleString()}</span>
        </div>
        {entry.error && <p className="mt-0.5 truncate text-red-500" title={entry.error}>{entry.error}</p>}
      </div>
      {!compareMode && (
        <div className="flex shrink-0 gap-0.5 opacity-0 group-hover:opacity-100">
          <button
            type="button"
            title="Replay"
            onClick={onReplay}
            className="rounded p-1 text-slate-400 hover:bg-slate-200 hover:text-slate-700 dark:hover:bg-slate-700 dark:hover:text-slate-200"
          >
            <RotateCcw size={13} />
          </button>
          <button
            type="button"
            title="Delete"
            onClick={onDelete}
            className="rounded p-1 text-slate-400 hover:bg-red-100 hover:text-red-600 dark:hover:bg-red-900/40"
          >
            <Trash2 size={13} />
          </button>
        </div>
      )}
    </div>
  );
}
