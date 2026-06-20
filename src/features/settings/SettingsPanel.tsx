//! Appearance settings popover: theme, accent color, editor font size.
//! Anchored under the gear button in the TopBar; closes on outside click/Escape.

import { useEffect, useRef } from "react";
import { Minus, Monitor, Moon, Plus, Sun } from "lucide-react";
import { useUiStore, type Accent, type Theme } from "../../stores/uiStore";

const ACCENTS: { id: Accent; label: string; swatch: string }[] = [
  { id: "blue", label: "Blue", swatch: "#3b82f6" },
  { id: "green", label: "Green", swatch: "#16a34a" },
  { id: "purple", label: "Purple", swatch: "#8b5cf6" },
  { id: "orange", label: "Orange", swatch: "#f97316" },
  { id: "pink", label: "Pink", swatch: "#ec4899" },
  { id: "red", label: "Red", swatch: "#ef4444" },
];

const THEMES: { id: Theme; label: string; icon: typeof Sun }[] = [
  { id: "light", label: "Light", icon: Sun },
  { id: "dark", label: "Dark", icon: Moon },
  { id: "system", label: "System", icon: Monitor },
];

interface Props {
  onClose: () => void;
}

export function SettingsPanel({ onClose }: Props) {
  const ref = useRef<HTMLDivElement>(null);
  const theme = useUiStore((s) => s.theme);
  const setTheme = useUiStore((s) => s.setTheme);
  const accent = useUiStore((s) => s.accent);
  const setAccent = useUiStore((s) => s.setAccent);
  const fontSize = useUiStore((s) => s.editorFontSize);
  const setEditorFontSize = useUiStore((s) => s.setEditorFontSize);

  useEffect(() => {
    const onPointerDown = (e: PointerEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) onClose();
    };
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [onClose]);

  return (
    <div
      ref={ref}
      role="dialog"
      aria-label="Appearance settings"
      className="absolute right-3 top-12 z-50 w-64 rounded-xl border border-slate-200 bg-white p-3 shadow-lg shadow-slate-900/10 dark:border-slate-700 dark:bg-slate-900 dark:shadow-black/40"
    >
      <p className="px-1 text-xs font-semibold tracking-wide text-slate-400 uppercase dark:text-slate-500">
        Theme
      </p>
      <div className="mt-1.5 grid grid-cols-3 gap-1">
        {THEMES.map(({ id, label, icon: Icon }) => (
          <button
            key={id}
            type="button"
            onClick={() => setTheme(id)}
            className={
              "flex flex-col items-center gap-1 rounded-lg border px-2 py-1.5 text-xs transition-colors " +
              (theme === id
                ? "border-accent/40 bg-accent/10 text-accent"
                : "border-transparent text-slate-500 hover:bg-slate-100 dark:text-slate-400 dark:hover:bg-slate-800")
            }
          >
            <Icon size={15} />
            {label}
          </button>
        ))}
      </div>

      <p className="mt-3 px-1 text-xs font-semibold tracking-wide text-slate-400 uppercase dark:text-slate-500">
        Accent
      </p>
      <div className="mt-1.5 flex flex-wrap gap-2 px-1">
        {ACCENTS.map((a) => (
          <button
            key={a.id}
            type="button"
            title={a.label}
            onClick={() => setAccent(a.id)}
            className={
              "h-6 w-6 rounded-full ring-offset-2 ring-offset-white transition-shadow dark:ring-offset-slate-900 " +
              (accent === a.id ? "ring-2 ring-slate-900 dark:ring-white" : "")
            }
            style={{ backgroundColor: a.swatch }}
          />
        ))}
      </div>

      <p className="mt-3 px-1 text-xs font-semibold tracking-wide text-slate-400 uppercase dark:text-slate-500">
        Editor font size
      </p>
      <div className="mt-1.5 flex items-center gap-2 px-1">
        <button
          type="button"
          onClick={() => setEditorFontSize(fontSize - 1)}
          className="flex h-6 w-6 items-center justify-center rounded-md text-slate-500 hover:bg-slate-100 dark:text-slate-400 dark:hover:bg-slate-800"
        >
          <Minus size={13} />
        </button>
        <span className="w-6 text-center text-sm tabular-nums">{fontSize}</span>
        <button
          type="button"
          onClick={() => setEditorFontSize(fontSize + 1)}
          className="flex h-6 w-6 items-center justify-center rounded-md text-slate-500 hover:bg-slate-100 dark:text-slate-400 dark:hover:bg-slate-800"
        >
          <Plus size={13} />
        </button>
      </div>
    </div>
  );
}
