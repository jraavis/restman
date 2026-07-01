//! Request body editor covering all seven modes.

import { useState } from "react";
import { BookOpen, Plus, RefreshCw, Trash2 } from "lucide-react";
import { LazyCodeEditor } from "../../components/LazyCodeEditor";
import { VariableSuggestInput } from "../../components/VariableSuggestInput";
import {
  emptyBody,
  type BodyMode,
  type FormField,
  type KeyValue,
  type RequestBody,
} from "../../lib/http";
import type { GraphqlSchemaStatus } from "./graphqlSchemaHooks";
import { KeyValueEditor, type Pair } from "./KeyValueEditor";
import { LazyGraphqlDocsExplorer } from "./LazyGraphqlDocsExplorer";
import type { GraphQLSchema } from "graphql";

/** `BodyEditor` doesn't have workspace/collection/request context, so
 * `RequestBuilder` (which does) binds `useGraphqlSchema()`'s `fetchSchema`
 * down to a plain zero-arg trigger before passing it here. */
export interface GraphqlBodyPanelState {
  status: GraphqlSchemaStatus;
  schema: GraphQLSchema | null;
  error: string | null;
  onFetchSchema: () => void;
}

const MODES: { id: BodyMode; label: string }[] = [
  { id: "none", label: "None" },
  { id: "json", label: "JSON" },
  { id: "raw", label: "Raw" },
  { id: "formData", label: "Form Data" },
  { id: "urlEncoded", label: "x-www-form-urlencoded" },
  { id: "binary", label: "Binary" },
  { id: "graphql", label: "GraphQL" },
];

const RAW_LANGUAGES = ["text", "json", "xml", "html", "javascript", "python"];

interface Props {
  body: RequestBody;
  onChange: (body: RequestBody) => void;
  variableKeys?: string[];
  /** Only consulted when `body.mode === "graphql"` — introspection state and
   * the fetch trigger, owned by `RequestBuilder` (it has the workspace/
   * collection/request context an introspection fetch needs). */
  graphqlSchemaState?: GraphqlBodyPanelState;
}

export function BodyEditor({ body, onChange, variableKeys, graphqlSchemaState }: Props) {
  return (
    <div className="flex h-full flex-col">
      <div className="mb-3 flex w-fit flex-wrap gap-0.5 rounded-lg bg-slate-100 p-0.5 dark:bg-slate-800">
        {MODES.map((m) => (
          <button
            key={m.id}
            type="button"
            onClick={() => onChange(emptyBody(m.id))}
            className={
              "rounded-md px-2.5 py-1 text-xs font-medium transition-colors " +
              (body.mode === m.id
                ? "bg-white text-slate-900 shadow-sm dark:bg-slate-700 dark:text-white"
                : "text-slate-500 hover:text-slate-800 dark:text-slate-400 dark:hover:text-slate-200")
            }
          >
            {m.label}
          </button>
        ))}
      </div>
      <div className="min-h-0 flex-1">
        {renderEditor(body, onChange, variableKeys, graphqlSchemaState)}
      </div>
    </div>
  );
}

function renderEditor(
  body: RequestBody,
  onChange: (b: RequestBody) => void,
  variableKeys?: string[],
  graphqlSchemaState?: GraphqlBodyPanelState,
) {
  switch (body.mode) {
    case "none":
      return <p className="p-2 text-xs text-slate-400">This request has no body.</p>;

    case "json":
      return (
        <LazyCodeEditor
          language="json"
          height="220px"
          value={body.data}
          onChange={(v) => onChange({ mode: "json", data: v ?? "" })}
          variableKeys={variableKeys}
        />
      );

    case "raw":
      return (
        <div className="flex flex-col gap-2">
          <select
            value={body.data.language ?? "text"}
            onChange={(e) =>
              onChange({ mode: "raw", data: { ...body.data, language: e.target.value } })
            }
            className="self-start rounded-md border border-slate-200 bg-transparent px-2 py-1 text-xs focus:outline-none focus:ring-2 focus:ring-accent/40 dark:border-slate-700"
          >
            {RAW_LANGUAGES.map((l) => (
              <option key={l} value={l}>
                {l}
              </option>
            ))}
          </select>
          <LazyCodeEditor
            language={monacoLang(body.data.language)}
            height="190px"
            value={body.data.content}
            onChange={(v) =>
              onChange({ mode: "raw", data: { ...body.data, content: v ?? "" } })
            }
            variableKeys={variableKeys}
          />
        </div>
      );

    case "urlEncoded":
      return (
        <KeyValueEditor
          rows={body.data as Pair[]}
          onChange={(rows) => onChange({ mode: "urlEncoded", data: rows as KeyValue[] })}
          variableKeys={variableKeys}
        />
      );

    case "formData":
      return (
        <FormDataEditor
          fields={body.data}
          onChange={(f) => onChange({ mode: "formData", data: f })}
          variableKeys={variableKeys}
        />
      );

    case "binary":
      return (
        <div className="flex flex-col gap-1.5 p-1">
          <label className="text-xs text-slate-500 dark:text-slate-400">File path</label>
          <input
            value={body.data.path}
            onChange={(e) => onChange({ mode: "binary", data: { path: e.target.value } })}
            placeholder="/absolute/path/to/file"
            className="rounded-lg border border-slate-200 bg-transparent px-2 py-1.5 text-sm focus:outline-none focus:ring-2 focus:ring-accent/40 dark:border-slate-700"
          />
          <p className="text-xs text-slate-400">
            Paste an absolute path. Native file picker comes with the dialog plugin.
          </p>
        </div>
      );

    case "graphql":
      return <GraphqlBody body={body} onChange={onChange} variableKeys={variableKeys} schemaState={graphqlSchemaState} />;
  }
}

function GraphqlBody({
  body,
  onChange,
  variableKeys,
  schemaState,
}: {
  body: Extract<RequestBody, { mode: "graphql" }>;
  onChange: (b: RequestBody) => void;
  variableKeys?: string[];
  schemaState?: GraphqlBodyPanelState;
}) {
  const [docsOpen, setDocsOpen] = useState(false);
  const status = schemaState?.status ?? "idle";

  return (
    <div className="flex h-full min-h-0 gap-3">
      <div className="flex min-w-0 flex-1 flex-col gap-2">
        <div className="flex items-center justify-between">
          <span className="text-xs text-slate-500 dark:text-slate-400">Query</span>
          <div className="flex items-center gap-2">
            {status === "error" && (
              <span className="max-w-[220px] truncate text-xs text-red-500" title={schemaState?.error ?? ""}>
                {schemaState?.error}
              </span>
            )}
            {status === "ready" && <span className="text-xs text-green-600 dark:text-green-500">Schema loaded</span>}
            <button
              type="button"
              onClick={() => schemaState?.onFetchSchema()}
              disabled={status === "loading"}
              className="flex items-center gap-1 rounded-md border border-slate-200 px-2 py-1 text-xs font-medium text-slate-600 hover:bg-slate-100 disabled:opacity-50 dark:border-slate-700 dark:text-slate-300 dark:hover:bg-slate-800"
            >
              <RefreshCw size={11} className={status === "loading" ? "animate-spin" : ""} />
              {status === "loading" ? "Fetching…" : "Fetch schema"}
            </button>
            <button
              type="button"
              onClick={() => setDocsOpen((o) => !o)}
              disabled={!schemaState?.schema}
              className={
                "flex items-center gap-1 rounded-md border px-2 py-1 text-xs font-medium disabled:opacity-40 " +
                (docsOpen
                  ? "border-accent/40 bg-accent/10 text-accent"
                  : "border-slate-200 text-slate-600 hover:bg-slate-100 dark:border-slate-700 dark:text-slate-300 dark:hover:bg-slate-800")
              }
            >
              <BookOpen size={11} />
              Docs
            </button>
          </div>
        </div>
        <LazyCodeEditor
          language="graphql"
          height="150px"
          value={body.data.query}
          onChange={(v) => onChange({ mode: "graphql", data: { ...body.data, query: v ?? "" } })}
          variableKeys={variableKeys}
          graphqlSchema={schemaState?.schema}
        />
        <span className="text-xs text-slate-500 dark:text-slate-400">Operation name (optional)</span>
        <input
          value={body.data.operationName ?? ""}
          onChange={(e) => onChange({ mode: "graphql", data: { ...body.data, operationName: e.target.value } })}
          placeholder="e.g. GetPets — only needed when the query defines more than one operation"
          className="rounded-lg border border-slate-200 bg-transparent px-2 py-1.5 text-sm focus:outline-none focus:ring-2 focus:ring-accent/40 dark:border-slate-700"
        />
        <span className="text-xs text-slate-500 dark:text-slate-400">Variables (JSON)</span>
        <LazyCodeEditor
          language="json"
          height="90px"
          value={body.data.variables ?? ""}
          onChange={(v) => onChange({ mode: "graphql", data: { ...body.data, variables: v ?? "" } })}
          variableKeys={variableKeys}
        />
      </div>
      {docsOpen && schemaState?.schema && (
        <div className="w-64 shrink-0 overflow-y-auto rounded-lg border border-slate-200 p-2 dark:border-slate-800">
          <LazyGraphqlDocsExplorer
            schema={schemaState.schema}
            onInsert={(name) =>
              onChange({ mode: "graphql", data: { ...body.data, query: body.data.query + (body.data.query.endsWith("\n") || body.data.query === "" ? "" : "\n") + name } })
            }
          />
        </div>
      )}
    </div>
  );
}

function monacoLang(lang?: string | null): string {
  if (!lang || lang === "text") return "plaintext";
  return lang;
}

function FormDataEditor({
  fields,
  onChange,
  variableKeys,
}: {
  fields: FormField[];
  onChange: (fields: FormField[]) => void;
  variableKeys?: string[];
}) {
  const update = (i: number, patch: Partial<FormField>) =>
    onChange(fields.map((f, idx) => (idx === i ? { ...f, ...patch } : f)));
  const remove = (i: number) => onChange(fields.filter((_, idx) => idx !== i));
  const add = () =>
    onChange([...fields, { key: "", value: "", enabled: true, isFile: false }]);

  const cell =
    "min-w-0 flex-1 rounded-md border border-transparent bg-transparent px-2 py-1 text-sm focus:border-slate-300 focus:outline-none dark:focus:border-slate-600";

  return (
    <div className="flex flex-col overflow-hidden rounded-lg border border-slate-200 dark:border-slate-800">
      {fields.length === 0 && (
        <p className="px-3 py-3 text-xs text-slate-400">Nothing here yet.</p>
      )}
      {fields.map((f, i) => (
        <div
          key={i}
          className="group flex items-center gap-1 border-b border-slate-100 px-1 last:border-b-0 hover:bg-slate-50 dark:border-slate-800 dark:hover:bg-slate-800/60"
        >
          <input
            type="checkbox"
            checked={f.enabled}
            onChange={(e) => update(i, { enabled: e.target.checked })}
            className="ml-1.5"
          />
          <input
            value={f.key}
            onChange={(e) => update(i, { key: e.target.value })}
            placeholder="Field name"
            className={cell}
          />
          {f.isFile ? (
            <input
              value={f.value}
              onChange={(e) => update(i, { value: e.target.value })}
              placeholder="/path/to/file"
              className={cell}
            />
          ) : (
            <VariableSuggestInput
              value={f.value}
              onChange={(value) => update(i, { value })}
              placeholder="Value"
              className={cell}
              variableKeys={variableKeys}
            />
          )}
          <button
            type="button"
            onClick={() => update(i, { isFile: !f.isFile })}
            title="Toggle text/file"
            className={
              "rounded-full px-2 py-0.5 text-xs font-medium " +
              (f.isFile
                ? "bg-accent/15 text-accent"
                : "bg-slate-100 text-slate-500 dark:bg-slate-800 dark:text-slate-400")
            }
          >
            {f.isFile ? "File" : "Text"}
          </button>
          <button
            type="button"
            onClick={() => remove(i)}
            className="flex h-7 w-7 items-center justify-center rounded-md text-slate-300 opacity-0 transition-opacity group-hover:opacity-100 hover:bg-red-50 hover:text-red-500 dark:text-slate-600 dark:hover:bg-red-950/40"
          >
            <Trash2 size={14} />
          </button>
        </div>
      ))}
      <button
        type="button"
        onClick={add}
        className="flex items-center gap-1.5 px-3 py-2 text-xs font-medium text-accent hover:bg-accent/5"
      >
        <Plus size={13} />
        Add field
      </button>
    </div>
  );
}
