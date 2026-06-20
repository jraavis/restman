//! Top bar: brand mark, workspace switcher (proves the IPC + DB round-trip),
//! environment indicator placeholder, sidebar toggle, and appearance settings.

import { useState } from "react";
import { ChevronDown, PanelLeft, Plus, Settings, Zap } from "lucide-react";
import {
  useActiveWorkspace,
  useCreateWorkspace,
  useSetActiveWorkspace,
  useWorkspaces,
} from "../features/workspaces/hooks";
import { SettingsPanel } from "../features/settings/SettingsPanel";
import { useUiStore } from "../stores/uiStore";

export function TopBar() {
  const { data: workspaces } = useWorkspaces();
  const { data: active } = useActiveWorkspace();
  const setActive = useSetActiveWorkspace();
  const createWs = useCreateWorkspace();
  const toggleSidebar = useUiStore((s) => s.toggleSidebar);
  const [settingsOpen, setSettingsOpen] = useState(false);

  return (
    <header className="relative flex h-12 shrink-0 items-center gap-2 border-b border-slate-200 bg-white px-3 dark:border-slate-800 dark:bg-slate-900">
      <button
        type="button"
        onClick={toggleSidebar}
        title="Toggle sidebar"
        className="flex h-7 w-7 items-center justify-center rounded-md text-slate-500 hover:bg-slate-100 dark:text-slate-400 dark:hover:bg-slate-800"
      >
        <PanelLeft size={16} />
      </button>

      <div className="flex items-center gap-1.5 pr-1">
        <span className="flex h-6 w-6 items-center justify-center rounded-md bg-accent text-white">
          <Zap size={13} strokeWidth={2.5} />
        </span>
        <span className="font-semibold tracking-tight text-slate-800 dark:text-slate-100">
          Restman
        </span>
      </div>

      <div className="ml-1 flex items-center gap-1">
        <div className="relative">
          <select
            value={active?.id ?? ""}
            onChange={(e) => setActive.mutate(e.target.value)}
            title="Active workspace"
            className="appearance-none rounded-full border border-slate-200 bg-slate-50 px-3 py-1 pr-7 text-sm font-medium text-slate-700 hover:bg-slate-100 focus:outline-none focus:ring-2 focus:ring-accent/40 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-200 dark:hover:bg-slate-700"
          >
            {(workspaces ?? []).map((w) => (
              <option key={w.id} value={w.id}>
                {w.name}
              </option>
            ))}
          </select>
          <ChevronDown
            size={13}
            className="pointer-events-none absolute right-2 top-1/2 -translate-y-1/2 text-slate-400"
          />
        </div>
        <button
          type="button"
          onClick={() => createWs.mutate(`Workspace ${(workspaces?.length ?? 0) + 1}`)}
          title="New workspace"
          className="flex h-7 w-7 items-center justify-center rounded-full text-slate-500 hover:bg-slate-100 dark:text-slate-400 dark:hover:bg-slate-800"
        >
          <Plus size={15} />
        </button>
      </div>

      <div className="ml-auto flex items-center gap-2">
        <span className="rounded-full bg-slate-100 px-2.5 py-1 text-xs text-slate-500 dark:bg-slate-800 dark:text-slate-400">
          No environment
        </span>
        <button
          type="button"
          onClick={() => setSettingsOpen((o) => !o)}
          title="Appearance settings"
          aria-expanded={settingsOpen}
          className={
            "flex h-7 w-7 items-center justify-center rounded-md transition-colors " +
            (settingsOpen
              ? "bg-accent/10 text-accent"
              : "text-slate-500 hover:bg-slate-100 dark:text-slate-400 dark:hover:bg-slate-800")
          }
        >
          <Settings size={16} />
        </button>
      </div>

      {settingsOpen && <SettingsPanel onClose={() => setSettingsOpen(false)} />}
    </header>
  );
}
