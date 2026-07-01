//! Mock server manager: create/edit/delete workspace-scoped mock servers and
//! their method+path -> canned-response rules, start/stop each server's
//! live loopback socket. Same modal shell as `PluginManagerDialog`.

import { useState } from "react";
import { Play, Plus, Square, Trash2 } from "lucide-react";
import { useCollections } from "../collections/hooks";
import type { MockRule, MockRuleInput, MockServer, MockServerInput } from "../../lib/types";
import {
  useCreateMockRule,
  useCreateMockServer,
  useCreateMockServerFromCollection,
  useDeleteMockRule,
  useDeleteMockServer,
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
  return { method: "GET", pathPattern: "/", status: 200, headers: [], body: "", delayMs: 0, sortOrder };
}

export function MockServerManagerDialog({ workspaceId, onClose }: { workspaceId: string; onClose: () => void }) {
  const { data: servers } = useMockServers(workspaceId);
  const { data: runningIds } = useRunningMockServerIds();
  const { data: collections } = useCollections(workspaceId);
  const [selectedId, setSelectedId] = useState<string | "new" | null>(null);
  const [fromCollectionOpen, setFromCollectionOpen] = useState(false);

  const selected = selectedId && selectedId !== "new" ? servers?.find((s) => s.id === selectedId) : undefined;
  const running = new Set(runningIds ?? []);

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
              className="mb-2 flex w-full items-center gap-1.5 rounded px-2 py-1 text-left text-xs text-accent hover:bg-accent/10 disabled:opacity-40"
            >
              <Plus size={12} /> From collection…
            </button>
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
  const [startError, setStartError] = useState<string | null>(null);

  function save() {
    if (server) {
      updateServer.mutate({ id: server.id, input: draft });
    } else {
      createServer.mutate(draft, { onSuccess: onDone });
    }
  }

  function remove() {
    if (!server) return;
    if (window.confirm(`Delete mock server "${server.name}"? This can't be undone.`)) {
      deleteServer.mutate(server.id, { onSuccess: onDone });
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
          onClick={save}
          className="rounded-md bg-accent px-3 py-1.5 text-xs font-medium text-white hover:bg-accent-hover disabled:opacity-50"
        >
          {saving ? "Saving…" : server ? "Save" : "Create"}
        </button>
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

function RulesTable({ mockServerId }: { mockServerId: string }) {
  const { data: rules } = useMockRules(mockServerId);
  const createRule = useCreateMockRule(mockServerId);
  const updateRule = useUpdateMockRule(mockServerId);
  const deleteRule = useDeleteMockRule(mockServerId);

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
          <div
            key={rule.id}
            className="flex items-center gap-1.5 border-b border-slate-100 p-1.5 text-xs last:border-b-0 dark:border-slate-800"
          >
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
              onClick={() => deleteRule.mutate(rule.id)}
              className="shrink-0 rounded p-1 text-slate-400 hover:bg-red-100 hover:text-red-600 dark:hover:bg-red-900/40"
            >
              <Trash2 size={12} />
            </button>
          </div>
        ))}
      </div>
    </div>
  );
}
