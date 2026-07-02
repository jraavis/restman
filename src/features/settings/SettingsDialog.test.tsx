//! Tests for SettingsDialog.
//!
//! NOTE: `npx vitest run` cannot start in this sandbox (`ERR_REQUIRE_ESM` from
//! `html-encoding-sniffer`/`@exodus/bytes`, pre-existing in `node_modules`,
//! unrelated to this task), so this file is hand-traced, not run. The actual
//! dialog (all 6 tabs, theme/accent/confirm-delete toggles, editor
//! font/wrap/tab-size, network defaults, history retention/clear, keybinding
//! remap capture + collision guard) was separately verified in a live
//! `npx vite` dev server via the Claude_Preview browser tool — including a
//! real bug caught and fixed there: the keybinding-capture input's keydown
//! was bubbling to the global shortcut listener and firing whatever command
//! the newly-typed combo already belonged to.

import { describe, it, expect, vi, beforeEach } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { SettingsDialog } from "./SettingsDialog";
import { useUiStore } from "../../stores/uiStore";
import { ipc } from "../../lib/ipc";

vi.mock("../../lib/ipc", () => ({
  ipc: {
    getHistoryRetention: vi.fn().mockResolvedValue(200),
    setHistoryRetention: vi.fn(),
    clearHistory: vi.fn(),
  },
}));

function renderDialog(workspaceId?: string) {
  const qc = new QueryClient();
  return render(
    <QueryClientProvider client={qc}>
      <SettingsDialog onClose={() => {}} workspaceId={workspaceId} />
    </QueryClientProvider>,
  );
}

describe("SettingsDialog", () => {
  beforeEach(() => {
    vi.mocked(ipc.getHistoryRetention).mockClear();
  });

  it("defaults to the General tab", () => {
    renderDialog();
    expect(screen.getByText("Theme")).toBeTruthy();
  });

  it("switches to the Editor tab and toggles word wrap", () => {
    renderDialog();
    fireEvent.click(screen.getByText("Editor"));
    fireEvent.click(screen.getByText("Word wrap"));
    expect(useUiStore.getState().editorWordWrap).toBe(true);
  });

  it("switches to the Network tab and edits the default timeout", () => {
    renderDialog();
    fireEvent.click(screen.getByText("Network"));
    const input = screen.getByDisplayValue("30");
    fireEvent.change(input, { target: { value: "60" } });
    expect(useUiStore.getState().defaultRequestOptions.timeoutSecs).toBe(60);
  });

  it("Data tab's clear-history button is disabled without a workspace", () => {
    renderDialog(undefined);
    fireEvent.click(screen.getByText("Data"));
    expect(screen.getByText("Clear history for this workspace").closest("button")).toBeDisabled();
  });

  it("Data tab's clear-history button is enabled with an active workspace", () => {
    renderDialog("ws-1");
    fireEvent.click(screen.getByText("Data"));
    expect(screen.getByText("Clear history for this workspace").closest("button")).not.toBeDisabled();
  });
});
