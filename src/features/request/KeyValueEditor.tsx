//! Editable key/value table with per-row enable toggles. Used for query params,
//! url-encoded fields, and (via name↔key mapping) headers.

import { useId } from "react";
import { Plus, Trash2 } from "lucide-react";
import { VariableSuggestInput } from "../../components/VariableSuggestInput";

export interface Pair {
  key: string;
  value: string;
  enabled: boolean;
}

interface Props {
  rows: Pair[];
  onChange: (rows: Pair[]) => void;
  keyPlaceholder?: string;
  valuePlaceholder?: string;
  /** Optional autocomplete suggestions for the key field. */
  keySuggestions?: string[];
  /** Known `{{var}}` names offered while typing in the value field. */
  variableKeys?: string[];
}

export function KeyValueEditor({
  rows,
  onChange,
  keyPlaceholder = "Key",
  valuePlaceholder = "Value",
  keySuggestions,
  variableKeys,
}: Props) {
  const listId = useId();

  const update = (i: number, patch: Partial<Pair>) =>
    onChange(rows.map((r, idx) => (idx === i ? { ...r, ...patch } : r)));
  const remove = (i: number) => onChange(rows.filter((_, idx) => idx !== i));
  const add = () => onChange([...rows, { key: "", value: "", enabled: true }]);

  const cell =
    "min-w-0 flex-1 rounded-md border border-transparent bg-transparent px-2 py-1 text-sm focus:border-slate-300 focus:outline-none dark:focus:border-slate-600";

  return (
    <div className="flex flex-col overflow-hidden rounded-lg border border-slate-200 dark:border-slate-800">
      {keySuggestions && (
        <datalist id={listId}>
          {keySuggestions.map((s) => (
            <option key={s} value={s} />
          ))}
        </datalist>
      )}
      {rows.length === 0 && (
        <p className="px-3 py-3 text-xs text-slate-400">Nothing here yet.</p>
      )}
      {rows.map((row, i) => (
        <div
          key={i}
          className="group flex items-center gap-1 border-b border-slate-100 px-1 last:border-b-0 hover:bg-slate-50 dark:border-slate-800 dark:hover:bg-slate-800/60"
        >
          <input
            type="checkbox"
            checked={row.enabled}
            onChange={(e) => update(i, { enabled: e.target.checked })}
            className="ml-1.5"
            title="Enabled"
          />
          <input
            value={row.key}
            list={keySuggestions ? listId : undefined}
            onChange={(e) => update(i, { key: e.target.value })}
            placeholder={keyPlaceholder}
            className={cell}
          />
          <VariableSuggestInput
            value={row.value}
            onChange={(value) => update(i, { value })}
            placeholder={valuePlaceholder}
            className={cell}
            variableKeys={variableKeys}
          />
          <button
            type="button"
            onClick={() => remove(i)}
            title="Remove"
            className="flex h-7 w-7 items-center justify-center rounded-md text-slate-300 opacity-0 transition-opacity group-hover:opacity-100 hover:bg-red-50 hover:text-red-500 dark:text-slate-600 dark:hover:bg-red-950/40"
          >
            <Trash2 size={14} />
          </button>
        </div>
      ))}
      <button
        type="button"
        onClick={add}
        className="flex items-center gap-1.5 px-3 py-2 text-xs font-medium text-accent hover:bg-accent/5"
      >
        <Plus size={13} />
        Add row
      </button>
    </div>
  );
}
