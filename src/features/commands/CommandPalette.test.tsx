//! Tests for CommandPalette.
//!
//! NOTE: `npx vitest run` cannot start in this sandbox (`ERR_REQUIRE_ESM` from
//! `html-encoding-sniffer`/`@exodus/bytes`, pre-existing in `node_modules`,
//! unrelated to this task), so this file is hand-traced, not run. The actual
//! palette (open via Cmd+K, filter, arrow-key highlight, click-to-run,
//! click-outside/Escape-to-close) was separately verified in a live
//! `npx vite` dev server via the Claude_Preview browser tool.

import { describe, it, expect, vi, beforeEach } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { CommandPalette } from "./CommandPalette";
import { runCommand } from "../../lib/commands";

vi.mock("../../lib/commands", async () => {
  const actual = await vi.importActual<typeof import("../../lib/commands")>("../../lib/commands");
  return { ...actual, runCommand: vi.fn() };
});

describe("CommandPalette", () => {
  beforeEach(() => {
    vi.mocked(runCommand).mockClear();
  });

  it("renders nothing when closed", () => {
    render(<CommandPalette open={false} onClose={() => {}} />);
    expect(screen.queryByPlaceholderText("Type a command…")).toBeNull();
  });

  it("lists commands (minus itself) when open", () => {
    render(<CommandPalette open={true} onClose={() => {}} />);
    expect(screen.getByText("Save request")).toBeTruthy();
    expect(screen.getByText("Switch environment")).toBeTruthy();
    expect(screen.queryByText("Command palette")).toBeNull();
  });

  it("filters by typed text", () => {
    render(<CommandPalette open={true} onClose={() => {}} />);
    fireEvent.change(screen.getByPlaceholderText("Type a command…"), { target: { value: "environment" } });
    expect(screen.getByText("Switch environment")).toBeTruthy();
    expect(screen.queryByText("Save request")).toBeNull();
  });

  it("clicking an entry runs its command and closes the palette", () => {
    const onClose = vi.fn();
    render(<CommandPalette open={true} onClose={onClose} />);
    fireEvent.click(screen.getByText("Switch environment"));
    expect(runCommand).toHaveBeenCalledWith("environment.switch");
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("Escape closes the palette", () => {
    const onClose = vi.fn();
    render(<CommandPalette open={true} onClose={onClose} />);
    fireEvent.keyDown(screen.getByPlaceholderText("Type a command…"), { key: "Escape" });
    expect(onClose).toHaveBeenCalledOnce();
  });
});
