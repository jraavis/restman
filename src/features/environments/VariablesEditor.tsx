//! Compact key/value/type/secret table for one variable scope — reused for
//! the Global section, the workspace section, and each environment's
//! expanded row in EnvironmentsPanel. Secret values arrive pre-masked from
//! the IPC layer (see `commands::variables`); typing a new value into an
//! existing secret row replaces it, leaving it untouched round-trips the
//! mask sentinel and the backend keeps the stored value as-is.

import { useRef, useState } from "react";
import { Eye, EyeOff, Lock, Plus, Trash2 } from "lucide-react";
import { useCreateVariable, useDeleteVariable, useUpdateVariable, useVariables } from "./hooks";
import type { VarScope, VarType, Variable, VariableInput } from "../../lib/types";

const VAR_TYPES: VarType[] = ["string", "number", "boolean", "json"];

export function VariablesEditor({ scope }: { scope: VarScope }) {
  const { data: variables, isLoading } = useVariables(scope);
  const createVariable = useCreateVariable(scope);
  const updateVariable = useUpdateVariable(scope);
  const deleteVariable = useDeleteVariable(scope);
  const [adding, setAdding] = useState(false);

  if (isLoading) {
    return <p className="px-1.5 py-1 text-xs text-slate-400">Loading…</p>;
  }

  return (
    <div className="flex flex-col gap-0.5">
      {variables?.length === 0 && !adding && (
        <p className="px-1.5 py-1 text-xs text-slate-400">No variables yet.</p>
      )}
      {variables?.map((v) => (
        <VariableRow
          key={v.id}
          variable={v}
          onChange={(input) => updateVariable.mutate({ id: v.id, input })}
          onDelete={() => deleteVariable.mutate(v.id)}
        />
      ))}
      {adding ? (
        <NewVariableRow
          onCommit={(input) => {
            createVariable.mutate(input);
            setAdding(false);
          }}
          onCancel={() => setAdding(false)}
        />
      ) : (
        <button
          type="button"
          onClick={() => setAdding(true)}
          className="flex items-center gap-1 self-start rounded px-1.5 py-1 text-xs text-slate-400 hover:bg-slate-100 hover:text-accent dark:hover:bg-slate-800"
        >
          <Plus size={12} /> Add variable
        </button>
      )}
    </div>
  );
}

function VariableRow({
  variable,
  onChange,
  onDelete,
}: {
  variable: Variable;
  onChange: (input: VariableInput) => void;
  onDelete: () => void;
}) {
  const [key, setKey] = useState(variable.key);
  const [value, setValue] = useState(variable.value);
  const [revealed, setRevealed] = useState(false);

  function toInput(overrides: Partial<VariableInput> = {}): VariableInput {
    return {
      key,
      value,
      varType: variable.varType,
      isSecret: variable.isSecret,
      enabled: variable.enabled,
      ...overrides,
    };
  }

  function commitKey() {
    const trimmed = key.trim();
    if (trimmed && trimmed !== variable.key) onChange(toInput({ key: trimmed }));
    else setKey(variable.key);
  }

  function commitValue() {
    if (value !== variable.value) onChange(toInput());
  }

  return (
    <div className="flex items-center gap-1 px-1.5 py-0.5">
      <input
        type="checkbox"
        checked={variable.enabled}
        onChange={(e) => onChange(toInput({ enabled: e.target.checked }))}
        title={variable.enabled ? "Enabled" : "Disabled"}
        className="shrink-0"
      />
      <input
        value={key}
        onChange={(e) => setKey(e.target.value)}
        onBlur={commitKey}
        placeholder="key"
        className="min-w-0 flex-1 rounded border border-transparent bg-transparent px-1 py-0.5 text-xs font-mono hover:border-slate-200 focus:border-accent/40 focus:outline-none dark:hover:border-slate-700"
      />
      <select
        value={variable.varType}
        onChange={(e) => onChange(toInput({ varType: e.target.value as VarType }))}
        className="shrink-0 rounded border border-transparent bg-transparent py-0.5 text-[11px] text-slate-400 hover:border-slate-200 focus:border-accent/40 focus:outline-none dark:hover:border-slate-700"
      >
        {VAR_TYPES.map((t) => (
          <option key={t} value={t}>
            {t}
          </option>
        ))}
      </select>
      {variable.varType === "boolean" ? (
        <select
          value={value}
          onChange={(e) => {
            setValue(e.target.value);
            onChange(toInput({ value: e.target.value }));
          }}
          className="min-w-0 flex-1 rounded border border-transparent bg-transparent px-1 py-0.5 text-xs hover:border-slate-200 focus:border-accent/40 focus:outline-none dark:hover:border-slate-700"
        >
          <option value="true">true</option>
          <option value="false">false</option>
        </select>
      ) : (
        <input
          value={value}
          onChange={(e) => setValue(e.target.value)}
          onBlur={commitValue}
          type={variable.isSecret && !revealed ? "password" : "text"}
          placeholder="value"
          className="min-w-0 flex-1 rounded border border-transparent bg-transparent px-1 py-0.5 text-xs font-mono hover:border-slate-200 focus:border-accent/40 focus:outline-none dark:hover:border-slate-700"
        />
      )}
      {variable.isSecret && variable.varType !== "boolean" && (
        <button
          type="button"
          onClick={() => setRevealed((r) => !r)}
          title={revealed ? "Hide value" : "Reveal value"}
          className="shrink-0 rounded p-0.5 text-slate-400 hover:bg-slate-200 hover:text-slate-700 dark:hover:bg-slate-700"
        >
          {revealed ? <EyeOff size={12} /> : <Eye size={12} />}
        </button>
      )}
      <button
        type="button"
        onClick={() => onChange(toInput({ isSecret: !variable.isSecret }))}
        title={variable.isSecret ? "Secret — click to unmark" : "Mark as secret"}
        className={
          "shrink-0 rounded p-0.5 hover:bg-slate-200 dark:hover:bg-slate-700 " +
          (variable.isSecret ? "text-amber-500" : "text-slate-300 dark:text-slate-600")
        }
      >
        <Lock size={12} />
      </button>
      <button
        type="button"
        onClick={onDelete}
        title="Delete"
        className="shrink-0 rounded p-0.5 text-slate-400 hover:bg-red-100 hover:text-red-600 dark:hover:bg-red-900/40"
      >
        <Trash2 size={12} />
      </button>
    </div>
  );
}

function NewVariableRow({
  onCommit,
  onCancel,
}: {
  onCommit: (input: VariableInput) => void;
  onCancel: () => void;
}) {
  const [key, setKey] = useState("");
  const [value, setValue] = useState("");
  const [varType, setVarType] = useState<VarType>("string");
  const [isSecret, setIsSecret] = useState(false);
  // Enter fires commit via blur (see onKeyDown), which would otherwise also
  // fire its own blur-commit right after — guard so only the first wins.
  const committedRef = useRef(false);

  function commit() {
    if (committedRef.current) return;
    committedRef.current = true;
    const trimmed = key.trim();
    if (!trimmed) {
      onCancel();
      return;
    }
    onCommit({ key: trimmed, value, varType, isSecret, enabled: true });
  }

  return (
    <div className="flex items-center gap-1 px-1.5 py-0.5">
      <span className="w-[13px] shrink-0" />
      <input
        autoFocus
        value={key}
        onChange={(e) => setKey(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") e.currentTarget.blur();
          if (e.key === "Escape") onCancel();
        }}
        onBlur={commit}
        placeholder="key"
        className="min-w-0 flex-1 rounded border border-accent/40 bg-white px-1 py-0.5 text-xs font-mono focus:outline-none dark:bg-slate-900"
      />
      <select
        value={varType}
        onChange={(e) => setVarType(e.target.value as VarType)}
        className="shrink-0 rounded border border-slate-200 bg-transparent py-0.5 text-[11px] text-slate-400 dark:border-slate-700"
      >
        {VAR_TYPES.map((t) => (
          <option key={t} value={t}>
            {t}
          </option>
        ))}
      </select>
      <input
        value={value}
        onChange={(e) => setValue(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") e.currentTarget.blur();
          if (e.key === "Escape") onCancel();
        }}
        onBlur={commit}
        type={isSecret ? "password" : "text"}
        placeholder="value"
        className="min-w-0 flex-1 rounded border border-accent/40 bg-white px-1 py-0.5 text-xs font-mono focus:outline-none dark:bg-slate-900"
      />
      <button
        type="button"
        onClick={() => setIsSecret((s) => !s)}
        title={isSecret ? "Secret" : "Mark as secret"}
        className={"shrink-0 rounded p-0.5 " + (isSecret ? "text-amber-500" : "text-slate-300 dark:text-slate-600")}
      >
        <Lock size={12} />
      </button>
    </div>
  );
}
