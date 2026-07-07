//! Collection test runner UI: launch, live progress stream, summary + export.

import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  CheckCircle,
  Download,
  Loader2,
  Play,
  XCircle,
} from "lucide-react";
import { listen } from "@tauri-apps/api/event";
import { save } from "@tauri-apps/plugin-dialog";
import { textToBase64 } from "../../lib/encoding";
import { ipc } from "../../lib/ipc";
import type {
  CollectionRunOptions,
  CollectionRunSummary,
  RequestRunResult,
  RunnerProgress,
} from "../../lib/types";
import { useRequests } from "./hooks";
import { buildCsvSample, buildJsonSample, extractTemplateVarNames } from "./runnerSampleData";

interface CollectionRunnerProps {
  workspaceId: string;
  collectionId: string;
  collectionName: string;
  onClose: () => void;
}

type RowState =
  | { status: "pending" }
  | { status: "running" }
  | { status: "done"; result: RequestRunResult };

interface RequestRow {
  requestId: string;
  name: string;
  state: RowState;
}

export function CollectionRunner({
  workspaceId,
  collectionId,
  collectionName,
  onClose,
}: CollectionRunnerProps) {
  const [running, setRunning] = useState(false);
  const [rows, setRows] = useState<RequestRow[]>([]);
  const [summary, setSummary] = useState<CollectionRunSummary | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [iterations, setIterations] = useState(1);
  const [delayMs, setDelayMs] = useState(0);
  const [parallel, setParallel] = useState(false);
  const [data, setData] = useState("");
  const unlisten = useRef<(() => void) | null>(null);
  const requestsQuery = useRequests(collectionId);
  const templateVarNames = useMemo(
    () => extractTemplateVarNames(requestsQuery.data ?? []),
    [requestsQuery.data],
  );

  // Clean up event listener on unmount.
  useEffect(() => () => unlisten.current?.(), []);

  const run = useCallback(async () => {
    setRunning(true);
    setRows([]);
    setSummary(null);
    setError(null);

    // Listen for progress events before firing the command so we don't miss
    // the first event if the runner is very fast.
    const ul = await listen<RunnerProgress>("runner:progress", (event) => {
      const p = event.payload;
      setRows((prev) => {
        const next = [...prev];
        const idx = next.findIndex((r) => r.requestId === p.requestId);
        if (idx === -1) {
          next.push({
            requestId: p.requestId,
            name: p.requestName,
            state: p.result ? { status: "done", result: p.result } : { status: "running" },
          });
        } else {
          next[idx] = {
            ...next[idx],
            state: p.result ? { status: "done", result: p.result } : { status: "running" },
          };
        }
        return next;
      });
    });
    unlisten.current = ul;

    const options: CollectionRunOptions = {
      workspaceId,
      collectionId,
      iterations,
      delayMs,
      parallel,
      data: data.trim() || null,
    };

    try {
      const result = await ipc.runCollectionTests(options);
      setSummary(result);
    } catch (e) {
      setError(typeof e === "string" ? e : String(e));
    } finally {
      setRunning(false);
      ul();
      unlisten.current = null;
    }
  }, [workspaceId, collectionId, iterations, delayMs, parallel, data]);

  const exportJunit = useCallback(async () => {
    if (!summary) return;
    const path = await save({ defaultPath: `${collectionName.replace(/\s+/g, "_")}_junit.xml` });
    if (!path) return;
    try {
      await ipc.writeFileBytes(path, textToBase64(summary.junitXml));
    } catch (e) {
      console.error("failed to export JUnit results:", e);
    }
  }, [summary, collectionName]);

  const exportJson = useCallback(async () => {
    if (!summary) return;
    const path = await save({ defaultPath: `${collectionName.replace(/\s+/g, "_")}_results.json` });
    if (!path) return;
    try {
      await ipc.writeFileBytes(path, textToBase64(JSON.stringify(summary, null, 2)));
    } catch (e) {
      console.error("failed to export JSON results:", e);
    }
  }, [summary, collectionName]);

  return (
    <div className="flex h-full flex-col bg-white text-sm dark:bg-slate-900">
      {/* Header */}
      <div className="flex items-center gap-2 border-b border-slate-200 px-4 py-3 dark:border-slate-700">
        <span className="font-semibold text-slate-800 dark:text-slate-100">
          Run collection: {collectionName}
        </span>
        <button
          type="button"
          onClick={onClose}
          className="ml-auto rounded px-2 py-0.5 text-xs text-slate-500 hover:bg-slate-100 dark:text-slate-400 dark:hover:bg-slate-800"
        >
          Close
        </button>
      </div>

      {/* Config strip */}
      {!running && !summary && (
        <div className="flex flex-wrap items-center gap-4 border-b border-slate-100 px-4 py-2 dark:border-slate-800">
          <label className="flex items-center gap-1.5 text-xs text-slate-600 dark:text-slate-300">
            Iterations
            <input
              type="number"
              min={1}
              value={iterations}
              onChange={(e) => setIterations(Math.max(1, Number(e.target.value)))}
              className="w-16 rounded border border-slate-200 px-2 py-0.5 text-xs dark:border-slate-700 dark:bg-slate-800"
            />
          </label>
          <label
            className={`flex items-center gap-1.5 text-xs text-slate-600 dark:text-slate-300 ${parallel ? "opacity-40" : ""}`}
            title={parallel ? "Ignored while running in parallel" : undefined}
          >
            Delay (ms)
            <input
              type="number"
              min={0}
              step={100}
              value={delayMs}
              disabled={parallel}
              onChange={(e) => setDelayMs(Math.max(0, Number(e.target.value)))}
              className="w-20 rounded border border-slate-200 px-2 py-0.5 text-xs disabled:cursor-not-allowed dark:border-slate-700 dark:bg-slate-800"
            />
          </label>
          <label className="flex items-center gap-1.5 text-xs text-slate-600 dark:text-slate-300">
            <input
              type="checkbox"
              checked={parallel}
              onChange={(e) => setParallel(e.target.checked)}
              className="rounded border-slate-300 dark:border-slate-600"
            />
            Run in parallel
          </label>
          <label className="flex flex-col gap-0.5 text-xs text-slate-600 dark:text-slate-300">
            <span className="flex items-center gap-1.5">
              Data (CSV/JSON)
              <button
                type="button"
                onClick={() => setData(buildCsvSample(templateVarNames))}
                title={
                  templateVarNames.length > 0
                    ? `Starter CSV using this collection's {{${templateVarNames.join("}}, {{")}}} vars`
                    : "Starter CSV — no {{vars}} found in this collection, using a generic example"
                }
                className="rounded border border-slate-200 px-1.5 py-0.5 text-[11px] text-slate-500 hover:bg-slate-100 dark:border-slate-700 dark:text-slate-400 dark:hover:bg-slate-800"
              >
                Insert CSV sample
              </button>
              <button
                type="button"
                onClick={() => setData(buildJsonSample(templateVarNames))}
                title={
                  templateVarNames.length > 0
                    ? `Starter JSON using this collection's {{${templateVarNames.join("}}, {{")}}} vars`
                    : "Starter JSON — no {{vars}} found in this collection, using a generic example"
                }
                className="rounded border border-slate-200 px-1.5 py-0.5 text-[11px] text-slate-500 hover:bg-slate-100 dark:border-slate-700 dark:text-slate-400 dark:hover:bg-slate-800"
              >
                Insert JSON sample
              </button>
            </span>
            <textarea
              value={data}
              onChange={(e) => setData(e.target.value)}
              placeholder='[{"id":"1"},{"id":"2"}]  or  id,name\n1,Alice'
              rows={2}
              className="w-64 rounded border border-slate-200 px-2 py-1 font-mono text-xs dark:border-slate-700 dark:bg-slate-800"
            />
          </label>
          <button
            type="button"
            onClick={() => void run()}
            className="ml-auto flex items-center gap-1.5 rounded-lg bg-accent px-3 py-1.5 text-xs font-semibold text-white hover:bg-accent-hover"
          >
            <Play size={12} />
            Run
          </button>
        </div>
      )}

      {/* Progress list */}
      {(running || rows.length > 0) && (
        <div className="min-h-0 flex-1 overflow-auto">
          {rows.map((row) => (
            <RunnerRow key={row.requestId} row={row} />
          ))}
          {running && (
            <div className="flex items-center gap-2 px-4 py-2 text-xs text-slate-400">
              <Loader2 size={13} className="animate-spin" />
              Running…
            </div>
          )}
        </div>
      )}

      {/* Error */}
      {error && (
        <div className="px-4 py-2 text-xs text-red-500">{error}</div>
      )}

      {/* Summary */}
      {summary && (
        <div className="border-t border-slate-200 px-4 py-3 dark:border-slate-700">
          <div className="flex flex-wrap items-center gap-4">
            <span className="flex items-center gap-1.5 font-semibold text-emerald-600 dark:text-emerald-400">
              <CheckCircle size={14} />
              {summary.passedTests} passed
            </span>
            {summary.failedTests > 0 && (
              <span className="flex items-center gap-1.5 font-semibold text-red-500">
                <XCircle size={14} />
                {summary.failedTests} failed
              </span>
            )}
            <span className="text-xs text-slate-500">
              {summary.totalRequests} requests ·{" "}
              {Math.round(summary.durationMs)}ms
            </span>
            <div className="ml-auto flex gap-2">
              <button
                type="button"
                onClick={exportJunit}
                className="flex items-center gap-1 rounded border border-slate-200 px-2 py-1 text-xs hover:bg-slate-50 dark:border-slate-700 dark:hover:bg-slate-800"
              >
                <Download size={11} />
                JUnit XML
              </button>
              <button
                type="button"
                onClick={exportJson}
                className="flex items-center gap-1 rounded border border-slate-200 px-2 py-1 text-xs hover:bg-slate-50 dark:border-slate-700 dark:hover:bg-slate-800"
              >
                <Download size={11} />
                JSON
              </button>
              <button
                type="button"
                onClick={() => { setSummary(null); setRows([]); }}
                className="rounded border border-slate-200 px-2 py-1 text-xs hover:bg-slate-50 dark:border-slate-700 dark:hover:bg-slate-800"
              >
                Run again
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

function RunnerRow({ row }: { row: RequestRow }) {
  const { state } = row;

  return (
    <div className="flex items-center gap-3 border-b border-slate-100 px-4 py-2 dark:border-slate-800">
      {/* Status icon */}
      <span className="w-4 shrink-0">
        {state.status === "running" && (
          <Loader2 size={13} className="animate-spin text-accent" />
        )}
        {state.status === "pending" && (
          <span className="block h-2 w-2 rounded-full bg-slate-300 dark:bg-slate-600" />
        )}
        {state.status === "done" &&
          (state.result.failed === 0 && !state.result.error ? (
            <CheckCircle size={13} className="text-emerald-500" />
          ) : (
            <XCircle size={13} className="text-red-500" />
          ))}
      </span>

      {/* Name */}
      <span className="flex-1 truncate text-slate-700 dark:text-slate-200">
        {row.name}
      </span>

      {/* Stats */}
      {state.status === "done" && (
        <span className="shrink-0 text-xs text-slate-500 dark:text-slate-400">
          {state.result.passed}✓ {state.result.failed > 0 ? `${state.result.failed}✗ ` : ""}
          {Math.round(state.result.durationMs)}ms
          {state.result.status ? ` · ${state.result.status}` : ""}
          {state.result.error && (
            <span className="ml-1 text-red-400">{state.result.error}</span>
          )}
        </span>
      )}
    </div>
  );
}
