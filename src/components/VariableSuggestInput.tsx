//! Drop-in replacement for a text `<input>` that pops up `{{var}}` suggestions
//! while typing `{{` — used by the URL bar and KeyValueEditor's value cells.
//! Monaco gets the equivalent via `monacoVariableCompletion.ts`; plain HTML
//! inputs have no completion API of their own, so this reimplements the
//! trigger-on-`{{`/filter/insert loop directly over `selectionStart`.

import { useRef, useState, type KeyboardEvent } from "react";

interface Props {
  value: string;
  onChange: (value: string) => void;
  variableKeys?: string[];
  placeholder?: string;
  className?: string;
  spellCheck?: boolean;
  onKeyDown?: (e: KeyboardEvent<HTMLInputElement>) => void;
}

function findTrigger(value: string, caret: number): { start: number; query: string } | null {
  const upToCaret = value.slice(0, caret);
  const idx = upToCaret.lastIndexOf("{{");
  if (idx === -1) return null;
  const between = upToCaret.slice(idx + 2);
  if (/[{}\s]/.test(between)) return null;
  return { start: idx + 2, query: between };
}

export function VariableSuggestInput({
  value,
  onChange,
  variableKeys = [],
  placeholder,
  className,
  spellCheck,
  onKeyDown,
}: Props) {
  const inputRef = useRef<HTMLInputElement>(null);
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [triggerStart, setTriggerStart] = useState<number | null>(null);
  const [highlight, setHighlight] = useState(0);

  const filtered = variableKeys
    .filter((k) => k.toLowerCase().includes(query.toLowerCase()))
    .slice(0, 8);

  function syncTrigger(el: HTMLInputElement) {
    const caret = el.selectionStart ?? 0;
    const trig = variableKeys.length ? findTrigger(el.value, caret) : null;
    if (trig) {
      setTriggerStart(trig.start);
      setQuery(trig.query);
      setOpen(true);
      setHighlight(0);
    } else {
      setOpen(false);
    }
  }

  function commit(key: string) {
    if (triggerStart == null) return;
    const caret = inputRef.current?.selectionStart ?? triggerStart;
    const next = `${value.slice(0, triggerStart)}${key}}}${value.slice(caret)}`;
    onChange(next);
    setOpen(false);
    const newCaret = triggerStart + key.length + 2;
    requestAnimationFrame(() => {
      inputRef.current?.setSelectionRange(newCaret, newCaret);
      inputRef.current?.focus();
    });
  }

  function handleKeyDown(e: KeyboardEvent<HTMLInputElement>) {
    if (open && filtered.length > 0) {
      if (e.key === "ArrowDown") {
        e.preventDefault();
        setHighlight((h) => (h + 1) % filtered.length);
        return;
      }
      if (e.key === "ArrowUp") {
        e.preventDefault();
        setHighlight((h) => (h - 1 + filtered.length) % filtered.length);
        return;
      }
      if (e.key === "Enter" || e.key === "Tab") {
        e.preventDefault();
        commit(filtered[highlight]);
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        setOpen(false);
        return;
      }
    }
    onKeyDown?.(e);
  }

  return (
    <div className="relative min-w-0 flex-1">
      <input
        ref={inputRef}
        value={value}
        onChange={(e) => {
          onChange(e.target.value);
          syncTrigger(e.target);
        }}
        onKeyDown={handleKeyDown}
        onKeyUp={(e) => syncTrigger(e.currentTarget)}
        onClick={(e) => syncTrigger(e.currentTarget)}
        onBlur={() => setOpen(false)}
        placeholder={placeholder}
        spellCheck={spellCheck}
        className={"w-full " + (className ?? "")}
      />
      {open && filtered.length > 0 && (
        <ul className="absolute left-0 top-full z-50 mt-1 max-h-48 w-max min-w-[10rem] overflow-auto rounded-md border border-slate-200 bg-white py-1 text-xs shadow-lg dark:border-slate-700 dark:bg-slate-800">
          {filtered.map((k, i) => (
            <li
              key={k}
              onMouseDown={(e) => {
                e.preventDefault();
                commit(k);
              }}
              className={
                "cursor-pointer px-2.5 py-1 font-mono " +
                (i === highlight
                  ? "bg-accent/10 text-accent"
                  : "hover:bg-slate-100 dark:hover:bg-slate-700")
              }
            >
              {"{{"}
              {k}
              {"}}"}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
