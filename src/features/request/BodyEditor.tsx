//! Request body editor covering all seven modes.

import { Plus, Trash2 } from "lucide-react";
import { LazyCodeEditor } from "../../components/LazyCodeEditor";
import {
  emptyBody,
  type BodyMode,
  type FormField,
  type KeyValue,
  type RequestBody,
} from "../../lib/http";
import { KeyValueEditor, type Pair } from "./KeyValueEditor";

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
}

export function BodyEditor({ body, onChange }: Props) {
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
      <div className="min-h-0 flex-1">{renderEditor(body, onChange)}</div>
    </div>
  );
}

function renderEditor(body: RequestBody, onChange: (b: RequestBody) => void) {
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
          />
        </div>
      );

    case "urlEncoded":
      return (
        <KeyValueEditor
          rows={body.data as Pair[]}
          onChange={(rows) => onChange({ mode: "urlEncoded", data: rows as KeyValue[] })}
        />
      );

    case "formData":
      return <FormDataEditor fields={body.data} onChange={(f) => onChange({ mode: "formData", data: f })} />;

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
      return (
        <div className="flex flex-col gap-2">
          <span className="text-xs text-slate-500 dark:text-slate-400">Query</span>
          <LazyCodeEditor
            language="graphql"
            height="150px"
            value={body.data.query}
            onChange={(v) =>
              onChange({ mode: "graphql", data: { ...body.data, query: v ?? "" } })
            }
          />
          <span className="text-xs text-slate-500 dark:text-slate-400">Variables (JSON)</span>
          <LazyCodeEditor
            language="json"
            height="90px"
            value={body.data.variables ?? ""}
            onChange={(v) =>
              onChange({ mode: "graphql", data: { ...body.data, variables: v ?? "" } })
            }
          />
        </div>
      );
  }
}

function monacoLang(lang?: string | null): string {
  if (!lang || lang === "text") return "plaintext";
  return lang;
}

function FormDataEditor({
  fields,
  onChange,
}: {
  fields: FormField[];
  onChange: (fields: FormField[]) => void;
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
          <input
            value={f.value}
            onChange={(e) => update(i, { value: e.target.value })}
            placeholder={f.isFile ? "/path/to/file" : "Value"}
            className={cell}
          />
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
