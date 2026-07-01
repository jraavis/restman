//! Tests for HistoryDiffDialog.
//!
//! NOTE: `npx vitest run` cannot start in this sandbox (`ERR_REQUIRE_ESM` from
//! `html-encoding-sniffer`/`@exodus/bytes`, pre-existing in `node_modules`,
//! unrelated to this task), so this file is hand-traced, not run.

import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import { HistoryDiffDialog } from "./HistoryDiffDialog";
import type { HistoryEntry } from "../../lib/types";

const base = {
  workspaceId: "w1",
  requestId: null,
  status: 200,
  durationMs: 12,
  request: {
    method: "GET",
    url: "https://a",
    headers: [],
    query: [],
    body: { mode: "none" as const },
    options: { timeoutSecs: 30, followRedirects: true, verifySsl: true, maxRedirects: 10, sendCookies: false },
  },
  response: null,
  error: null,
  createdAt: 0,
};
const entryA: HistoryEntry = { ...base, id: "a", name: "Entry A", url: "https://a", method: "GET" };
const entryB: HistoryEntry = {
  ...base,
  id: "b",
  name: "Entry B",
  url: "https://b",
  method: "POST",
  request: { ...base.request, method: "POST", url: "https://b" },
};

describe("HistoryDiffDialog", () => {
  it("shows both entry names and a diff section that changed", () => {
    render(<HistoryDiffDialog entryA={entryA} entryB={entryB} onClose={() => {}} />);
    expect(screen.getByText(/A: Entry A/)).toBeTruthy();
    expect(screen.getByText(/B: Entry B/)).toBeTruthy();
    expect(screen.getByText("Request line")).toBeTruthy();
  });

  it("marks an unchanged section as (no change)", () => {
    render(<HistoryDiffDialog entryA={entryA} entryB={entryA} onClose={() => {}} />);
    expect(screen.getAllByText("(no change)").length).toBeGreaterThan(0);
  });

  it("calls onClose when the backdrop is clicked", () => {
    const onClose = vi.fn();
    const { container } = render(<HistoryDiffDialog entryA={entryA} entryB={entryB} onClose={onClose} />);
    (container.firstChild as HTMLElement).click();
    expect(onClose).toHaveBeenCalledOnce();
  });
});
