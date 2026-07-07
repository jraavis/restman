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

import { afterEach, describe, it, expect, vi } from "vitest";
import { useEffect } from "react";
import { fireEvent, render, screen } from "@testing-library/react";
import { BodyEditor, type GraphqlBodyPanelState } from "./BodyEditor";
import type { RequestBody } from "../../lib/http";

// A minimal fake of the bits of `editor.IStandaloneCodeEditor` that
// `insertFieldAtCursor` (in `BodyEditor.tsx`) actually calls, so the
// caret-aware-insert path can be exercised without a real Monaco instance
// (which vitest/jsdom can't run — see the mocked `LazyCodeEditor` below).
// `vi.hoisted` so the mock factory (itself hoisted above these imports by
// vitest) can reach it.
const { fakeEditor } = vi.hoisted(() => ({
  fakeEditor: { current: null as unknown },
}));

vi.mock("../../components/LazyCodeEditor", () => ({
  LazyCodeEditor: ({
    value,
    onChange,
    onMount,
  }: {
    value: string;
    onChange: (v: string) => void;
    onMount?: (ed: unknown) => void;
  }) => {
    // Runs once per mount, matching real `onMount` semantics — `onMount` is
    // deliberately left out of the dep list since it's a fresh inline
    // closure every render in `BodyEditor` and only the mount-time call
    // matters here.
    useEffect(() => {
      if (fakeEditor.current) onMount?.(fakeEditor.current);
    }, []);
    return <textarea data-testid="monaco" value={value} onChange={(e) => onChange(e.target.value)} />;
  },
}));

function makeFakeEditor(selection: { startLineNumber: number; startColumn: number } | null) {
  return {
    getSelection: vi.fn(() => selection),
    executeEdits: vi.fn(),
    setSelection: vi.fn(),
    revealPositionInCenterIfOutsideViewport: vi.fn(),
    focus: vi.fn(),
  };
}

vi.mock("./LazyGraphqlDocsExplorer", () => ({
  LazyGraphqlDocsExplorer: ({ onInsert }: { onInsert: (name: string) => void }) => (
    <button onClick={() => onInsert("pets")}>mock-docs-entry</button>
  ),
}));

const graphqlBody: RequestBody = { mode: "graphql", data: { query: "{ pets { id } }", variables: "" } };

function idlePanel(overrides: Partial<GraphqlBodyPanelState> = {}): GraphqlBodyPanelState {
  return { status: "idle", schema: null, error: null, onFetchSchema: vi.fn(), ...overrides };
}

afterEach(() => {
  fakeEditor.current = null;
});

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

  it("clicking a docs entry with a mounted editor inserts at the cursor instead of appending", () => {
    const editor = makeFakeEditor({ startLineNumber: 1, startColumn: 5 });
    fakeEditor.current = editor;
    const onChange = vi.fn();
    const fakeSchema = {} as GraphqlBodyPanelState["schema"];
    render(
      <BodyEditor
        body={graphqlBody}
        onChange={onChange}
        graphqlSchemaState={idlePanel({ status: "ready", schema: fakeSchema })}
      />,
    );
    fireEvent.click(screen.getByText("Docs"));
    fireEvent.click(screen.getByText("mock-docs-entry"));

    // Replaces whatever `getSelection()` reported (here a collapsed cursor
    // at column 5) with the field name, rather than touching the query end.
    expect(editor.executeEdits).toHaveBeenCalledWith("graphql-docs-insert", [
      { range: { startLineNumber: 1, startColumn: 5 }, text: "pets", forceMoveMarkers: true },
    ]);
    // Cursor lands right after the inserted text (column 5 + "pets".length).
    expect(editor.setSelection).toHaveBeenCalledWith({
      startLineNumber: 1,
      startColumn: 9,
      endLineNumber: 1,
      endColumn: 9,
    });
    expect(editor.focus).toHaveBeenCalledOnce();
    // The imperative Monaco edit — not a synthesized `onChange` call — is
    // what's expected to drive the value here (mirrors how `@monaco-editor/
    // react` really works: it listens to the model's own change event).
    expect(onChange).not.toHaveBeenCalled();
  });

  it("falls back to appending when the editor hasn't reported a selection yet", () => {
    fakeEditor.current = makeFakeEditor(null);
    const onChange = vi.fn();
    const fakeSchema = {} as GraphqlBodyPanelState["schema"];
    render(
      <BodyEditor
        body={graphqlBody}
        onChange={onChange}
        graphqlSchemaState={idlePanel({ status: "ready", schema: fakeSchema })}
      />,
    );
    fireEvent.click(screen.getByText("Docs"));
    fireEvent.click(screen.getByText("mock-docs-entry"));
    expect(onChange).toHaveBeenCalledWith({
      mode: "graphql",
      data: { query: "{ pets { id } }\npets", variables: "" },
    });
  });
});
