//! Left sidebar with switchable panels. Collections/History/Environments are
//! wired in Phase 2; for now they show empty-state placeholders. Resizable
//! via the drag handle on its trailing edge.

import { FolderTree, History, Variable } from "lucide-react";
import { ResizeHandle } from "../components/ResizeHandle";
import { useUiStore, type SidePanel } from "../stores/uiStore";

const PANELS: { id: SidePanel; label: string; icon: typeof FolderTree }[] = [
  { id: "collections", label: "Collections", icon: FolderTree },
  { id: "history", label: "History", icon: History },
  { id: "environments", label: "Environments", icon: Variable },
];

export function Sidebar() {
  const activePanel = useUiStore((s) => s.activePanel);
  const setActivePanel = useUiStore((s) => s.setActivePanel);
  const sidebarWidth = useUiStore((s) => s.sidebarWidth);
  const setSidebarWidth = useUiStore((s) => s.setSidebarWidth);

  return (
    <div className="flex shrink-0 border-r border-slate-200 dark:border-slate-800">
      <aside
        style={{ width: sidebarWidth }}
        className="flex shrink-0 flex-col bg-white dark:bg-slate-900"
      >
        <nav className="flex border-b border-slate-200 dark:border-slate-800">
          {PANELS.map(({ id, label, icon: Icon }) => (
            <button
              key={id}
              type="button"
              onClick={() => setActivePanel(id)}
              title={label}
              className={
                "flex flex-1 flex-col items-center gap-1 border-b-2 px-2 py-2 text-xs font-medium transition-colors " +
                (activePanel === id
                  ? "border-accent text-accent"
                  : "border-transparent text-slate-500 hover:text-slate-800 dark:hover:text-slate-200")
              }
            >
              <Icon size={15} />
              {label}
            </button>
          ))}
        </nav>
        <div className="flex flex-1 flex-col items-center justify-center gap-1 overflow-auto p-3 text-center text-slate-400">
          <p className="text-sm font-medium capitalize text-slate-500 dark:text-slate-400">
            {activePanel}
          </p>
          <p className="text-xs">Coming in Phase 2.</p>
        </div>
      </aside>
      <ResizeHandle
        orientation="vertical"
        onResize={(dx) => setSidebarWidth(sidebarWidth + dx)}
      />
    </div>
  );
}
