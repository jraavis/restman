//! Ephemeral WebSocket client — connect to any `ws(s)://` endpoint, send
//! text/binary frames, and watch the bidirectional transcript live. Not wired
//! into saved requests/collections (see PLAN.md #17b) — same standalone
//! connect/watch/disconnect surface and modal shell as `SsePanel`, plus a
//! send composer. The handshake goes through the workspace's reqwest transport
//! (proxy / client cert / default headers), unlike a raw browser WebSocket.

import { useState } from "react";
import { ArrowDown, ArrowUp, Cable, Loader2 } from "lucide-react";
import { isValidUrl, protocolOf } from "../../lib/methods";
import type { HeaderEntry } from "../../lib/http";
import { KeyValueEditor, type Pair } from "../request/KeyValueEditor";
import { useWsConnection, type WsLogEntry, type WsStatus } from "./wsHooks";

function headersToRows(headers: HeaderEntry[]): Pair[] {
  return headers.map((h) => ({ key: h.name, value: h.value, enabled: h.enabled }));
}
function rowsToHeaders(rows: Pair[]): HeaderEntry[] {
  return rows.map((r) => ({ name: r.key, value: r.value, enabled: r.enabled }));
}

export function WsPanel({ workspaceId, onClose }: { workspaceId: string; onClose: () => void }) {
  const [url, setUrl] = useState("");
  const [headers, setHeaders] = useState<HeaderEntry[]>([]);
  const [draft, setDraft] = useState("");
  const [binary, setBinary] = useState(false);
  const { status, log, errorMessage, connect, disconnect, send } = useWsConnection();

  const scheme = protocolOf(url);
  const urlOk = url.trim() !== "" && isValidUrl(url) && (scheme === "ws" || scheme === "wss");
  const busy = status === "connecting" || status === "open";
  const canSend = status === "open" && draft !== "";

  function handleSend() {
    if (!canSend) return;
    void send({ binary, data: draft });
    setDraft("");
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/30" onClick={onClose}>
      <div
        onClick={(e) => e.stopPropagation()}
        className="flex max-h-[85vh] w-[36rem] flex-col rounded-lg border border-slate-200 bg-white p-4 shadow-xl dark:border-slate-700 dark:bg-slate-800"
      >
        <div className="mb-3 flex items-center justify-between">
          <h2 className="flex items-center gap-1.5 text-sm font-semibold text-slate-800 dark:text-slate-100">
            <Cable size={14} /> WebSocket
          </h2>
          <StatusBadge status={status} />
        </div>

        <div className="flex items-center gap-2">
          <input
            value={url}
            onChange={(e) => setUrl(e.target.value)}
            disabled={busy}
            placeholder="wss://echo.websocket.org"
            spellCheck={false}
            className="flex-1 rounded-md border border-slate-200 bg-transparent px-2.5 py-1.5 text-sm focus:outline-none focus:ring-2 focus:ring-accent/40 disabled:opacity-60 dark:border-slate-700"
          />
          {busy ? (
            <button
              type="button"
              onClick={disconnect}
              className="rounded-md bg-red-500 px-3 py-1.5 text-sm font-medium text-white hover:bg-red-600"
            >
              Disconnect
            </button>
          ) : (
            <button
              type="button"
              disabled={!urlOk}
              onClick={() => void connect(workspaceId, url, headers)}
              className="rounded-md bg-accent px-3 py-1.5 text-sm font-medium text-white disabled:opacity-40"
            >
              Connect
            </button>
          )}
        </div>

        <details className="mt-2 text-xs">
          <summary className="cursor-pointer text-slate-500 dark:text-slate-400">
            Headers {headers.filter((h) => h.enabled).length > 0 && `(${headers.filter((h) => h.enabled).length})`}
          </summary>
          <div className="mt-2">
            <KeyValueEditor
              rows={headersToRows(headers)}
              onChange={(rows) => setHeaders(rowsToHeaders(rows))}
              keyPlaceholder="Header"
              valuePlaceholder="Value"
            />
          </div>
        </details>

        {errorMessage && (
          <p className="mt-2 rounded-md bg-red-50 px-2 py-1 text-xs text-red-600 dark:bg-red-900/30 dark:text-red-400">
            {errorMessage}
          </p>
        )}

        <div className="mt-3 min-h-0 flex-1 overflow-auto rounded-md border border-slate-100 dark:border-slate-700">
          {log.length === 0 ? (
            <div className="flex flex-col items-center justify-center gap-1 p-6 text-center text-sm text-slate-400">
              <p>No messages yet.</p>
              <p className="text-xs">Connect to a socket to send and receive frames here.</p>
            </div>
          ) : (
            log.map((entry) => <LogRow key={entry.id} entry={entry} />)
          )}
        </div>

        <div className="mt-3 flex items-end gap-2">
          <textarea
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
                e.preventDefault();
                handleSend();
              }
            }}
            disabled={status !== "open"}
            rows={2}
            placeholder={status === "open" ? "Message…  (⌘/Ctrl+Enter to send)" : "Connect to send"}
            spellCheck={false}
            className="flex-1 resize-none rounded-md border border-slate-200 bg-transparent px-2.5 py-1.5 font-mono text-xs focus:outline-none focus:ring-2 focus:ring-accent/40 disabled:opacity-60 dark:border-slate-700"
          />
          <div className="flex flex-col items-stretch gap-1">
            <label className="flex items-center gap-1 text-xs text-slate-500 dark:text-slate-400" title="Send the input as a binary frame (input is base64)">
              <input type="checkbox" checked={binary} onChange={(e) => setBinary(e.target.checked)} disabled={status !== "open"} />
              base64
            </label>
            <button
              type="button"
              disabled={!canSend}
              onClick={handleSend}
              className="rounded-md bg-accent px-3 py-1.5 text-sm font-medium text-white disabled:opacity-40"
            >
              Send
            </button>
          </div>
        </div>

        <div className="mt-3 flex justify-end text-sm">
          <button
            type="button"
            onClick={onClose}
            className="rounded-md px-3 py-1.5 text-slate-500 hover:bg-slate-100 dark:hover:bg-slate-700"
          >
            Close
          </button>
        </div>
      </div>
    </div>
  );
}

function StatusBadge({ status }: { status: WsStatus }) {
  const labels: Record<WsStatus, string> = {
    idle: "Idle",
    connecting: "Connecting…",
    open: "Open",
    closed: "Closed",
    error: "Error",
  };
  const classes: Record<WsStatus, string> = {
    idle: "text-slate-400",
    connecting: "text-amber-500",
    open: "text-green-600 dark:text-green-400",
    closed: "text-slate-400",
    error: "text-red-500",
  };
  return (
    <span className={"flex items-center gap-1 text-xs font-medium " + classes[status]}>
      {status === "connecting" && <Loader2 size={11} className="animate-spin" />}
      {labels[status]}
    </span>
  );
}

function LogRow({ entry }: { entry: WsLogEntry }) {
  const time = new Date(entry.receivedAt).toLocaleTimeString();
  const { event, direction } = entry;

  if (event.type === "open") return <SystemRow time={time} text="Connection opened" />;
  if (event.type === "error") return <SystemRow time={time} text={`Error: ${event.message}`} isError />;
  if (event.type === "closed") {
    const detail =
      event.code != null ? ` (${event.code}${event.reason ? `: ${event.reason}` : ""})` : "";
    return <SystemRow time={time} text={`Connection closed${detail}`} />;
  }

  const sent = direction === "out";
  return (
    <div className="flex items-start gap-2 border-b border-slate-100 px-2 py-1.5 text-xs last:border-0 dark:border-slate-800">
      <span className="shrink-0 text-slate-400">{time}</span>
      {sent ? (
        <ArrowUp size={12} className="mt-0.5 shrink-0 text-accent" />
      ) : (
        <ArrowDown size={12} className="mt-0.5 shrink-0 text-green-500" />
      )}
      <div className="min-w-0 flex-1">
        {event.binary && (
          <span className="mr-1.5 rounded border border-slate-200 px-1 py-0.5 text-slate-500 dark:border-slate-700">
            binary
          </span>
        )}
        <pre className="mt-0.5 whitespace-pre-wrap break-all font-mono text-slate-700 dark:text-slate-300">
          {event.data}
        </pre>
      </div>
    </div>
  );
}

function SystemRow({ time, text, isError }: { time: string; text: string; isError?: boolean }) {
  return (
    <div className="flex items-center gap-2 border-b border-slate-100 px-2 py-1.5 text-xs last:border-0 dark:border-slate-800">
      <span className="shrink-0 text-slate-400">{time}</span>
      <span className={isError ? "text-red-500" : "text-slate-400 italic"}>{text}</span>
    </div>
  );
}
