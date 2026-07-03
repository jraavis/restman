//! Top-level layout: top bar over a sidebar + main (request / response) split.
//! The request/response divide is a drag-resizable vertical split.

import { useState } from "react";
import { ResizeHandle } from "../components/ResizeHandle";
import { CommandPalette } from "../features/commands/CommandPalette";
import { UpdateBanner } from "../features/updates/UpdateBanner";
import { useAutoUpdateCheck } from "../features/updates/useUpdater";
import { useGlobalCommandShortcuts, useRegisterCommand } from "../lib/commands";
import { useUiStore } from "../stores/uiStore";
import { useAppMenu } from "./useAppMenu";
import { RequestPane } from "./RequestPane";
import { ResponsePanel } from "./ResponsePanel";
import { Sidebar } from "./Sidebar";
import { TopBar } from "./TopBar";

export function AppShell() {
  const sidebarOpen = useUiStore((s) => s.sidebarOpen);
  const requestSplit = useUiStore((s) => s.requestSplit);
  const setRequestSplit = useUiStore((s) => s.setRequestSplit);
  useGlobalCommandShortcuts();
  useAppMenu();
  useAutoUpdateCheck();

  const [paletteOpen, setPaletteOpen] = useState(false);
  useRegisterCommand("app.commandPalette", () => setPaletteOpen((o) => !o));

  return (
    <div className="flex h-full flex-col bg-slate-50 text-slate-900 dark:bg-slate-950 dark:text-slate-100">
      <CommandPalette open={paletteOpen} onClose={() => setPaletteOpen(false)} />
      <UpdateBanner />
      <TopBar />
      <div className="flex min-h-0 flex-1">
        {sidebarOpen && <Sidebar />}
        <main
          id="main-column"
          className="grid min-w-0 flex-1"
          style={{ gridTemplateRows: `${requestSplit * 100}% auto ${(1 - requestSplit) * 100}%` }}
        >
          <div className="min-h-0 overflow-hidden">
            <RequestPane />
          </div>
          <ResizeHandle
            orientation="horizontal"
            onResize={(dy) => {
              const main = document.getElementById("main-column");
              const total = main?.clientHeight ?? window.innerHeight;
              setRequestSplit(requestSplit + dy / total);
            }}
            className="border-y border-slate-200 dark:border-slate-800"
          />
          <div className="min-h-0 overflow-hidden">
            <ResponsePanel />
          </div>
        </main>
      </div>
    </div>
  );
}
