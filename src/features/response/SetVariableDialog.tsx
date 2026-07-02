//! Dialog to save a selected response value as an environment/workspace variable.

import { useMemo, useState } from "react";
import { Lock } from "lucide-react";
import {
  useActiveEnvironment,
  useCreateVariable,
  useUpdateVariable,
  useVariables,
} from "../environments/hooks";
import { useRequestStore } from "../../stores/requestStore";
import type { VarScope, Variable } from "../../lib/types";
import { pathToPmExpression, type JsonPath } from "./jsonPath";

export function SetVariableDialog({
  workspaceId,
  collectionId,
  value,
  jsonPath,
  onClose,
}: {
  workspaceId: string;
  collectionId: string | null;
  value: string;
  jsonPath: JsonPath | null;
  onClose: () => void;
}) {
  const { data: activeEnv } = useActiveEnvironment(workspaceId);
  const defaultScope: VarScope = activeEnv
    ? { kind: "environment", id: activeEnv.id }
    : { kind: "workspace", id: workspaceId };

  const [scopeKind, setScopeKind] = useState<VarScope["kind"]>(defaultScope.kind);
  const [selectedVarId, setSelectedVarId] = useState<string>("");
  const [newKey, setNewKey] = useState("");
  const [autoExtract, setAutoExtract] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const scope: VarScope = useMemo(() => {
    switch (scopeKind) {
      case "global":
        return { kind: "global" };
      case "workspace":
        return { kind: "workspace", id: workspaceId };
      case "collection":
        return { kind: "collection", id: collectionId ?? "" };
      case "environment":
        return { kind: "environment", id: activeEnv?.id ?? "" };
    }
  }, [scopeKind, workspaceId, collectionId, activeEnv?.id]);

  const scopeEnabled =
    scope.kind === "global" ||
    (scope.kind === "workspace" && !!workspaceId) ||
    (scope.kind === "collection" && !!collectionId) ||
    (scope.kind === "environment" && !!activeEnv?.id);

  const { data: variables } = useVariables(scope, scopeEnabled);
  const createVariable = useCreateVariable(scope);
  const updateVariable = useUpdateVariable(scope);
  const setPostResponseScript = useRequestStore((s) => s.setPostResponseScript);
  const postResponseScript = useRequestStore((s) => s.postResponseScript);

  const selectedVar: Variable | undefined = variables?.find((v) => v.id === selectedVarId);
  const key = selectedVarId ? (selectedVar?.key ?? "") : newKey.trim();
  const isSecret = selectedVar?.isSecret ?? false;
  const canAutoExtract = jsonPath != null && jsonPath.length > 0;

  async function handleSave() {
    if (!key) {
      setError("Enter a variable name.");
      return;
    }
    if (!scopeEnabled) {
      setError("Selected scope is not available.");
      return;
    }
    setSaving(true);
    setError(null);
    try {
      const input = {
        key,
        value,
        varType: "string" as const,
        isSecret,
        enabled: true,
      };
      if (selectedVar) {
        await updateVariable.mutateAsync({ id: selectedVar.id, input });
      } else {
        await createVariable.mutateAsync(input);
      }

      if (autoExtract && canAutoExtract) {
        const expr = pathToPmExpression(jsonPath!);
        const line = `pm.environment.set("${key}", ${expr});`;
        const existing = postResponseScript.trim();
        if (!existing.includes(line)) {
          setPostResponseScript(existing ? `${existing}\n${line}` : line);
        }
      }

      onClose();
    } catch (e) {
      setError(typeof e === "string" ? e : String(e));
    } finally {
      setSaving(false);
    }
  }

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/30"
      onClick={onClose}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        className="w-96 rounded-lg border border-slate-200 bg-white p-4 shadow-xl dark:border-slate-700 dark:bg-slate-800"
      >
        <h2 className="mb-3 text-sm font-semibold text-slate-800 dark:text-slate-100">
          Set as variable value
        </h2>

        <label className="mb-3 block text-xs">
          <span className="mb-1 block text-slate-500 dark:text-slate-400">Scope</span>
          <select
            value={scopeKind}
            onChange={(e) => {
              setScopeKind(e.target.value as VarScope["kind"]);
              setSelectedVarId("");
            }}
            className="w-full rounded border border-slate-200 bg-white px-2 py-1.5 text-sm dark:border-slate-600 dark:bg-slate-900"
          >
            {activeEnv && <option value="environment">Active environment</option>}
            <option value="workspace">Workspace</option>
            <option value="global">Global</option>
            {collectionId && <option value="collection">Collection</option>}
          </select>
        </label>

        <label className="mb-3 block text-xs">
          <span className="mb-1 block text-slate-500 dark:text-slate-400">Variable</span>
          <select
            value={selectedVarId}
            onChange={(e) => setSelectedVarId(e.target.value)}
            className="mb-1.5 w-full rounded border border-slate-200 bg-white px-2 py-1.5 text-sm dark:border-slate-600 dark:bg-slate-900"
          >
            <option value="">— New variable —</option>
            {variables?.map((v) => (
              <option key={v.id} value={v.id}>
                {v.key}
                {v.isSecret ? " (secret)" : ""}
              </option>
            ))}
          </select>
          {!selectedVarId && (
            <input
              type="text"
              value={newKey}
              onChange={(e) => setNewKey(e.target.value)}
              placeholder="Variable name"
              className="w-full rounded border border-slate-200 bg-white px-2 py-1.5 text-sm dark:border-slate-600 dark:bg-slate-900"
            />
          )}
        </label>

        <label className="mb-3 block text-xs">
          <span className="mb-1 block text-slate-500 dark:text-slate-400">Value</span>
          <pre className="max-h-24 overflow-auto rounded border border-slate-100 bg-slate-50 p-2 font-mono text-xs text-slate-700 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-200">
            {value}
          </pre>
        </label>

        {isSecret && (
          <p className="mb-3 flex items-center gap-1 text-xs text-amber-600 dark:text-amber-400">
            <Lock size={12} />
            Updating a secret variable — the stored value will be replaced.
          </p>
        )}

        {canAutoExtract && (
          <label className="mb-3 flex items-center gap-2 text-xs text-slate-600 dark:text-slate-300">
            <input
              type="checkbox"
              checked={autoExtract}
              onChange={(e) => setAutoExtract(e.target.checked)}
            />
            Update on every response
          </label>
        )}

        {error && <p className="mb-3 text-xs text-red-500">{error}</p>}

        <div className="flex justify-end gap-2">
          <button
            type="button"
            onClick={onClose}
            className="rounded px-3 py-1.5 text-xs text-slate-500 hover:bg-slate-100 dark:hover:bg-slate-700"
          >
            Cancel
          </button>
          <button
            type="button"
            disabled={saving}
            onClick={() => void handleSave()}
            className="rounded bg-accent px-3 py-1.5 text-xs font-medium text-white hover:bg-accent/90 disabled:opacity-50"
          >
            {saving ? "Saving…" : "Save"}
          </button>
        </div>
      </div>
    </div>
  );
}