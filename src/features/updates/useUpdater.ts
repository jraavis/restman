//! Shared in-app update flow (Phase: in-app updates). One non-persisted
//! zustand store drives both the Settings → About "Check for updates" button
//! and the launch-time `UpdateBanner`, so check/download/relaunch logic and
//! progress accounting exist exactly once. Talks to GitHub Releases through
//! `tauri-plugin-updater`'s JS bindings (endpoint + pubkey live in
//! `tauri.conf.json`) — no custom Rust command, same direct-plugin pattern
//! as the dialog plugin.

import { useEffect } from "react";
import { create } from "zustand";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { useUiStore } from "../../stores/uiStore";

export type UpdatePhase =
  | "idle"
  | "checking"
  | "available"
  | "downloading"
  | "installing"
  | "upToDate"
  | "error";

export interface UpdateProgress {
  downloaded: number;
  /** Bytes expected, or null when the server didn't send a content length
   * (progress bar renders indeterminate). */
  total: number | null;
}

interface UpdaterState {
  phase: UpdatePhase;
  update: Update | null;
  progress: UpdateProgress;
  /** "Later" was clicked — hides the banner until the next check finds
   * something. The About tab ignores this. */
  dismissed: boolean;
  error: string | null;
  /** `silent` swallows errors (startup auto-check: dev builds and offline
   * machines must stay quiet); the About tab passes false to surface them. */
  checkForUpdate: (opts?: { silent?: boolean }) => Promise<void>;
  installAndRestart: () => Promise<void>;
  dismiss: () => void;
}

export const useUpdaterStore = create<UpdaterState>()((set, get) => ({
  phase: "idle",
  update: null,
  progress: { downloaded: 0, total: null },
  dismissed: false,
  error: null,

  checkForUpdate: async ({ silent = false } = {}) => {
    const { phase } = get();
    if (phase === "checking" || phase === "downloading" || phase === "installing") return;
    set({ phase: "checking", error: null });
    try {
      const update = await check();
      if (update) {
        set({ phase: "available", update, dismissed: false });
      } else {
        set({ phase: "upToDate", update: null });
      }
    } catch (e) {
      set(silent ? { phase: "idle" } : { phase: "error", error: String(e) });
    }
  },

  installAndRestart: async () => {
    const { update, phase } = get();
    if (!update || phase === "downloading" || phase === "installing") return;
    set({ phase: "downloading", progress: { downloaded: 0, total: null }, error: null });
    try {
      await update.downloadAndInstall((event) => {
        if (event.event === "Started") {
          set({ progress: { downloaded: 0, total: event.data.contentLength ?? null } });
        } else if (event.event === "Progress") {
          set((s) => ({
            progress: { ...s.progress, downloaded: s.progress.downloaded + event.data.chunkLength },
          }));
        } else if (event.event === "Finished") {
          set({ phase: "installing" });
        }
      });
      await relaunch();
    } catch (e) {
      set({ phase: "error", error: String(e) });
    }
  },

  dismiss: () => set({ dismissed: true }),
}));

/** Delay before the startup check so it never competes with first paint /
 * initial workspace loads. */
const AUTO_CHECK_DELAY_MS = 3000;

/** Mount once (AppShell). Runs a silent check shortly after launch when the
 * `autoCheckUpdates` preference is on and we're inside a real Tauri shell —
 * plain-vite dev/preview has no updater IPC, so it must not even try. */
export function useAutoUpdateCheck() {
  const autoCheckUpdates = useUiStore((s) => s.autoCheckUpdates);
  const checkForUpdate = useUpdaterStore((s) => s.checkForUpdate);

  useEffect(() => {
    if (!autoCheckUpdates) return;
    if (!("__TAURI_INTERNALS__" in window)) return;
    const timer = setTimeout(() => void checkForUpdate({ silent: true }), AUTO_CHECK_DELAY_MS);
    return () => clearTimeout(timer);
  }, [autoCheckUpdates, checkForUpdate]);
}
