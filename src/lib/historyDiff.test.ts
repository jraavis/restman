//! Tests for the history-diff pure helpers.
//!
//! NOTE: `npx vitest run` cannot start in this sandbox (`ERR_REQUIRE_ESM` from
//! `html-encoding-sniffer`/`@exodus/bytes`, pre-existing in `node_modules`,
//! unrelated to this task), so this file hasn't run under vitest — but these
//! exact assertions were run standalone via `npx tsx` before this file was
//! committed.

import { describe, expect, it } from "vitest";
import { buildSideBySideDiff, headersToLines, historyDiffSections, requestBodyToText } from "./historyDiff";
import type { HistoryEntry } from "./types";

describe("buildSideBySideDiff", () => {
  it("aligns unchanged lines on both sides and a changed line as removed+added", () => {
    const rows = buildSideBySideDiff("a\nb\nc", "a\nx\nc");
    expect(rows).toEqual([
      { left: "a", right: "a", kind: "same" },
      { left: "b", right: null, kind: "removed" },
      { left: null, right: "x", kind: "added" },
      { left: "c", right: "c", kind: "same" },
    ]);
  });

  it("doesn't emit a phantom trailing row for a trailing newline", () => {
    expect(buildSideBySideDiff("a\n", "a\n")).toHaveLength(1);
  });
});

describe("headersToLines", () => {
  it("sorts and drops disabled headers", () => {
    const lines = headersToLines([
      { name: "Zeta", value: "1", enabled: true },
      { name: "Alpha", value: "2", enabled: true },
      { name: "Skip", value: "x", enabled: false },
    ]);
    expect(lines).toBe("Alpha: 2\nZeta: 1");
  });
});

describe("requestBodyToText", () => {
  it("returns empty string for no body", () => {
    expect(requestBodyToText({ mode: "none" })).toBe("");
  });

  it("pretty-prints JSON bodies", () => {
    expect(requestBodyToText({ mode: "json", data: '{"a":1}' })).toBe('{\n  "a": 1\n}');
  });

  it("renders urlEncoded pairs as key=value lines", () => {
    expect(requestBodyToText({ mode: "urlEncoded", data: [{ key: "a", value: "1", enabled: true }] })).toBe("a=1");
  });
});

describe("historyDiffSections", () => {
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
    response: {
      status: 200,
      statusText: "OK",
      headers: [],
      bodyBase64: btoa("hello"),
      sizeBytes: 5,
      timing: { totalMs: 1, dnsMs: null, connectMs: null, tlsMs: null, ttfbMs: null, downloadMs: null },
      finalUrl: "https://a",
      httpVersion: "1.1",
    },
    error: null,
    createdAt: 0,
  };
  const a: HistoryEntry = { ...base, id: "a", name: "A", url: "https://a", method: "GET" };
  const b: HistoryEntry = {
    ...base,
    id: "b",
    name: "B",
    url: "https://b",
    method: "POST",
    request: { ...base.request, method: "POST", url: "https://b" },
  };

  it("diffs the request line", () => {
    const section = historyDiffSections(a, b).find((s) => s.label === "Request line");
    expect(section).toEqual({ label: "Request line", before: "GET https://a", after: "POST https://b" });
  });
});
