//! Ephemeral UI state (theme, accent, sidebar, layout sizing, editor prefs),
//! persisted to localStorage so the look and layout survive restarts.

import { create } from "zustand";
import { persist } from "zustand/middleware";

export type Theme = "light" | "dark" | "system";
export type Accent = "blue" | "green" | "purple" | "orange" | "pink" | "red";
export type SidePanel = "collections" | "history" | "environments";

/** Applied to a freshly created request/tab (`defaultRequest()`) — not
 * retroactive to existing requests, same as every other UI preference here. */
export interface DefaultRequestOptions {
  timeoutSecs: number;
  followRedirects: boolean;
  verifySsl: boolean;
}

const MIN_SIDEBAR = 200;
const MAX_SIDEBAR = 440;
const MIN_SPLIT = 0.25;
const MAX_SPLIT = 0.75;
const MIN_FONT = 11;
const MAX_FONT = 20;
const MIN_TAB_SIZE = 1;
const MAX_TAB_SIZE = 8;

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
  editorWordWrap: boolean;
  editorTabSize: number;
  activePanel: SidePanel;
  /** Settings → General. Currently only gates workspace deletion's confirm
   * dialog — other delete flows (collections/requests/environments/etc.)
   * don't consult it yet, a known scope limit for this pass. */
  confirmBeforeDelete: boolean;
  defaultRequestOptions: DefaultRequestOptions;
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
  setEditorWordWrap: (wrap: boolean) => void;
  setEditorTabSize: (size: number) => void;
  setActivePanel: (panel: SidePanel) => void;
  setConfirmBeforeDelete: (confirm: boolean) => void;
  setDefaultRequestOptions: (options: DefaultRequestOptions) => void;
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
      editorWordWrap: false,
      editorTabSize: 2,
      activePanel: "collections",
      confirmBeforeDelete: true,
      defaultRequestOptions: { timeoutSecs: 30, followRedirects: true, verifySsl: true },
      keybindingOverrides: {},
      setTheme: (theme) => set({ theme }),
      setAccent: (accent) => set({ accent }),
      toggleSidebar: () => set((s) => ({ sidebarOpen: !s.sidebarOpen })),
      setSidebarWidth: (width) => set({ sidebarWidth: clamp(width, MIN_SIDEBAR, MAX_SIDEBAR) }),
      setRequestSplit: (fraction) => set({ requestSplit: clamp(fraction, MIN_SPLIT, MAX_SPLIT) }),
      setEditorFontSize: (size) => set({ editorFontSize: clamp(size, MIN_FONT, MAX_FONT) }),
      setEditorWordWrap: (editorWordWrap) => set({ editorWordWrap }),
      setEditorTabSize: (size) => set({ editorTabSize: clamp(size, MIN_TAB_SIZE, MAX_TAB_SIZE) }),
      setActivePanel: (activePanel) => set({ activePanel }),
      setConfirmBeforeDelete: (confirmBeforeDelete) => set({ confirmBeforeDelete }),
      setDefaultRequestOptions: (defaultRequestOptions) => set({ defaultRequestOptions }),
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
