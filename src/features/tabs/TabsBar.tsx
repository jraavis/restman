//! Open-tabs bar above the request builder. Owns tab switch/close/reorder
//! and the debounced flush of the live draft back to its DB row; the
//! bootstrap-on-empty and active-tab-load sync live in `useTabSync`.

import { useEffect, useRef, useState } from "react";
import { MoreHorizontal, Plus, X } from "lucide-react";
import { useRegisterCommands } from "../../lib/commands";
import { defaultRequest } from "../../lib/http";
import { methodBadgeClasses } from "../../lib/methods";
import type { Tab } from "../../lib/types";
import { useDismissable } from "../../lib/useDismissable";
import { useRequestStore } from "../../stores/requestStore";
import { useUiStore } from "../../stores/uiStore";
import { useActiveWorkspace } from "../workspaces/hooks";
import {
  useCloseAllTabs,
  useCloseOtherTabs,
  useCloseTab,
  useCreateTab,
  useReorderTabs,
  useSetActiveTab,
  useUpdateTabDraft,
} from "./hooks";
import { useTabSync } from "./useTabSync";

export function TabsBar() {
  const { data: workspace } = useActiveWorkspace();
  const workspaceId = workspace?.id;
  const { tabs, isLoadingTabs } = useTabSync(workspaceId);

  const storeActiveTabId = useRequestStore((s) => s.activeTabId);
  const title = useRequestStore((s) => s.title);
  const request = useRequestStore((s) => s.request);
  const defaultRequestOptions = useUiStore((s) => s.defaultRequestOptions);

  const createTab = useCreateTab(workspaceId);
  const setActiveTab = useSetActiveTab(workspaceId);
  const closeTab = useCloseTab(workspaceId);
  const closeOtherTabs = useCloseOtherTabs(workspaceId);
  const closeAllTabs = useCloseAllTabs(workspaceId);
  const reorderTabs = useReorderTabs(workspaceId);
  const { mutate: flushDraft } = useUpdateTabDraft(workspaceId);

  // Debounce the live draft back to the active tab's row.
  const flushTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(() => {
    if (!storeActiveTabId) return;
    flushTimer.current = setTimeout(() => {
      flushDraft({ id: storeActiveTabId, title, draft: request });
    }, 500);
    return () => {
      if (flushTimer.current) clearTimeout(flushTimer.current);
    };
  }, [storeActiveTabId, title, request, flushDraft]);

  function switchTo(id: string) {
    if (id === storeActiveTabId) return;
    // Flush the outgoing tab immediately so up to 500ms of edits aren't lost.
    if (flushTimer.current) {
      clearTimeout(flushTimer.current);
      flushTimer.current = null;
    }
    if (storeActiveTabId) flushDraft({ id: storeActiveTabId, title, draft: request });
    setActiveTab.mutate(id);
  }

  // Cmd/Ctrl+1..9 switches to the tab at that position; reads the latest
  // tabs through a ref since `useRegisterCommands` only re-runs its effect
  // when the set of command ids changes, not on every tabs/switchTo update.
  const tabsRef = useRef(tabs);
  tabsRef.current = tabs;
  const switchToRef = useRef(switchTo);
  switchToRef.current = switchTo;

  useRegisterCommands({
    ...Object.fromEntries(
      Array.from({ length: 9 }, (_, i) => [
        `tab.switchTo.${i + 1}`,
        () => {
          const target = tabsRef.current[i];
          if (target) switchToRef.current(target.id);
        },
      ]),
    ),
    "tab.new": () => createTab.mutate({ requestId: null, title: "Untitled", draft: defaultRequest(defaultRequestOptions) }),
  });

  const dragIndex = useRef<number | null>(null);
  function onDrop(toIndex: number) {
    const fromIndex = dragIndex.current;
    dragIndex.current = null;
    if (fromIndex === null || fromIndex === toIndex) return;
    const ids = tabs.map((t) => t.id);
    const [moved] = ids.splice(fromIndex, 1);
    ids.splice(toIndex, 0, moved);
    reorderTabs.mutate(ids);
  }

  if (!workspaceId || (isLoadingTabs && tabs.length === 0)) {
    return <div className="h-9 shrink-0 border-b border-slate-200 dark:border-slate-800" />;
  }

  return (
    <div className="flex h-9 shrink-0 items-stretch border-b border-slate-200 dark:border-slate-800">
      <div className="flex min-w-0 flex-1 overflow-x-auto">
        {tabs.map((tab, index) => (
          <TabChip
            key={tab.id}
            tab={tab}
            isActive={tab.id === storeActiveTabId}
            liveTitle={title}
            liveMethod={request.method}
            onSelect={() => switchTo(tab.id)}
            onClose={(e) => {
              e.stopPropagation();
              // If this is the active tab, drop its pending flush — it's
              // about to be deleted, and a flush landing after that would
              // hit a NotFound update.
              if (tab.id === storeActiveTabId && flushTimer.current) {
                clearTimeout(flushTimer.current);
                flushTimer.current = null;
              }
              closeTab.mutate(tab.id);
            }}
            onDragStart={() => {
              dragIndex.current = index;
            }}
            onDragOver={(e) => e.preventDefault()}
            onDrop={() => onDrop(index)}
          />
        ))}
      </div>
      <button
        type="button"
        title="New tab"
        onClick={() => createTab.mutate({ requestId: null, title: "Untitled", draft: defaultRequest(defaultRequestOptions) })}
        className="flex shrink-0 items-center px-2 text-slate-400 hover:text-slate-700 dark:hover:text-slate-200"
      >
        <Plus size={15} />
      </button>
      <OverflowMenu
        disabled={tabs.length <= 1}
        onCloseOthers={() => storeActiveTabId && closeOtherTabs.mutate(storeActiveTabId)}
        onCloseAll={() => closeAllTabs.mutate()}
      />
    </div>
  );
}

function TabChip({
  tab,
  isActive,
  liveTitle,
  liveMethod,
  onSelect,
  onClose,
  onDragStart,
  onDragOver,
  onDrop,
}: {
  tab: Tab;
  isActive: boolean;
  liveTitle: string;
  liveMethod: string;
  onSelect: () => void;
  onClose: (e: React.MouseEvent) => void;
  onDragStart: () => void;
  onDragOver: (e: React.DragEvent) => void;
  onDrop: () => void;
}) {
  const title = isActive ? liveTitle : tab.title;
  const method = isActive ? liveMethod : tab.draft.method;
  return (
    <div
      draggable
      onDragStart={onDragStart}
      onDragOver={onDragOver}
      onDrop={onDrop}
      onClick={onSelect}
      title={title}
      className={
        "group flex min-w-[7rem] max-w-[14rem] shrink-0 cursor-pointer items-center gap-1.5 border-r border-slate-200 px-2.5 text-xs dark:border-slate-800 " +
        (isActive
          ? "bg-white text-slate-800 dark:bg-slate-900 dark:text-slate-100"
          : "bg-slate-50 text-slate-500 hover:bg-slate-100 dark:bg-slate-950 dark:hover:bg-slate-900")
      }
    >
      <span className={"shrink-0 rounded border px-1 text-[10px] font-bold " + methodBadgeClasses(method)}>
        {method}
      </span>
      <span className="min-w-0 flex-1 truncate">{title}</span>
      <button
        type="button"
        onClick={onClose}
        title="Close tab"
        className="shrink-0 rounded p-0.5 opacity-0 hover:bg-slate-200 group-hover:opacity-100 dark:hover:bg-slate-700"
      >
        <X size={12} />
      </button>
    </div>
  );
}

function OverflowMenu({
  disabled,
  onCloseOthers,
  onCloseAll,
}: {
  disabled: boolean;
  onCloseOthers: () => void;
  onCloseAll: () => void;
}) {
  const [open, setOpen] = useState(false);
  const ref = useDismissable<HTMLDivElement>(() => setOpen(false));

  return (
    <div ref={ref} className="relative flex shrink-0 items-center border-l border-slate-200 dark:border-slate-800">
      <button
        type="button"
        title="Tab actions"
        onClick={() => setOpen((o) => !o)}
        className="flex items-center px-2 text-slate-400 hover:text-slate-700 dark:hover:text-slate-200"
      >
        <MoreHorizontal size={15} />
      </button>
      {open && (
        <div className="absolute right-0 top-full z-10 mt-1 w-40 rounded-md border border-slate-200 bg-white py-1 text-xs shadow-lg dark:border-slate-700 dark:bg-slate-800">
          <button
            type="button"
            disabled={disabled}
            onClick={() => {
              onCloseOthers();
              setOpen(false);
            }}
            className="block w-full px-3 py-1.5 text-left hover:bg-slate-100 disabled:opacity-40 disabled:hover:bg-transparent dark:hover:bg-slate-700"
          >
            Close other tabs
          </button>
          <button
            type="button"
            disabled={disabled}
            onClick={() => {
              onCloseAll();
              setOpen(false);
            }}
            className="block w-full px-3 py-1.5 text-left hover:bg-slate-100 disabled:opacity-40 disabled:hover:bg-transparent dark:hover:bg-slate-700"
          >
            Close all tabs
          </button>
        </div>
      )}
    </div>
  );
}
