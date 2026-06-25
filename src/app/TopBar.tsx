//! Top bar: brand mark, workspace switcher (rename/delete via the overflow
//! menu), environment quick-switch indicator, sidebar toggle, and appearance
//! settings.

import { useState } from "react";
import { Cable, ChevronDown, Cookie, MoreHorizontal, PanelLeft, Pencil, Plus, Radio, Settings, Settings2, Trash2, Zap } from "lucide-react";
import {
  useActiveWorkspace,
  useCreateWorkspace,
  useDeleteWorkspace,
  useSetActiveWorkspace,
  useUpdateWorkspace,
  useWorkspaces,
} from "../features/workspaces/hooks";
import { WorkspaceSettingsDialog } from "../features/workspaces/WorkspaceSettingsDialog";
import { CookieJarDialog } from "../features/cookies/CookieJarDialog";
import { SsePanel } from "../features/streaming/SsePanel";
import { WsPanel } from "../features/streaming/WsPanel";
import { EnvironmentSwitcher } from "../features/environments/EnvironmentSwitcher";
import { SettingsPanel } from "../features/settings/SettingsPanel";
import { useDismissable } from "../lib/useDismissable";
import { useUiStore } from "../stores/uiStore";

export function TopBar() {
  const { data: workspaces } = useWorkspaces();
  const { data: active } = useActiveWorkspace();
  const setActive = useSetActiveWorkspace();
  const createWs = useCreateWorkspace();
  const updateWs = useUpdateWorkspace();
  const deleteWs = useDeleteWorkspace();
  const toggleSidebar = useUiStore((s) => s.toggleSidebar);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [cookiesOpen, setCookiesOpen] = useState(false);
  const [streamingOpen, setStreamingOpen] = useState(false);
  const [wsOpen, setWsOpen] = useState(false);
  const [wsSettingsOpen, setWsSettingsOpen] = useState(false);
  const [wsMenuOpen, setWsMenuOpen] = useState(false);
  const [renaming, setRenaming] = useState(false);
  const [draftName, setDraftName] = useState("");
  const wsMenuRef = useDismissable<HTMLDivElement>(() => setWsMenuOpen(false));

  function commitRename() {
    setRenaming(false);
    const trimmed = draftName.trim();
    if (active && trimmed && trimmed !== active.name) {
      updateWs.mutate({ id: active.id, name: trimmed });
    }
  }

  function handleDeleteWorkspace() {
    setWsMenuOpen(false);
    if (!active) return;
    if ((workspaces?.length ?? 0) <= 1) return;
    if (window.confirm(`Delete workspace "${active.name}" and everything in it? This can't be undone.`)) {
      deleteWs.mutate(active.id);
    }
  }

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
        {renaming ? (
          <input
            autoFocus
            value={draftName}
            onChange={(e) => setDraftName(e.target.value)}
            onBlur={commitRename}
            onKeyDown={(e) => {
              if (e.key === "Enter") commitRename();
              if (e.key === "Escape") setRenaming(false);
            }}
            className="rounded-full border border-accent/40 bg-white px-3 py-1 text-sm focus:outline-none dark:bg-slate-900"
          />
        ) : (
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
        )}
        <button
          type="button"
          onClick={() => createWs.mutate(`Workspace ${(workspaces?.length ?? 0) + 1}`)}
          title="New workspace"
          className="flex h-7 w-7 items-center justify-center rounded-full text-slate-500 hover:bg-slate-100 dark:text-slate-400 dark:hover:bg-slate-800"
        >
          <Plus size={15} />
        </button>
        <div ref={wsMenuRef} className="relative">
          <button
            type="button"
            onClick={() => setWsMenuOpen((o) => !o)}
            title="Workspace actions"
            className="flex h-7 w-7 items-center justify-center rounded-full text-slate-500 hover:bg-slate-100 dark:text-slate-400 dark:hover:bg-slate-800"
          >
            <MoreHorizontal size={15} />
          </button>
          {wsMenuOpen && (
            <div className="absolute left-0 top-full z-50 mt-1 w-44 rounded-md border border-slate-200 bg-white py-1 text-xs shadow-lg dark:border-slate-700 dark:bg-slate-800">
              <button
                type="button"
                disabled={!active}
                onClick={() => {
                  setWsMenuOpen(false);
                  if (active) {
                    setDraftName(active.name);
                    setRenaming(true);
                  }
                }}
                className="flex w-full items-center gap-1.5 px-3 py-1.5 text-left hover:bg-slate-100 disabled:opacity-40 dark:hover:bg-slate-700"
              >
                <Pencil size={12} /> Rename
              </button>
              <button
                type="button"
                disabled={!active}
                onClick={() => {
                  setWsMenuOpen(false);
                  setWsSettingsOpen(true);
                }}
                className="flex w-full items-center gap-1.5 px-3 py-1.5 text-left hover:bg-slate-100 disabled:opacity-40 dark:hover:bg-slate-700"
              >
                <Settings2 size={12} /> Settings
              </button>
              <button
                type="button"
                disabled={!active || (workspaces?.length ?? 0) <= 1}
                onClick={handleDeleteWorkspace}
                title={(workspaces?.length ?? 0) <= 1 ? "Can't delete the only workspace" : undefined}
                className="flex w-full items-center gap-1.5 px-3 py-1.5 text-left text-red-500 hover:bg-red-50 disabled:opacity-40 disabled:hover:bg-transparent dark:hover:bg-red-900/30"
              >
                <Trash2 size={12} /> Delete
              </button>
            </div>
          )}
        </div>
      </div>

      <div className="ml-auto flex items-center gap-2">
        <EnvironmentSwitcher workspaceId={active?.id} />
        <button
          type="button"
          onClick={() => setCookiesOpen(true)}
          title="Cookies"
          className="flex h-7 w-7 items-center justify-center rounded-md text-slate-500 hover:bg-slate-100 dark:text-slate-400 dark:hover:bg-slate-800"
        >
          <Cookie size={16} />
        </button>
        <button
          type="button"
          disabled={!active}
          onClick={() => setStreamingOpen(true)}
          title="SSE streaming"
          className="flex h-7 w-7 items-center justify-center rounded-md text-slate-500 hover:bg-slate-100 disabled:opacity-40 dark:text-slate-400 dark:hover:bg-slate-800"
        >
          <Radio size={16} />
        </button>
        <button
          type="button"
          disabled={!active}
          onClick={() => setWsOpen(true)}
          title="WebSocket"
          className="flex h-7 w-7 items-center justify-center rounded-md text-slate-500 hover:bg-slate-100 disabled:opacity-40 dark:text-slate-400 dark:hover:bg-slate-800"
        >
          <Cable size={16} />
        </button>
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
      {cookiesOpen && <CookieJarDialog onClose={() => setCookiesOpen(false)} />}
      {streamingOpen && active && (
        <SsePanel workspaceId={active.id} onClose={() => setStreamingOpen(false)} />
      )}
      {wsOpen && active && (
        <WsPanel workspaceId={active.id} onClose={() => setWsOpen(false)} />
      )}
      {wsSettingsOpen && active && (
        <WorkspaceSettingsDialog
          workspaceId={active.id}
          workspaceName={active.name}
          onClose={() => setWsSettingsOpen(false)}
        />
      )}
    </header>
  );
}
