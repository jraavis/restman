//! Tests for the shared updater store: check flow (found / up-to-date /
//! silent vs. loud errors), download progress accounting from the plugin's
//! Started/Progress/Finished events, relaunch after install, and dismissal.
//! The `@tauri-apps` plugins are mocked — the real updater IPC only exists
//! inside a Tauri shell; the end-to-end hop is verified against a published
//! release instead (see PLAN.md).

import { beforeEach, describe, expect, it, vi } from "vitest";
import type { DownloadEvent, Update } from "@tauri-apps/plugin-updater";

const checkMock = vi.fn();
const relaunchMock = vi.fn();
vi.mock("@tauri-apps/plugin-updater", () => ({ check: (...a: unknown[]) => checkMock(...a) }));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: () => relaunchMock() }));

import { useUpdaterStore } from "./useUpdater";

function fakeUpdate(events: DownloadEvent[]): Update {
  return {
    version: "0.3.0",
    downloadAndInstall: vi.fn(async (onEvent?: (e: DownloadEvent) => void) => {
      for (const e of events) onEvent?.(e);
    }),
  } as unknown as Update;
}

const initialState = useUpdaterStore.getState();

describe("useUpdaterStore", () => {
  beforeEach(() => {
    useUpdaterStore.setState(initialState, true);
    checkMock.mockReset();
    relaunchMock.mockReset();
  });

  it("moves to 'available' with the update when check finds one", async () => {
    const update = fakeUpdate([]);
    checkMock.mockResolvedValue(update);
    await useUpdaterStore.getState().checkForUpdate();
    expect(useUpdaterStore.getState().phase).toBe("available");
    expect(useUpdaterStore.getState().update).toBe(update);
    expect(useUpdaterStore.getState().dismissed).toBe(false);
  });

  it("moves to 'upToDate' when check returns null", async () => {
    checkMock.mockResolvedValue(null);
    await useUpdaterStore.getState().checkForUpdate();
    expect(useUpdaterStore.getState().phase).toBe("upToDate");
  });

  it("silent check swallows errors back to idle", async () => {
    checkMock.mockRejectedValue(new Error("offline"));
    await useUpdaterStore.getState().checkForUpdate({ silent: true });
    expect(useUpdaterStore.getState().phase).toBe("idle");
    expect(useUpdaterStore.getState().error).toBeNull();
  });

  it("loud check surfaces errors", async () => {
    checkMock.mockRejectedValue(new Error("boom"));
    await useUpdaterStore.getState().checkForUpdate();
    expect(useUpdaterStore.getState().phase).toBe("error");
    expect(useUpdaterStore.getState().error).toContain("boom");
  });

  it("accumulates download progress from plugin events and relaunches", async () => {
    const update = fakeUpdate([
      { event: "Started", data: { contentLength: 100 } },
      { event: "Progress", data: { chunkLength: 40 } },
      { event: "Progress", data: { chunkLength: 60 } },
      { event: "Finished" },
    ] as DownloadEvent[]);
    checkMock.mockResolvedValue(update);
    await useUpdaterStore.getState().checkForUpdate();
    await useUpdaterStore.getState().installAndRestart();
    const s = useUpdaterStore.getState();
    expect(s.progress).toEqual({ downloaded: 100, total: 100 });
    expect(s.phase).toBe("installing");
    expect(relaunchMock).toHaveBeenCalledOnce();
  });

  it("null total when Started carries no contentLength", async () => {
    const update = fakeUpdate([
      { event: "Started", data: {} },
      { event: "Progress", data: { chunkLength: 10 } },
    ] as DownloadEvent[]);
    useUpdaterStore.setState({ phase: "available", update });
    await useUpdaterStore.getState().installAndRestart();
    expect(useUpdaterStore.getState().progress).toEqual({ downloaded: 10, total: null });
  });

  it("install failure lands in 'error' without relaunching", async () => {
    const update = {
      version: "0.3.0",
      downloadAndInstall: vi.fn().mockRejectedValue(new Error("sig mismatch")),
    } as unknown as Update;
    useUpdaterStore.setState({ phase: "available", update });
    await useUpdaterStore.getState().installAndRestart();
    expect(useUpdaterStore.getState().phase).toBe("error");
    expect(useUpdaterStore.getState().error).toContain("sig mismatch");
    expect(relaunchMock).not.toHaveBeenCalled();
  });

  it("installAndRestart is a no-op without an update", async () => {
    await useUpdaterStore.getState().installAndRestart();
    expect(useUpdaterStore.getState().phase).toBe("idle");
    expect(relaunchMock).not.toHaveBeenCalled();
  });

  it("dismiss hides, next found update un-dismisses", async () => {
    useUpdaterStore.getState().dismiss();
    expect(useUpdaterStore.getState().dismissed).toBe(true);
    checkMock.mockResolvedValue(fakeUpdate([]));
    await useUpdaterStore.getState().checkForUpdate();
    expect(useUpdaterStore.getState().dismissed).toBe(false);
  });
});
