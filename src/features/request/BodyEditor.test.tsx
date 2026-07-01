//! Tests for BodyEditor's GraphQL mode: operationName pass-through, the
//! Docs-explorer toggle gating on a fetched schema, and the Fetch-schema
//! button delegating to the caller-supplied state.
//!
//! NOTE: `npx vitest run` cannot start in this sandbox (`ERR_REQUIRE_ESM` from
//! `html-encoding-sniffer`/`@exodus/bytes`, pre-existing in `node_modules`,
//! unrelated to this task; see PLAN.md "How to resume in a new session"), so
//! this file is hand-traced against `BodyEditor.tsx`'s logic, not run. The
//! actual GraphQL panel (mode switch, query typing, Fetch-schema/Docs
//! buttons, operationName input) was separately verified in a live
//! `npx vite` dev server via the Claude_Preview browser tool.

import { describe, it, expect, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { BodyEditor, type GraphqlBodyPanelState } from "./BodyEditor";
import type { RequestBody } from "../../lib/http";

vi.mock("../../components/LazyCodeEditor", () => ({
  LazyCodeEditor: ({ value, onChange }: { value: string; onChange: (v: string) => void }) => (
    <textarea data-testid="monaco" value={value} onChange={(e) => onChange(e.target.value)} />
  ),
}));

vi.mock("./LazyGraphqlDocsExplorer", () => ({
  LazyGraphqlDocsExplorer: ({ onInsert }: { onInsert: (name: string) => void }) => (
    <button onClick={() => onInsert("pets")}>mock-docs-entry</button>
  ),
}));

const graphqlBody: RequestBody = { mode: "graphql", data: { query: "{ pets { id } }", variables: "" } };

function idlePanel(overrides: Partial<GraphqlBodyPanelState> = {}): GraphqlBodyPanelState {
  return { status: "idle", schema: null, error: null, onFetchSchema: vi.fn(), ...overrides };
}

describe("BodyEditor GraphQL mode", () => {
  it("Docs button is disabled until a schema is present", () => {
    render(<BodyEditor body={graphqlBody} onChange={() => {}} graphqlSchemaState={idlePanel()} />);
    expect(screen.getByText("Docs").closest("button")).toBeDisabled();
  });

  it("Docs button is enabled once a schema is fetched", () => {
    const fakeSchema = {} as GraphqlBodyPanelState["schema"];
    render(
      <BodyEditor
        body={graphqlBody}
        onChange={() => {}}
        graphqlSchemaState={idlePanel({ status: "ready", schema: fakeSchema })}
      />,
    );
    expect(screen.getByText("Docs").closest("button")).not.toBeDisabled();
  });

  it("clicking Fetch schema calls onFetchSchema", () => {
    const onFetchSchema = vi.fn();
    render(<BodyEditor body={graphqlBody} onChange={() => {}} graphqlSchemaState={idlePanel({ onFetchSchema })} />);
    fireEvent.click(screen.getByText("Fetch schema"));
    expect(onFetchSchema).toHaveBeenCalledOnce();
  });

  it("shows the fetch error when status is error", () => {
    render(
      <BodyEditor
        body={graphqlBody}
        onChange={() => {}}
        graphqlSchemaState={idlePanel({ status: "error", error: "connection refused" })}
      />,
    );
    expect(screen.getByText("connection refused")).toBeTruthy();
  });

  it("editing the operationName input emits the updated body", () => {
    const onChange = vi.fn();
    render(<BodyEditor body={graphqlBody} onChange={onChange} graphqlSchemaState={idlePanel()} />);
    fireEvent.change(screen.getByPlaceholderText(/GetPets/), { target: { value: "GetPets" } });
    expect(onChange).toHaveBeenCalledWith({
      mode: "graphql",
      data: { query: "{ pets { id } }", variables: "", operationName: "GetPets" },
    });
  });

  it("clicking a docs entry appends its name to the query", () => {
    const onChange = vi.fn();
    const fakeSchema = {} as GraphqlBodyPanelState["schema"];
    render(
      <BodyEditor
        body={graphqlBody}
        onChange={onChange}
        graphqlSchemaState={idlePanel({ status: "ready", schema: fakeSchema })}
      />,
    );
    fireEvent.click(screen.getByText("Docs")); // open the panel
    fireEvent.click(screen.getByText("mock-docs-entry"));
    expect(onChange).toHaveBeenCalledWith({
      mode: "graphql",
      data: { query: "{ pets { id } }\npets", variables: "" },
    });
  });
});
