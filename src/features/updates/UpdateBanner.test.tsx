//! Banner visibility and actions: hidden when idle/dismissed, shown when an
//! update is available, "Later" dismisses, progress text while downloading.

import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import type { Update } from "@tauri-apps/plugin-updater";

vi.mock("@tauri-apps/plugin-updater", () => ({ check: vi.fn() }));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: vi.fn() }));

import { UpdateBanner } from "./UpdateBanner";
import { useUpdaterStore } from "./useUpdater";

const initialState = useUpdaterStore.getState();
const update = { version: "0.3.0", downloadAndInstall: vi.fn() } as unknown as Update;

describe("UpdateBanner", () => {
  beforeEach(() => {
    useUpdaterStore.setState(initialState, true);
  });

  it("renders nothing when idle", () => {
    const { container } = render(<UpdateBanner />);
    expect(container.firstChild).toBeNull();
  });

  it("shows version and actions when an update is available", () => {
    useUpdaterStore.setState({ phase: "available", update });
    render(<UpdateBanner />);
    expect(screen.getByText("Update available: v0.3.0")).toBeTruthy();
    expect(screen.getByText("Install & Restart")).toBeTruthy();
  });

  it("'Later' dismisses the banner", () => {
    useUpdaterStore.setState({ phase: "available", update });
    const { container } = render(<UpdateBanner />);
    fireEvent.click(screen.getByText("Later"));
    expect(container.firstChild).toBeNull();
    expect(useUpdaterStore.getState().dismissed).toBe(true);
  });

  it("shows byte progress while downloading", () => {
    useUpdaterStore.setState({
      phase: "downloading",
      update,
      progress: { downloaded: 512 * 1024, total: 2 * 1024 * 1024 },
    });
    render(<UpdateBanner />);
    expect(screen.getByText("Downloading… 512 KB of 2.0 MB")).toBeTruthy();
    expect(screen.queryByText("Later")).toBeNull();
  });
});
