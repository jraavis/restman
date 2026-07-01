//! Ephemeral UI state (theme, accent, sidebar, layout sizing, editor prefs),
//! persisted to localStorage so the look and layout survive restarts.

import { create } from "zustand";
import { persist } from "zustand/middleware";

export type Theme = "light" | "dark" | "system";
export type Accent = "blue" | "green" | "purple" | "orange" | "pink" | "red";
export type SidePanel = "collections" | "history" | "environments";

const MIN_SIDEBAR = 200;
const MAX_SIDEBAR = 440;
const MIN_SPLIT = 0.25;
const MAX_SPLIT = 0.75;
const MIN_FONT = 11;
const MAX_FONT = 20;

function clamp(n: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, n));
}

interface UiState {
  theme: Theme;
  accent: Accent;
  sidebarOpen: boolean;
  sidebarWidth: number;
  /** Fraction (0..1) of the main column given to the request pane; rest goes to the response panel. */
  requestSplit: number;
  editorFontSize: number;
  activePanel: SidePanel;
  /** User-remapped shortcuts, keyed by command id (see `lib/commands.ts`).
   * Only holds entries that differ from a command's `defaultShortcut` — the
   * Keybindings settings tab writes here, `commandForShortcut` reads it. */
  keybindingOverrides: Record<string, string>;
  setTheme: (theme: Theme) => void;
  setAccent: (accent: Accent) => void;
  toggleSidebar: () => void;
  setSidebarWidth: (width: number) => void;
  setRequestSplit: (fraction: number) => void;
  setEditorFontSize: (size: number) => void;
  setActivePanel: (panel: SidePanel) => void;
  setKeybindingOverride: (commandId: string, shortcut: string) => void;
  clearKeybindingOverride: (commandId: string) => void;
}

export const useUiStore = create<UiState>()(
  persist(
    (set) => ({
      theme: "system",
      accent: "blue",
      sidebarOpen: true,
      sidebarWidth: 256,
      requestSplit: 0.45,
      editorFontSize: 13,
      activePanel: "collections",
      keybindingOverrides: {},
      setTheme: (theme) => set({ theme }),
      setAccent: (accent) => set({ accent }),
      toggleSidebar: () => set((s) => ({ sidebarOpen: !s.sidebarOpen })),
      setSidebarWidth: (width) => set({ sidebarWidth: clamp(width, MIN_SIDEBAR, MAX_SIDEBAR) }),
      setRequestSplit: (fraction) => set({ requestSplit: clamp(fraction, MIN_SPLIT, MAX_SPLIT) }),
      setEditorFontSize: (size) => set({ editorFontSize: clamp(size, MIN_FONT, MAX_FONT) }),
      setActivePanel: (activePanel) => set({ activePanel }),
      setKeybindingOverride: (commandId, shortcut) =>
        set((s) => ({ keybindingOverrides: { ...s.keybindingOverrides, [commandId]: shortcut } })),
      clearKeybindingOverride: (commandId) =>
        set((s) => {
          const next = { ...s.keybindingOverrides };
          delete next[commandId];
          return { keybindingOverrides: next };
        }),
    }),
    { name: "restman-ui" },
  ),
);
