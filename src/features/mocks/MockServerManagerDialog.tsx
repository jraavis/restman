//! Mock server manager: create/edit/delete workspace-scoped mock servers and
//! their method+path -> canned-response rules, start/stop each server's
//! live loopback socket. Same modal shell as `PluginManagerDialog`.

import { useRef, useState, type ChangeEvent } from "react";
import { confirmDelete } from "../../lib/confirmDelete";
import { save } from "@tauri-apps/plugin-dialog";
import { ChevronDown, ChevronRight, Download, Play, Plus, Square, Trash2, Upload } from "lucide-react";
import { useCollections } from "../collections/hooks";
import { KeyValueEditor, type Pair } from "../request/KeyValueEditor";
import { ipc } from "../../lib/ipc";
import { textToBase64 } from "../../lib/encoding";
import type { HeaderEntry } from "../../lib/http";
import type { BodyMatchMode, BodyMatcher, MockMatcher, MockRule, MockRuleInput, MockServer, MockServerInput } from "../../lib/types";
import {
  useCreateMockRule,
  useCreateMockServer,
  useCreateMockServerFromCollection,
  useDeleteMockRule,
  useDeleteMockServer,
  useExportMockServer,
  useImportMockServer,
  useMockRules,
  useMockServers,
  useRunningMockServerIds,
  useStartMockServer,
  useStopMockServer,
  useUpdateMockRule,
  useUpdateMockServer,
} from "./hooks";

const METHODS = ["ANY", "GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];

function emptyRuleInput(sortOrder: number): MockRuleInput {
  return {
    method: "GET",
    pathPattern: "/",
    status: 200,
    headers: [],
    body: "",
    delayMs: 0,
    sortOrder,
    queryMatchers: [],
    headerMatchers: [],
    bodyMatcher: null,
  };
}

export function MockServerManagerDialog({ workspaceId, onClose }: { workspaceId: string; onClose: () => void }) {
  const { data: servers } = useMockServers(workspaceId);
  const { data: runningIds } = useRunningMockServerIds();
  const { data: collections } = useCollections(workspaceId);
  const [selectedId, setSelectedId] = useState<string | "new" | null>(null);
  const [fromCollectionOpen, setFromCollectionOpen] = useState(false);

  const selected = selectedId && selectedId !== "new" ? servers?.find((s) => s.id === selectedId) : undefined;
  const running = new Set(runningIds ?? []);
  const importServer = useImportMockServer(workspaceId);
  const importFileRef = useRef<HTMLInputElement>(null);
  const [importError, setImportError] = useState<string | null>(null);

  async function onImportFile(e: ChangeEvent<HTMLInputElement>) {
    const file = e.target.files?.[0];
    e.target.value = "";
    if (!file) return;
    setImportError(null);
    try {
      const server = await importServer.mutateAsync(await file.text());
      setSelectedId(server.id);
    } catch (err) {
      setImportError(typeof err === "string" ? err : err instanceof Error ? err.message : String(err));
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/30" onClick={onClose}>
      <div
        onClick={(e) => e.stopPropagation()}
        className="flex h-[34rem] max-h-[85vh] w-[48rem] flex-col rounded-lg border border-slate-200 bg-white p-4 shadow-xl dark:border-slate-700 dark:bg-slate-800"
      >
        <div className="mb-3 flex items-center justify-between">
          <h2 className="text-sm font-semibold text-slate-800 dark:text-slate-100">Mock Servers</h2>
          <button
            type="button"
            onClick={onClose}
            className="rounded px-2 py-0.5 text-xs text-slate-500 hover:bg-slate-100 dark:text-slate-400 dark:hover:bg-slate-800"
          >
            Close
          </button>
        </div>

        <div className="flex min-h-0 flex-1 gap-3">
          <div className="w-44 shrink-0 overflow-y-auto border-r border-slate-100 pr-2 dark:border-slate-700">
            <button
              type="button"
              onClick={() => setSelectedId("new")}
              className="mb-1 flex w-full items-center gap-1.5 rounded px-2 py-1 text-left text-xs text-accent hover:bg-accent/10"
            >
              <Plus size={12} /> New
            </button>
            <button
              type="button"
              disabled={!collections?.length}
              onClick={() => setFromCollectionOpen(true)}
              className="mb-1 flex w-full items-center gap-1.5 rounded px-2 py-1 text-left text-xs text-accent hover:bg-accent/10 disabled:opacity-40"
            >
              <Plus size={12} /> From collection…
            </button>
            <button
              type="button"
              onClick={() => importFileRef.current?.click()}
              className="mb-2 flex w-full items-center gap-1.5 rounded px-2 py-1 text-left text-xs text-accent hover:bg-accent/10"
            >
              <Upload size={12} /> Import…
            </button>
            <input ref={importFileRef} type="file" accept=".json" className="hidden" onChange={onImportFile} />
            {importError && <p className="mb-2 px-2 text-xs text-red-500">{importError}</p>}
            {(servers ?? []).map((s) => (
              <button
                key={s.id}
                type="button"
                onClick={() => setSelectedId(s.id)}
                className={`flex w-full items-center gap-1.5 rounded px-2 py-1 text-left text-xs ${
                  selectedId === s.id
                    ? "bg-accent/10 text-accent"
                    : "text-slate-600 hover:bg-slate-100 dark:text-slate-300 dark:hover:bg-slate-700"
                }`}
              >
                <span
                  className={`h-1.5 w-1.5 shrink-0 rounded-full ${running.has(s.id) ? "bg-green-500" : "bg-slate-300 dark:bg-slate-600"}`}
                />
                <span className="min-w-0 flex-1 truncate">{s.name}</span>
                <span className="shrink-0 text-slate-400">:{s.port}</span>
              </button>
            ))}
            {servers?.length === 0 && <p className="px-2 py-1 text-xs text-slate-400">No mock servers yet.</p>}
          </div>

          <div className="min-h-0 flex-1 overflow-y-auto pr-1">
            {selectedId === "new" && (
              <MockServerEditor key="new" workspaceId={workspaceId} onDone={() => setSelectedId(null)} />
            )}
            {selected && (
              <MockServerEditor
                key={selected.id}
                workspaceId={workspaceId}
                server={selected}
                isRunning={running.has(selected.id)}
                onDone={() => setSelectedId(null)}
              />
            )}
            {!selectedId && (
              <div className="flex h-full items-center justify-center text-xs text-slate-400">
                Select a mock server or create a new one.
              </div>
            )}
          </div>
        </div>
      </div>

      {fromCollectionOpen && (
        <FromCollectionDialog
          workspaceId={workspaceId}
          collections={collections ?? []}
          onClose={() => setFromCollectionOpen(false)}
          onCreated={(id) => {
            setFromCollectionOpen(false);
            setSelectedId(id);
          }}
        />
      )}
    </div>
  );
}

function FromCollectionDialog({
  workspaceId,
  collections,
  onClose,
  onCreated,
}: {
  workspaceId: string;
  collections: { id: string; name: string }[];
  onClose: () => void;
  onCreated: (id: string) => void;
}) {
  const [collectionId, setCollectionId] = useState(collections[0]?.id ?? "");
  const [name, setName] = useState("");
  const [port, setPort] = useState(3001);
  const createFromCollection = useCreateMockServerFromCollection(workspaceId);

  return (
    <div className="fixed inset-0 z-[60] flex items-center justify-center bg-black/30" onClick={onClose}>
      <div
        onClick={(e) => e.stopPropagation()}
        className="flex w-80 flex-col gap-2 rounded-lg border border-slate-200 bg-white p-4 text-xs shadow-xl dark:border-slate-700 dark:bg-slate-800"
      >
        <h3 className="text-sm font-semibold text-slate-800 dark:text-slate-100">New mock server from collection</h3>
        <label className="flex flex-col gap-1">
          <span className="text-slate-500 dark:text-slate-400">Collection</span>
          <select
            value={collectionId}
            onChange={(e) => setCollectionId(e.target.value)}
            className="rounded border border-slate-200 px-2 py-1 dark:border-slate-700 dark:bg-slate-900"
          >
            {collections.map((c) => (
              <option key={c.id} value={c.id}>
                {c.name}
              </option>
            ))}
          </select>
        </label>
        <label className="flex flex-col gap-1">
          <span className="text-slate-500 dark:text-slate-400">Name</span>
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="My Mock Server"
            className="rounded border border-slate-200 px-2 py-1 dark:border-slate-700 dark:bg-slate-900"
          />
        </label>
        <label className="flex flex-col gap-1">
          <span className="text-slate-500 dark:text-slate-400">Port</span>
          <input
            type="number"
            value={port}
            onChange={(e) => setPort(Number(e.target.value))}
            className="w-24 rounded border border-slate-200 px-2 py-1 dark:border-slate-700 dark:bg-slate-900"
          />
        </label>
        <div className="mt-2 flex justify-end gap-2">
          <button type="button" onClick={onClose} className="rounded px-2 py-1 text-slate-500 hover:bg-slate-100 dark:hover:bg-slate-700">
            Cancel
          </button>
          <button
            type="button"
            disabled={!collectionId || !name.trim() || createFromCollection.isPending}
            onClick={() =>
              createFromCollection.mutate(
                { collectionId, name: name.trim(), port },
                { onSuccess: (server) => onCreated(server.id) },
              )
            }
            className="rounded bg-accent px-3 py-1 font-medium text-white disabled:opacity-50"
          >
            Create
          </button>
        </div>
      </div>
    </div>
  );
}

function MockServerEditor({
  workspaceId,
  server,
  isRunning,
  onDone,
}: {
  workspaceId: string;
  server?: MockServer;
  isRunning?: boolean;
  onDone: () => void;
}) {
  const [draft, setDraft] = useState<MockServerInput>(() =>
    server ? { name: server.name, port: server.port } : { name: "", port: 3001 },
  );
  const createServer = useCreateMockServer(workspaceId);
  const updateServer = useUpdateMockServer(workspaceId);
  const deleteServer = useDeleteMockServer(workspaceId);
  const startServer = useStartMockServer();
  const stopServer = useStopMockServer();
  const exportServer = useExportMockServer();
  const [startError, setStartError] = useState<string | null>(null);

  function saveServer() {
    if (server) {
      updateServer.mutate({ id: server.id, input: draft });
    } else {
      createServer.mutate(draft, { onSuccess: onDone });
    }
  }

  function remove() {
    if (!server) return;
    if (confirmDelete(`Delete mock server "${server.name}"? This can't be undone.`)) {
      deleteServer.mutate(server.id, { onSuccess: onDone });
    }
  }

  async function exportToFile() {
    if (!server) return;
    try {
      const content = await exportServer.mutateAsync(server.id);
      const path = await save({ defaultPath: `${server.name.replace(/\s+/g, "_")}.mock.json` });
      if (!path) return;
      await ipc.writeFileBytes(path, textToBase64(content));
    } catch (e) {
      console.error("failed to export mock server:", e);
    }
  }

  function toggleRunning() {
    if (!server) return;
    setStartError(null);
    if (isRunning) {
      stopServer.mutate(server.id);
    } else {
      startServer.mutate(server.id, {
        onError: (e) => setStartError(typeof e === "string" ? e : e instanceof Error ? e.message : String(e)),
      });
    }
  }

  const saving = createServer.isPending || updateServer.isPending;

  return (
    <div className="flex h-full flex-col gap-3">
      <div className="flex items-center gap-2">
        <input
          value={draft.name}
          onChange={(e) => setDraft({ ...draft, name: e.target.value })}
          placeholder="Mock server name"
          className="flex-1 rounded border border-slate-200 px-2 py-1 text-xs dark:border-slate-700 dark:bg-slate-900"
        />
        <input
          type="number"
          value={draft.port}
          onChange={(e) => setDraft({ ...draft, port: Number(e.target.value) })}
          className="w-20 rounded border border-slate-200 px-2 py-1 text-xs dark:border-slate-700 dark:bg-slate-900"
        />
        {server && (
          <button
            type="button"
            onClick={toggleRunning}
            disabled={startServer.isPending || stopServer.isPending}
            className={
              "flex items-center gap-1 rounded-md px-2 py-1 text-xs font-medium disabled:opacity-50 " +
              (isRunning
                ? "bg-red-50 text-red-600 hover:bg-red-100 dark:bg-red-950/30 dark:hover:bg-red-950/60"
                : "bg-green-50 text-green-700 hover:bg-green-100 dark:bg-green-950/30 dark:hover:bg-green-950/60")
            }
          >
            {isRunning ? <Square size={11} /> : <Play size={11} />}
            {isRunning ? "Stop" : "Start"}
          </button>
        )}
      </div>
      {startError && <p className="text-xs text-red-500">{startError}</p>}

      <div className="flex items-center gap-2">
        <button
          type="button"
          disabled={saving}
          onClick={saveServer}
          className="rounded-md bg-accent px-3 py-1.5 text-xs font-medium text-white hover:bg-accent-hover disabled:opacity-50"
        >
          {saving ? "Saving…" : server ? "Save" : "Create"}
        </button>
        {server && (
          <button
            type="button"
            onClick={exportToFile}
            disabled={exportServer.isPending}
            className="flex items-center gap-1 rounded-md px-2 py-1 text-xs text-slate-500 hover:bg-slate-100 disabled:opacity-50 dark:text-slate-400 dark:hover:bg-slate-700"
          >
            <Download size={12} /> Export
          </button>
        )}
        {server && (
          <button
            type="button"
            onClick={remove}
            className="flex items-center gap-1 rounded-md px-2 py-1 text-xs text-red-500 hover:bg-red-50 dark:hover:bg-red-900/30"
          >
            <Trash2 size={12} /> Delete
          </button>
        )}
      </div>

      {server && <RulesTable mockServerId={server.id} />}
    </div>
  );
}

/** `HeaderEntry`/`MockMatcher` (name/value/enabled) ↔ `KeyValueEditor`'s
 * `Pair` (key/value/enabled) — same mapping convention `RequestBuilder` uses. */
const headersToPairs = (headers: HeaderEntry[]): Pair[] =>
  headers.map((h) => ({ key: h.name, value: h.value, enabled: h.enabled }));
const pairsToHeaders = (pairs: Pair[]): HeaderEntry[] =>
  pairs.map((p) => ({ name: p.key, value: p.value, enabled: p.enabled }));
const matchersToPairs = (matchers: MockMatcher[]): Pair[] =>
  matchers.map((m) => ({ key: m.name, value: m.value, enabled: m.enabled }));
const pairsToMatchers = (pairs: Pair[]): MockMatcher[] =>
  pairs.map((p) => ({ name: p.key, value: p.value, enabled: p.enabled }));

function matcherCount(rule: MockRule): number {
  return rule.queryMatchers.length + rule.headerMatchers.length + (rule.bodyMatcher ? 1 : 0);
}

function RulesTable({ mockServerId }: { mockServerId: string }) {
  const { data: rules } = useMockRules(mockServerId);
  const createRule = useCreateMockRule(mockServerId);
  const updateRule = useUpdateMockRule(mockServerId);
  const deleteRule = useDeleteMockRule(mockServerId);
  const [headersOpenId, setHeadersOpenId] = useState<string | null>(null);
  const [matchOpenId, setMatchOpenId] = useState<string | null>(null);

  function addRule() {
    createRule.mutate(emptyRuleInput(rules?.length ?? 0));
  }

  function patchRule(rule: MockRule, patch: Partial<MockRuleInput>) {
    updateRule.mutate({
      id: rule.id,
      input: {
        method: rule.method,
        pathPattern: rule.pathPattern,
        status: rule.status,
        headers: rule.headers,
        body: rule.body,
        delayMs: rule.delayMs,
        sortOrder: rule.sortOrder,
        queryMatchers: rule.queryMatchers,
        headerMatchers: rule.headerMatchers,
        bodyMatcher: rule.bodyMatcher,
        ...patch,
      },
    });
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-1.5">
      <div className="flex items-center justify-between">
        <span className="text-xs font-semibold tracking-wide text-slate-400 uppercase dark:text-slate-500">Rules</span>
        <button
          type="button"
          onClick={addRule}
          className="flex items-center gap-1 rounded px-1.5 py-0.5 text-xs text-accent hover:bg-accent/10"
        >
          <Plus size={11} /> Add rule
        </button>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto rounded-md border border-slate-200 dark:border-slate-700">
        {(rules ?? []).length === 0 && <p className="p-2 text-xs text-slate-400">No rules yet — first match wins.</p>}
        {(rules ?? []).map((rule) => (
          <div key={rule.id} className="border-b border-slate-100 last:border-b-0 dark:border-slate-800">
          <div className="flex items-center gap-1.5 p-1.5 text-xs">
            <select
              value={rule.method ?? "ANY"}
              onChange={(e) => patchRule(rule, { method: e.target.value === "ANY" ? null : e.target.value })}
              className="w-20 shrink-0 rounded border border-slate-200 bg-transparent px-1 py-0.5 dark:border-slate-700"
            >
              {METHODS.map((m) => (
                <option key={m} value={m}>
                  {m}
                </option>
              ))}
            </select>
            <input
              defaultValue={rule.pathPattern}
              onBlur={(e) => patchRule(rule, { pathPattern: e.target.value })}
              placeholder="/users/:id"
              className="min-w-0 flex-1 rounded border border-slate-200 bg-transparent px-1 py-0.5 font-mono dark:border-slate-700"
            />
            <input
              type="number"
              defaultValue={rule.status}
              onBlur={(e) => patchRule(rule, { status: Number(e.target.value) })}
              className="w-14 shrink-0 rounded border border-slate-200 bg-transparent px-1 py-0.5 dark:border-slate-700"
              title="Status"
            />
            <input
              type="number"
              defaultValue={rule.delayMs}
              onBlur={(e) => patchRule(rule, { delayMs: Number(e.target.value) })}
              className="w-16 shrink-0 rounded border border-slate-200 bg-transparent px-1 py-0.5 dark:border-slate-700"
              title="Delay (ms)"
            />
            <input
              defaultValue={rule.body}
              onBlur={(e) => patchRule(rule, { body: e.target.value })}
              placeholder="Response body"
              className="min-w-0 flex-[2] rounded border border-slate-200 bg-transparent px-1 py-0.5 font-mono dark:border-slate-700"
            />
            <button
              type="button"
              onClick={() => setHeadersOpenId(headersOpenId === rule.id ? null : rule.id)}
              title="Response headers"
              className="flex shrink-0 items-center gap-0.5 rounded px-1 py-0.5 text-slate-400 hover:bg-slate-100 hover:text-slate-600 dark:hover:bg-slate-700"
            >
              {headersOpenId === rule.id ? <ChevronDown size={11} /> : <ChevronRight size={11} />}
              Headers{rule.headers.length > 0 ? ` (${rule.headers.length})` : ""}
            </button>
            <button
              type="button"
              onClick={() => setMatchOpenId(matchOpenId === rule.id ? null : rule.id)}
              title="Extra match conditions (query/header/body) — narrows this rule beyond method+path"
              className="flex shrink-0 items-center gap-0.5 rounded px-1 py-0.5 text-slate-400 hover:bg-slate-100 hover:text-slate-600 dark:hover:bg-slate-700"
            >
              {matchOpenId === rule.id ? <ChevronDown size={11} /> : <ChevronRight size={11} />}
              Match{matcherCount(rule) > 0 ? ` (${matcherCount(rule)})` : ""}
            </button>
            <button
              type="button"
              onClick={() => deleteRule.mutate(rule.id)}
              className="shrink-0 rounded p-1 text-slate-400 hover:bg-red-100 hover:text-red-600 dark:hover:bg-red-900/40"
            >
              <Trash2 size={12} />
            </button>
          </div>
          {headersOpenId === rule.id && (
            <div className="px-1.5 pb-1.5">
              <KeyValueEditor
                rows={headersToPairs(rule.headers)}
                onChange={(pairs) => patchRule(rule, { headers: pairsToHeaders(pairs) })}
                keyPlaceholder="Header name"
                valuePlaceholder="Header value"
              />
            </div>
          )}
          {matchOpenId === rule.id && (
            <RuleMatchersEditor
              rule={rule}
              onChange={(patch) => patchRule(rule, patch)}
            />
          )}
          </div>
        ))}
      </div>
    </div>
  );
}

const BODY_MATCH_MODES: { value: BodyMatchMode; label: string }[] = [
  { value: "contains", label: "Body contains" },
  { value: "jsonEquals", label: "JSON field equals" },
];

/** Extra request-match conditions on top of method+path: query params and
 * headers the incoming request must carry, plus an optional body check —
 * lets two rules share the same method+path and be picked apart by request
 * content (first match in the list still wins). */
function RuleMatchersEditor({
  rule,
  onChange,
}: {
  rule: MockRule;
  onChange: (patch: Partial<MockRuleInput>) => void;
}) {
  const bodyMatcher = rule.bodyMatcher;

  function setBodyMatcherEnabled(enabled: boolean) {
    onChange({ bodyMatcher: enabled ? { mode: "contains", jsonPath: "", value: "" } : null });
  }

  function patchBodyMatcher(patch: Partial<BodyMatcher>) {
    if (!bodyMatcher) return;
    onChange({ bodyMatcher: { ...bodyMatcher, ...patch } });
  }

  return (
    <div className="flex flex-col gap-2 px-1.5 pb-2 text-xs">
      <div>
        <span className="mb-1 block text-slate-400 dark:text-slate-500">Query params must match</span>
        <KeyValueEditor
          rows={matchersToPairs(rule.queryMatchers)}
          onChange={(pairs) => onChange({ queryMatchers: pairsToMatchers(pairs) })}
          keyPlaceholder="Param name"
          valuePlaceholder="Value"
        />
      </div>
      <div>
        <span className="mb-1 block text-slate-400 dark:text-slate-500">Request headers must match</span>
        <KeyValueEditor
          rows={matchersToPairs(rule.headerMatchers)}
          onChange={(pairs) => onChange({ headerMatchers: pairsToMatchers(pairs) })}
          keyPlaceholder="Header name"
          valuePlaceholder="Value"
        />
      </div>
      <div className="flex items-center gap-1.5">
        <input type="checkbox" checked={bodyMatcher !== null} onChange={(e) => setBodyMatcherEnabled(e.target.checked)} />
        <span className="text-slate-400 dark:text-slate-500">Request body must match</span>
        {bodyMatcher && (
          <>
            <select
              value={bodyMatcher.mode}
              onChange={(e) => patchBodyMatcher({ mode: e.target.value as BodyMatchMode })}
              className="rounded border border-slate-200 bg-transparent px-1 py-0.5 dark:border-slate-700"
            >
              {BODY_MATCH_MODES.map((m) => (
                <option key={m.value} value={m.value}>
                  {m.label}
                </option>
              ))}
            </select>
            {bodyMatcher.mode === "jsonEquals" && (
              <input
                defaultValue={bodyMatcher.jsonPath}
                onBlur={(e) => patchBodyMatcher({ jsonPath: e.target.value })}
                placeholder="user.id"
                className="w-24 rounded border border-slate-200 bg-transparent px-1 py-0.5 font-mono dark:border-slate-700"
              />
            )}
            <input
              defaultValue={bodyMatcher.value}
              onBlur={(e) => patchBodyMatcher({ value: e.target.value })}
              placeholder="Expected value"
              className="min-w-0 flex-1 rounded border border-slate-200 bg-transparent px-1 py-0.5 font-mono dark:border-slate-700"
            />
          </>
        )}
      </div>
    </div>
  );
}
