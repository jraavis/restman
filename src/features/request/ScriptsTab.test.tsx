//! Tests for the ScriptsTab component.

import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import { ScriptsTab } from "./ScriptsTab";

// Monaco editors are heavy; mock LazyCodeEditor so this test doesn't need
// a real browser renderer.
vi.mock("../../components/LazyCodeEditor", () => ({
  LazyCodeEditor: ({ value }: { value: string }) => (
    <textarea data-testid="monaco" defaultValue={value} />
  ),
}));

describe("ScriptsTab", () => {
  it("renders pre-request and post-response editor sections", () => {
    render(
      <ScriptsTab
        preScript=""
        postScript=""
        onPreChange={() => {}}
        onPostChange={() => {}}
      />,
    );
    expect(screen.getByText(/Pre-request script/i)).toBeTruthy();
    expect(screen.getByText(/Post-response script/i)).toBeTruthy();
  });

  it("passes current script values to editors", () => {
    render(
      <ScriptsTab
        preScript="pm.abort();"
        postScript="pm.test('ok', () => {});"
        onPreChange={() => {}}
        onPostChange={() => {}}
      />,
    );
    const editors = screen.getAllByTestId("monaco");
    const values = editors.map((e) => (e as HTMLTextAreaElement).defaultValue);
    expect(values).toContain("pm.abort();");
    expect(values).toContain("pm.test('ok', () => {});");
  });
});
