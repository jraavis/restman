//! Cmd+K command palette: searches `COMMANDS` (excluding itself — no point
//! offering "open the palette" from inside the palette) and runs whichever
//! entry the user picks via `runCommand`. Same overlay shell as
//! `CookieJarDialog`/other dialogs in this codebase.

import { useEffect, useMemo, useRef, useState } from "react";
import { CornerDownLeft, Search } from "lucide-react";
import { COMMANDS, runCommand } from "../../lib/commands";
import { useUiStore } from "../../stores/uiStore";

const SHORTCUT_LABELS: Record<string, string> = {
  mod: "⌘",
  shift: "⇧",
  alt: "⌥",
  enter: "⏎",
};

function formatShortcut(shortcut: string): string {
  return shortcut
    .split("+")
    .map((part) => SHORTCUT_LABELS[part] ?? part.toUpperCase())
    .join("");
}

export function CommandPalette({ open, onClose }: { open: boolean; onClose: () => void }) {
  const overrides = useUiStore((s) => s.keybindingOverrides);
  const [query, setQuery] = useState("");
  const [activeIndex, setActiveIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  const entries = useMemo(() => COMMANDS.filter((c) => c.id !== "app.commandPalette"), []);
  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return entries;
    return entries.filter(
      (c) => c.label.toLowerCase().includes(q) || c.category.toLowerCase().includes(q),
    );
  }, [entries, query]);

  useEffect(() => {
    if (open) {
      setQuery("");
      setActiveIndex(0);
      // Autofocus once the overlay has actually mounted.
      requestAnimationFrame(() => inputRef.current?.focus());
    }
  }, [open]);

  useEffect(() => {
    setActiveIndex(0);
  }, [query]);

  if (!open) return null;

  function choose(index: number) {
    const cmd = filtered[index];
    if (!cmd) return;
    runCommand(cmd.id);
    onClose();
  }

  return (
    <div className="fixed inset-0 z-50 flex items-start justify-center bg-black/30 pt-[15vh]" onClick={onClose}>
      <div
        onClick={(e) => e.stopPropagation()}
        className="flex max-h-[60vh] w-[32rem] flex-col overflow-hidden rounded-lg border border-slate-200 bg-white shadow-xl dark:border-slate-700 dark:bg-slate-800"
      >
        <div className="flex items-center gap-2 border-b border-slate-100 px-3 py-2.5 dark:border-slate-700">
          <Search size={14} className="shrink-0 text-slate-400" />
          <input
            ref={inputRef}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "ArrowDown") {
                e.preventDefault();
                setActiveIndex((i) => Math.min(i + 1, filtered.length - 1));
              } else if (e.key === "ArrowUp") {
                e.preventDefault();
                setActiveIndex((i) => Math.max(i - 1, 0));
              } else if (e.key === "Enter") {
                e.preventDefault();
                choose(activeIndex);
              } else if (e.key === "Escape") {
                onClose();
              }
            }}
            placeholder="Type a command…"
            className="w-full bg-transparent text-sm focus:outline-none dark:text-slate-100"
          />
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto py-1">
          {filtered.length === 0 && <p className="px-3 py-4 text-center text-xs text-slate-400">No matching commands.</p>}
          {filtered.map((c, i) => {
            const shortcut = overrides[c.id] ?? c.defaultShortcut;
            return (
              <button
                key={c.id}
                type="button"
                onMouseEnter={() => setActiveIndex(i)}
                onClick={() => choose(i)}
                className={
                  "flex w-full items-center justify-between px-3 py-2 text-left text-sm " +
                  (i === activeIndex
                    ? "bg-accent/10 text-accent"
                    : "text-slate-700 dark:text-slate-200")
                }
              >
                <span className="flex items-center gap-2">
                  <span>{c.label}</span>
                  <span className="text-[10px] font-semibold tracking-wide text-slate-400 uppercase">
                    {c.category}
                  </span>
                </span>
                {shortcut ? (
                  <span className="shrink-0 rounded border border-slate-200 px-1.5 py-0.5 font-mono text-[10px] text-slate-400 dark:border-slate-600">
                    {formatShortcut(shortcut)}
                  </span>
                ) : (
                  i === activeIndex && <CornerDownLeft size={12} className="shrink-0 text-accent" />
                )}
              </button>
            );
          })}
        </div>
      </div>
    </div>
  );
}
