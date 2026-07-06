//! Top bar: brand mark, workspace switcher (rename/delete via the overflow
//! menu), environment quick-switch indicator, sidebar toggle, and appearance
//! settings.

import { useState, type MouseEvent } from "react";
import { Cable, ChevronDown, Cookie, MoreHorizontal, Network, PanelLeft, Pencil, Plus, Puzzle, Radio, Server, Settings, Settings2, Trash2, Wrench } from "lucide-react";
import { useRegisterCommand } from "../lib/commands";
import { confirmDelete } from "../lib/confirmDelete";
import {
  useActiveWorkspace,
  useCreateWorkspace,
  useDeleteWorkspace,
  useSetActiveWorkspace,
  useUpdateWorkspace,
  useWorkspaces,
} from "../features/workspaces/hooks";
import { WorkspaceSettingsDialog } from "../features/workspaces/WorkspaceSettingsDialog";
import { PluginManagerDialog } from "../features/plugins/PluginManagerDialog";
import { MockServerManagerDialog } from "../features/mocks/MockServerManagerDialog";
import { CookieJarDialog } from "../features/cookies/CookieJarDialog";
import { SsePanel } from "../features/streaming/SsePanel";
import { WsPanel } from "../features/streaming/WsPanel";
import { GrpcPanel } from "../features/streaming/GrpcPanel";
import { EnvironmentSwitcher } from "../features/environments/EnvironmentSwitcher";
import { SettingsDialog } from "../features/settings/SettingsDialog";
import { ToolsDialog } from "../features/tools/ToolsDialog";
import { WindowControls } from "../components/WindowControls";
import { useDismissable } from "../lib/useDismissable";
import { useUiStore } from "../stores/uiStore";
import { useStreamingPanelStore } from "../stores/streamingPanelStore";
import { getCurrentWindow } from "@tauri-apps/api/window";

const isMac = /Mac|iPhone|iPod|iPad/.test(navigator.userAgent);

/** Frameless titlebar: drag on press, maximize/restore on double-click. */
function handleDragRegionMouseDown(e: MouseEvent) {
  if (e.button !== 0 || !("__TAURI_INTERNALS__" in window)) return;
  const win = getCurrentWindow();
  if (e.detail === 2) {
    void win.toggleMaximize();
  } else {
    void win.startDragging();
  }
}

export function TopBar() {
  const { data: workspaces } = useWorkspaces();
  const { data: active } = useActiveWorkspace();
  const setActive = useSetActiveWorkspace();
  const createWs = useCreateWorkspace();
  const updateWs = useUpdateWorkspace();
  const deleteWs = useDeleteWorkspace();
  const toggleSidebar = useUiStore((s) => s.toggleSidebar);
  const streamingPanel = useStreamingPanelStore((s) => s.panel);
  const openStreamingPanel = useStreamingPanelStore((s) => s.openStreamingPanel);
  const closeStreamingPanel = useStreamingPanelStore((s) => s.closeStreamingPanel);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [toolsOpen, setToolsOpen] = useState(false);
  const [cookiesOpen, setCookiesOpen] = useState(false);
  const [wsSettingsOpen, setWsSettingsOpen] = useState(false);
  const [pluginsOpen, setPluginsOpen] = useState(false);
  const [mockServersOpen, setMockServersOpen] = useState(false);
  const [wsMenuOpen, setWsMenuOpen] = useState(false);
  const [renaming, setRenaming] = useState(false);
  const [draftName, setDraftName] = useState("");
  const wsMenuRef = useDismissable<HTMLDivElement>(() => setWsMenuOpen(false));

  useRegisterCommand("app.openSettings", () => setSettingsOpen((o) => !o));
  useRegisterCommand("app.openTools", () => setToolsOpen((o) => !o));

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
    if (confirmDelete(`Delete workspace "${active.name}" and everything in it? This can't be undone.`)) {
      deleteWs.mutate(active.id);
    }
  }

  return (
    <header className="relative flex h-12 shrink-0 items-center gap-2 border-b border-slate-200 bg-white px-3 dark:border-slate-800 dark:bg-slate-900">
      {isMac && <WindowControls />}

      <button
        type="button"
        onClick={toggleSidebar}
        title="Toggle sidebar"
        className="flex h-7 w-7 items-center justify-center rounded-md text-slate-500 hover:bg-slate-100 dark:text-slate-400 dark:hover:bg-slate-800"
      >
        <PanelLeft size={16} />
      </button>

      <div
        className="flex cursor-default items-center gap-1.5 pr-1"
        onMouseDown={handleDragRegionMouseDown}
      >
        <img src="/restman.png" alt="" className="h-6 w-6 rounded-md" />
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
                disabled={!active}
                onClick={() => {
                  setWsMenuOpen(false);
                  setPluginsOpen(true);
                }}
                className="flex w-full items-center gap-1.5 px-3 py-1.5 text-left hover:bg-slate-100 disabled:opacity-40 dark:hover:bg-slate-700"
              >
                <Puzzle size={12} /> Plugins
              </button>
              <button
                type="button"
                disabled={!active}
                onClick={() => {
                  setWsMenuOpen(false);
                  setMockServersOpen(true);
                }}
                className="flex w-full items-center gap-1.5 px-3 py-1.5 text-left hover:bg-slate-100 disabled:opacity-40 dark:hover:bg-slate-700"
              >
                <Server size={12} /> Mock Servers
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

      <div
        className="min-h-full min-w-0 flex-1 cursor-default"
        onMouseDown={handleDragRegionMouseDown}
      />

      <div className="flex items-center gap-2">
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
          onClick={() => setToolsOpen(true)}
          title="Developer tools"
          className="flex h-7 w-7 items-center justify-center rounded-md text-slate-500 hover:bg-slate-100 dark:text-slate-400 dark:hover:bg-slate-800"
        >
          <Wrench size={16} />
        </button>
        <button
          type="button"
          disabled={!active}
          onClick={() => openStreamingPanel("sse")}
          title="SSE streaming"
          className="flex h-7 w-7 items-center justify-center rounded-md text-slate-500 hover:bg-slate-100 disabled:opacity-40 dark:text-slate-400 dark:hover:bg-slate-800"
        >
          <Radio size={16} />
        </button>
        <button
          type="button"
          disabled={!active}
          onClick={() => openStreamingPanel("ws")}
          title="WebSocket"
          className="flex h-7 w-7 items-center justify-center rounded-md text-slate-500 hover:bg-slate-100 disabled:opacity-40 dark:text-slate-400 dark:hover:bg-slate-800"
        >
          <Cable size={16} />
        </button>
        <button
          type="button"
          disabled={!active}
          onClick={() => openStreamingPanel("grpc")}
          title="gRPC"
          className="flex h-7 w-7 items-center justify-center rounded-md text-slate-500 hover:bg-slate-100 disabled:opacity-40 dark:text-slate-400 dark:hover:bg-slate-800"
        >
          <Network size={16} />
        </button>
        <button
          type="button"
          onClick={() => setSettingsOpen((o) => !o)}
          title="Settings"
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
        {!isMac && <WindowControls />}
      </div>

      {settingsOpen && <SettingsDialog onClose={() => setSettingsOpen(false)} workspaceId={active?.id} />}
      {toolsOpen && <ToolsDialog onClose={() => setToolsOpen(false)} />}
      {cookiesOpen && <CookieJarDialog onClose={() => setCookiesOpen(false)} />}
      {streamingPanel?.kind === "sse" && active && (
        <SsePanel workspaceId={active.id} savedRequest={streamingPanel.savedRequest} onClose={closeStreamingPanel} />
      )}
      {streamingPanel?.kind === "ws" && active && (
        <WsPanel workspaceId={active.id} savedRequest={streamingPanel.savedRequest} onClose={closeStreamingPanel} />
      )}
      {streamingPanel?.kind === "grpc" && active && (
        <GrpcPanel workspaceId={active.id} savedRequest={streamingPanel.savedRequest} onClose={closeStreamingPanel} />
      )}
      {wsSettingsOpen && active && (
        <WorkspaceSettingsDialog
          workspaceId={active.id}
          workspaceName={active.name}
          onClose={() => setWsSettingsOpen(false)}
        />
      )}
      {pluginsOpen && active && (
        <PluginManagerDialog workspaceId={active.id} onClose={() => setPluginsOpen(false)} />
      )}
      {mockServersOpen && active && (
        <MockServerManagerDialog workspaceId={active.id} onClose={() => setMockServersOpen(false)} />
      )}
    </header>
  );
}
