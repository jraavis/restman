//! Plugin manager: create/edit/delete workspace-scoped JS plugins (custom
//! codegen languages, custom import/export formats) and test-run raw source
//! against a sample input before saving. Same modal shell as
//! `WorkspaceSettingsDialog`/`CookieJarDialog`.

import { useState } from "react";
import { Play, Plus, Trash2 } from "lucide-react";
import { LazyCodeEditor } from "../../components/LazyCodeEditor";
import { defaultRequest } from "../../lib/http";
import { ipc } from "../../lib/ipc";
import {
  defaultCodegenOptions,
  defaultRequestAuth,
  emptyPluginInput,
  type ImportedNode,
  type Plugin,
  type PluginInput,
  type PluginKind,
} from "../../lib/types";
import { useCreatePlugin, useDeletePlugin, usePlugins, useUpdatePlugin } from "./hooks";

const KIND_LABELS: Record<PluginKind, string> = {
  codegen: "Codegen",
  import: "Import",
  export: "Export",
};

const ENTRY_POINT_HINTS: Record<PluginKind, string> = {
  codegen: "Must define: function generate(request, options) → string",
  import: 'Must define: function parse(content) → { root, warnings? }',
  export: "Must define: function exportCollection(node) → string",
};

/** Minimal `ImportedNode` for an export plugin's "test run" — not real
 * workspace data, just enough shape for the plugin to render something. */
function sampleImportedNode(): ImportedNode {
  return {
    name: "Sample Collection",
    description: null,
    auth: { type: "none" },
    requests: [
      {
        name: "Get thing",
        method: "GET",
        url: "https://api.example.com/items",
        headers: [],
        query: [],
        body: { mode: "none" },
        options: defaultRequest().options,
        auth: defaultRequestAuth(),
        preRequestScript: "",
        postResponseScript: "",
      },
    ],
    children: [],
  };
}

export function PluginManagerDialog({ workspaceId, onClose }: { workspaceId: string; onClose: () => void }) {
  const [kindFilter, setKindFilter] = useState<PluginKind>("codegen");
  const { data: plugins } = usePlugins(workspaceId, kindFilter);
  const [selectedId, setSelectedId] = useState<string | "new" | null>(null);

  const selected = selectedId && selectedId !== "new" ? plugins?.find((p) => p.id === selectedId) : undefined;

  function selectKind(k: PluginKind) {
    setKindFilter(k);
    setSelectedId(null);
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/30" onClick={onClose}>
      <div
        onClick={(e) => e.stopPropagation()}
        className="flex h-[34rem] max-h-[85vh] w-[44rem] flex-col rounded-lg border border-slate-200 bg-white p-4 shadow-xl dark:border-slate-700 dark:bg-slate-800"
      >
        <div className="mb-3 flex items-center justify-between">
          <h2 className="text-sm font-semibold text-slate-800 dark:text-slate-100">Plugins</h2>
          <button
            type="button"
            onClick={onClose}
            className="rounded px-2 py-0.5 text-xs text-slate-500 hover:bg-slate-100 dark:text-slate-400 dark:hover:bg-slate-800"
          >
            Close
          </button>
        </div>

        <div className="mb-3 flex w-fit rounded-lg border border-slate-200 text-xs dark:border-slate-700">
          {(["codegen", "import", "export"] as PluginKind[]).map((k) => (
            <button
              key={k}
              type="button"
              onClick={() => selectKind(k)}
              className={`px-3 py-1 ${kindFilter === k ? "bg-accent text-white" : "text-slate-500 dark:text-slate-400"}`}
            >
              {KIND_LABELS[k]}
            </button>
          ))}
        </div>

        <div className="flex min-h-0 flex-1 gap-3">
          <div className="w-40 shrink-0 overflow-y-auto border-r border-slate-100 pr-2 dark:border-slate-700">
            <button
              type="button"
              onClick={() => setSelectedId("new")}
              className="mb-1 flex w-full items-center gap-1.5 rounded px-2 py-1 text-left text-xs text-accent hover:bg-accent/10"
            >
              <Plus size={12} /> New
            </button>
            {(plugins ?? []).map((p) => (
              <button
                key={p.id}
                type="button"
                onClick={() => setSelectedId(p.id)}
                className={`flex w-full flex-col items-start rounded px-2 py-1 text-left text-xs ${
                  selectedId === p.id
                    ? "bg-accent/10 text-accent"
                    : "text-slate-600 hover:bg-slate-100 dark:text-slate-300 dark:hover:bg-slate-700"
                }`}
              >
                <span className="truncate font-medium">{p.name}</span>
                {!p.enabled && <span className="text-[10px] uppercase text-slate-400">disabled</span>}
              </button>
            ))}
            {plugins?.length === 0 && <p className="px-2 py-1 text-xs text-slate-400">No plugins yet.</p>}
          </div>

          <div className="min-h-0 flex-1 overflow-y-auto pr-1">
            {selectedId === "new" && (
              <PluginEditor
                key="new"
                workspaceId={workspaceId}
                defaultKind={kindFilter}
                onDone={() => setSelectedId(null)}
              />
            )}
            {selected && (
              <PluginEditor
                key={selected.id}
                workspaceId={workspaceId}
                plugin={selected}
                onDone={() => setSelectedId(null)}
              />
            )}
            {!selectedId && (
              <div className="flex h-full items-center justify-center text-xs text-slate-400">
                Select a plugin or create a new one.
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

function PluginEditor({
  workspaceId,
  plugin,
  defaultKind,
  onDone,
}: {
  workspaceId: string;
  plugin?: Plugin;
  defaultKind?: PluginKind;
  onDone: () => void;
}) {
  const [draft, setDraft] = useState<PluginInput>(() =>
    plugin
      ? {
          name: plugin.name,
          kind: plugin.kind,
          languageLabel: plugin.languageLabel,
          source: plugin.source,
          enabled: plugin.enabled,
        }
      : emptyPluginInput(defaultKind ?? "codegen"),
  );
  const [testResult, setTestResult] = useState<string | null>(null);
  const [testError, setTestError] = useState<string | null>(null);
  const [testing, setTesting] = useState(false);

  const createPlugin = useCreatePlugin(workspaceId);
  const updatePlugin = useUpdatePlugin(workspaceId);
  const deletePlugin = useDeletePlugin(workspaceId);

  function save() {
    if (plugin) {
      updatePlugin.mutate({ id: plugin.id, input: draft }, { onSuccess: onDone });
    } else {
      createPlugin.mutate(draft, { onSuccess: onDone });
    }
  }

  function remove() {
    if (!plugin) return;
    if (window.confirm(`Delete plugin "${plugin.name}"? This can't be undone.`)) {
      deletePlugin.mutate(plugin.id, { onSuccess: onDone });
    }
  }

  async function runTest() {
    setTesting(true);
    setTestError(null);
    setTestResult(null);
    try {
      if (draft.kind === "codegen") {
        setTestResult(await ipc.previewPluginCodegen(draft.source, defaultRequest(), defaultCodegenOptions()));
      } else if (draft.kind === "import") {
        const preview = await ipc.previewPluginImport(draft.source, "sample content");
        setTestResult(JSON.stringify(preview, null, 2));
      } else {
        setTestResult(await ipc.previewPluginExport(draft.source, sampleImportedNode()));
      }
    } catch (e) {
      setTestError(typeof e === "string" ? e : String(e));
    } finally {
      setTesting(false);
    }
  }

  const saving = createPlugin.isPending || updatePlugin.isPending;

  return (
    <div className="flex h-full flex-col gap-2">
      <div className="flex items-center gap-2">
        <input
          value={draft.name}
          onChange={(e) => setDraft({ ...draft, name: e.target.value })}
          placeholder="Plugin name"
          className="flex-1 rounded border border-slate-200 px-2 py-1 text-xs dark:border-slate-700 dark:bg-slate-900"
        />
        <input
          value={draft.languageLabel}
          onChange={(e) => setDraft({ ...draft, languageLabel: e.target.value })}
          placeholder={draft.kind === "codegen" ? "Language label" : "Format label"}
          className="w-36 rounded border border-slate-200 px-2 py-1 text-xs dark:border-slate-700 dark:bg-slate-900"
        />
        <label className="flex items-center gap-1 text-xs text-slate-500 dark:text-slate-400">
          <input
            type="checkbox"
            checked={draft.enabled}
            onChange={(e) => setDraft({ ...draft, enabled: e.target.checked })}
          />
          Enabled
        </label>
      </div>

      <div className="min-h-0 flex-1 overflow-hidden rounded border border-slate-200 dark:border-slate-700">
        <LazyCodeEditor
          value={draft.source}
          onChange={(v) => setDraft({ ...draft, source: v ?? "" })}
          language="javascript"
          options={{ minimap: { enabled: false }, fontSize: 12, scrollBeyondLastLine: false, wordWrap: "on" }}
        />
      </div>

      <p className="text-[11px] text-slate-400">{ENTRY_POINT_HINTS[draft.kind]}</p>

      <div className="flex items-center gap-2">
        <button
          type="button"
          onClick={() => void runTest()}
          disabled={testing}
          className="flex items-center gap-1 rounded-md border border-slate-200 px-2 py-1 text-xs text-slate-600 hover:bg-slate-100 disabled:opacity-50 dark:border-slate-700 dark:text-slate-300 dark:hover:bg-slate-700"
        >
          <Play size={12} /> {testing ? "Running…" : "Test run"}
        </button>
        {plugin && (
          <button
            type="button"
            onClick={remove}
            className="flex items-center gap-1 rounded-md px-2 py-1 text-xs text-red-500 hover:bg-red-50 dark:hover:bg-red-900/30"
          >
            <Trash2 size={12} /> Delete
          </button>
        )}
        <button
          type="button"
          disabled={saving}
          onClick={save}
          className="ml-auto rounded-md bg-accent px-3 py-1.5 text-xs font-medium text-white hover:bg-accent-hover disabled:opacity-50"
        >
          {saving ? "Saving…" : plugin ? "Save" : "Create"}
        </button>
      </div>

      {testError && (
        <div className="max-h-24 overflow-auto rounded border border-red-200 bg-red-50 p-2 text-xs text-red-600 dark:border-red-900 dark:bg-red-950/30 dark:text-red-400">
          {testError}
        </div>
      )}
      {testResult !== null && !testError && (
        <pre className="max-h-32 overflow-auto rounded border border-slate-200 bg-slate-50 p-2 text-xs dark:border-slate-700 dark:bg-slate-900">
          {testResult}
        </pre>
      )}
    </div>
  );
}
