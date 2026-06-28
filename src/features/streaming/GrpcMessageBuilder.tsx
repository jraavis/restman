//! `GrpcMessageBuilder` — an editable form (or JSON editor) for a gRPC
//! request message, driven by a `GrpcMethodDescriptor`'s `inputFields`.
//! Purely presentational + local state: no TanStack Query, no real `invoke`.
//! The "Invoke" button emits the current message as a JSON string via
//! `onSend`. Mock IPC lives in `grpcMessageIpc.ts` (#33 swaps in the real
//! backend wrapper).

import { useMemo, useState } from "react";
import type { GrpcFieldDescriptor, GrpcMethodDescriptor } from "./grpcSchemaTypes";

export interface GrpcMessageBuilderProps {
  /** The selected method to build a request for. */
  method: GrpcMethodDescriptor;
  /** Emits the current message as a JSON string. */
  onSend: (requestJson: string) => void;
  /** Disable the Send button (e.g. not connected yet). */
  sendDisabled?: boolean;
  /** Override the Send button label. Defaults to "Invoke". */
  sendLabel?: string;
}

type Mode = "form" | "json";

const INTEGER_TYPES = new Set([
  "int32",
  "int64",
  "uint32",
  "uint64",
  "sint32",
  "sint64",
  "fixed32",
  "fixed64",
  "sfixed32",
  "sfixed64",
]);
const FLOAT_TYPES = new Set(["float", "double"]);

/** Build an empty initial message object from the descriptor's input fields. */
function emptyMessageFor(fields: GrpcFieldDescriptor[]): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const f of fields) {
    out[f.name] = f.repeated ? [] : "";
  }
  return out;
}

/** Format a single scalar form value into the JSON-ready representation. */
function scalarFromForm(field: GrpcFieldDescriptor, raw: string): unknown {
  if (field.type === "bool") return raw === "true";
  if (INTEGER_TYPES.has(field.type)) {
    const n = Number(raw);
    return Number.isFinite(n) && raw.trim() !== "" ? n : 0;
  }
  if (FLOAT_TYPES.has(field.type)) {
    const n = Number(raw);
    return Number.isFinite(n) && raw.trim() !== "" ? n : 0;
  }
  return raw;
}

export function GrpcMessageBuilder({
  method,
  onSend,
  sendDisabled = false,
  sendLabel = "Invoke",
}: GrpcMessageBuilderProps) {
  const [mode, setMode] = useState<Mode>("form");
  const [values, setValues] = useState<Record<string, unknown>>(() =>
    emptyMessageFor(method.inputFields),
  );
  // Raw JSON textarea text + error state, kept separate so a parse failure
  // never crashes the form state.
  const [jsonText, setJsonText] = useState<string>(() => JSON.stringify(values, null, 2));
  const [jsonError, setJsonError] = useState<string | null>(null);

  // Re-sync the JSON textarea whenever the editing mode flips to JSON, so
  // the user always sees the latest form values reflected in the JSON.
  // (Form→JSON push; JSON→Form pull happens on a successful parse.)
  function switchMode(next: Mode) {
    if (next === mode) return;
    if (next === "json") {
      setJsonText(JSON.stringify(values, null, 2));
      setJsonError(null);
    } else {
      // JSON → Form: try to reconcile from the textarea. On parse failure,
      // keep the form's prior values (don't crash) and bubble up an error
      // hint — but still flip to form mode so the user can fix via the form.
      try {
        const parsed = JSON.parse(jsonText);
        if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
          setValues({ ...values, ...parsed });
          setJsonError(null);
        } else {
          setJsonError("JSON must be an object (not an array or primitive).");
        }
      } catch {
        setJsonError("Invalid JSON — form kept its previous values.");
      }
    }
    setMode(next);
  }

  function setField(name: string, value: unknown) {
    setValues((prev) => ({ ...prev, [name]: value }));
  }

  // For repeated text fields we render a textarea (one value per line) and
  // join lines into a JSON array on emit; repeated bool/number/scalars reuse
  // the same textarea — parsing per line into the right scalar shape.
  function handleRepeatedLines(field: GrpcFieldDescriptor, text: string) {
    const lines = text.split("\n").map((l) => l.trimEnd()).filter((l) => l.trim() !== "");
    const arr = lines.map((l) => scalarFromForm(field, l));
    setField(field.name, arr);
  }

  const invoke = () => {
    // Always emit the canonical, parsed state — never the raw textarea text —
    // so a stale JSON parse error can't leak broken JSON to the caller.
    onSend(JSON.stringify(values, null, 2));
  };

  const jsonTextarea = useMemo(
    () => (
      <textarea
        data-testid="grpc-json-editor"
        value={jsonText}
        onChange={(e) => {
          setJsonText(e.target.value);
          // Clear a stale error as soon as the user starts editing; re-eval
          // happens on the next mode switch.
          if (jsonError) setJsonError(null);
        }}
        spellCheck={false}
        rows={10}
        className="w-full resize-none rounded-md border border-slate-200 bg-transparent px-2.5 py-1.5 font-mono text-xs focus:outline-none focus:ring-2 focus:ring-accent/40 dark:border-slate-700"
      />
    ),
    [jsonText, jsonError],
  );

  return (
    <div className="flex flex-col gap-3">
      <div className="flex items-center justify-between">
        <div className="text-xs text-slate-500 dark:text-slate-400">
          <span className="font-mono">{method.fullName}</span>{" "}
          <span className="rounded border border-slate-200 px-1 py-0.5 text-slate-500 dark:border-slate-700">
            {method.streamingType}
          </span>
        </div>
        {/* Segmented Form/JSON toggle — mirrors WsPanel's small bordered badge style. */}
        <div className="inline-flex overflow-hidden rounded-md border border-slate-200 text-xs dark:border-slate-700">
          <button
            type="button"
            onClick={() => switchMode("form")}
            className={
              "px-2 py-1 " +
              (mode === "form"
                ? "bg-accent text-white"
                : "bg-transparent text-slate-600 hover:bg-slate-100 dark:text-slate-300 dark:hover:bg-slate-700")
            }
          >
            Form
          </button>
          <button
            type="button"
            onClick={() => switchMode("json")}
            className={
              "px-2 py-1 " +
              (mode === "json"
                ? "bg-accent text-white"
                : "bg-transparent text-slate-600 hover:bg-slate-100 dark:text-slate-300 dark:hover:bg-slate-700")
            }
          >
            JSON
          </button>
        </div>
      </div>

      {mode === "form" ? (
        method.inputFields.length === 0 ? (
          <p className="rounded-md border border-slate-100 bg-slate-50 px-3 py-2 text-xs text-slate-500 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-400">
            This method takes no request fields.
          </p>
        ) : (
          <div className="flex flex-col gap-2">
            {method.inputFields.map((field) => (
              <FieldRow
                key={field.name}
                field={field}
                value={values[field.name]}
                onChangeScalar={(v) => setField(field.name, v)}
                onChangeRepeatedText={(text) => handleRepeatedLines(field, text)}
              />
            ))}
          </div>
        )
      ) : (
        <div className="flex flex-col gap-1">
          {jsonTextarea}
          {jsonError && (
            <p className="rounded-md bg-red-50 px-2 py-1 text-xs text-red-600 dark:bg-red-900/30 dark:text-red-400">
              {jsonError}
            </p>
          )}
        </div>
      )}

      <div className="flex justify-end">
        <button
          type="button"
          disabled={sendDisabled}
          onClick={invoke}
          className="rounded-md bg-accent px-3 py-1.5 text-sm font-medium text-white disabled:opacity-40"
        >
          {sendLabel}
        </button>
      </div>
    </div>
  );
}

function FieldRow({
  field,
  value,
  onChangeScalar,
  onChangeRepeatedText,
}: {
  field: GrpcFieldDescriptor;
  value: unknown;
  onChangeScalar: (v: string) => void;
  onChangeRepeatedText: (text: string) => void;
}) {
  // Repeated fields always get a textarea (one per line), regardless of the
  // underlying scalar type — parsing into the right shape happens in the
  // parent's `handleRepeatedLines`.
  if (field.repeated) {
    const text = Array.isArray(value)
      ? (value as unknown[]).map((v) => String(v)).join("\n")
      : "";
    return (
      <label className="flex flex-col gap-1 text-xs">
        <span className="flex items-center gap-1.5 text-slate-600 dark:text-slate-300">
          {field.name}
          <TypeBadge field={field} />
        </span>
        <textarea
          data-testid={`grpc-field-${field.name}`}
          value={text}
          onChange={(e) => onChangeRepeatedText(e.target.value)}
          rows={2}
          spellCheck={false}
          placeholder="One value per line"
          className="w-full resize-none rounded-md border border-slate-200 bg-transparent px-2 py-1 font-mono text-xs focus:outline-none focus:ring-2 focus:ring-accent/40 dark:border-slate-700"
        />
      </label>
    );
  }

  if (field.type === "message" && field.messageTypeName) {
    // Nested sub-message — a JSON sub-editor, NOT a recursive re-render.
    // Expanding the sub-message's own fields is out of scope for this mock
    // (#32); task #33 will wire in real reflection for sub-messages.
    const text =
      typeof value === "string" && value.trim() !== "" ? value : "";
    return (
      <label className="flex flex-col gap-1 text-xs">
        <span className="flex items-center gap-1.5 text-slate-600 dark:text-slate-300">
          {field.name}
          <TypeBadge field={field} />
        </span>
        <textarea
          data-testid={`grpc-field-${field.name}`}
          value={text}
          onChange={(e) => onChangeScalar(e.target.value)}
          rows={3}
          spellCheck={false}
          placeholder={`JSON for ${field.messageTypeName}…`}
          className="w-full resize-none rounded-md border border-slate-200 bg-transparent px-2 py-1 font-mono text-xs focus:outline-none focus:ring-2 focus:ring-accent/40 dark:border-slate-700"
        />
      </label>
    );
  }

  if (field.type === "bool") {
    const checked = value === true;
    return (
      <label className="flex items-center gap-2 text-xs text-slate-600 dark:text-slate-300">
        <input
          type="checkbox"
          data-testid={`grpc-field-${field.name}`}
          checked={checked}
          onChange={(e) => onChangeScalar(e.target.checked ? "true" : "false")}
          className="h-3.5 w-3.5"
        />
        <span className="flex items-center gap-1.5">
          {field.name}
          <TypeBadge field={field} />
        </span>
      </label>
    );
  }

  const numeric = INTEGER_TYPES.has(field.type) || FLOAT_TYPES.has(field.type);
  const text = typeof value === "string" ? value : value == null ? "" : String(value);

  return (
    <label className="flex flex-col gap-1 text-xs">
      <span className="flex items-center gap-1.5 text-slate-600 dark:text-slate-300">
        {field.name}
        <TypeBadge field={field} />
      </span>
      <input
        type={numeric ? "number" : "text"}
        step={FLOAT_TYPES.has(field.type) ? "any" : undefined}
        data-testid={`grpc-field-${field.name}`}
        value={text}
        onChange={(e) => onChangeScalar(e.target.value)}
        spellCheck={false}
        className="w-full rounded-md border border-slate-200 bg-transparent px-2 py-1 font-mono text-xs focus:outline-none focus:ring-2 focus:ring-accent/40 dark:border-slate-700"
      />
      {field.type === "bytes" && (
        <span className="text-[10px] text-slate-400">base64-encoded</span>
      )}
    </label>
  );
}

/** Small bordered type badge — reuses WsPanel's `binary` badge styling. */
function TypeBadge({ field }: { field: GrpcFieldDescriptor }) {
  return (
    <span className="rounded border border-slate-200 px-1 py-0.5 text-slate-500 dark:border-slate-700">
      {field.repeated ? `repeated ${field.type}` : field.type}
    </span>
  );
}