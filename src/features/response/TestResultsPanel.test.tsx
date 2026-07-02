//! Tests for the TestResultsPanel component.

import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { TestResultsPanel } from "./TestResultsPanel";
import type { ScriptResult } from "../../lib/types";

function makeResult(overrides: Partial<ScriptResult> = {}): ScriptResult {
  return {
    tests: [],
    error: null,
    envMutations: [],
    envUnsets: [],
    aborted: false,
    ...overrides,
  };
}

describe("TestResultsPanel", () => {
  it("renders empty state when no scripts ran", () => {
    render(<TestResultsPanel preScript={null} postScript={null} />);
    expect(screen.getByText(/No tests ran/i)).toBeTruthy();
  });

  it("shows empty state when scripts ran but no tests defined", () => {
    const r = makeResult({ tests: [] });
    render(<TestResultsPanel preScript={r} postScript={null} />);
    // empty result (no tests, no error) → filtered out → empty state
    expect(screen.getByText(/No tests ran/i)).toBeTruthy();
  });

  it("renders passed test count in summary bar", () => {
    const r = makeResult({
      tests: [
        { name: "status is 200", passed: true, error: null },
        { name: "body has token", passed: true, error: null },
      ],
    });
    render(<TestResultsPanel preScript={null} postScript={r} />);
    expect(screen.getByText(/2 passed/i)).toBeTruthy();
  });

  it("renders failed count when there are failures", () => {
    const r = makeResult({
      tests: [
        { name: "status is 200", passed: true, error: null },
        { name: "body has token", passed: false, error: "Expected abc to equal xyz" },
      ],
    });
    render(<TestResultsPanel preScript={null} postScript={r} />);
    expect(screen.getByText(/1 failed/i)).toBeTruthy();
    expect(screen.getByText("Expected abc to equal xyz")).toBeTruthy();
  });

  it("renders script error (uncaught exception) in an alert box", () => {
    const r = makeResult({ error: "ReferenceError: foo is not defined", tests: [] });
    render(<TestResultsPanel preScript={null} postScript={r} />);
    expect(screen.getByText(/ReferenceError/)).toBeTruthy();
  });

  it("renders both pre and post sections", () => {
    const pre = makeResult({ tests: [{ name: "pre-check", passed: true, error: null }] });
    const post = makeResult({ tests: [{ name: "post-check", passed: false, error: "oops" }] });
    render(<TestResultsPanel preScript={pre} postScript={post} />);
    expect(screen.getByText("Pre-request")).toBeTruthy();
    expect(screen.getByText("Post-response")).toBeTruthy();
  });
});
