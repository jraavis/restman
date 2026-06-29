//! Import a collection or environment from an external format: paste/upload →
//! preview (parse only, no DB writes) → pick conflict mode → confirm (the
//! actual DB write) → report. Mirrors `interop::{parse, apply_import}` and
//! `interop::environment::{parse, apply_environment_import}`'s own
//! preview/apply split on the Rust side.

import { useState, type ChangeEvent } from "react";
import { AlertTriangle, CheckCircle2, ChevronRight, File, Folder, Upload } from "lucide-react";
import { ipc } from "../../lib/ipc";
import { usePlugins } from "../plugins/hooks";
import type {
  ConflictMode,
  EnvironmentImportReport,
  EnvironmentPreview,
  ImportFormat,
  ImportedNode,
  ImportPreview,
  ImportReport,
} from "../../lib/types";

/** Mutually exclusive native-format vs. plugin-id selector for `previewImport`
 * — same shape `ipc.previewImport`'s `source` param expects. */
type ImportSource = { kind: "native"; format: ImportFormat } | { kind: "plugin"; pluginId: string };

function sourceToOptionValue(source: ImportSource): string {
  return source.kind === "native" ? `native:${source.format}` : `plugin:${source.pluginId}`;
}
function optionValueToSource(value: string): ImportSource {
  if (value.startsWith("plugin:")) return { kind: "plugin", pluginId: value.slice("plugin:".length) };
  return { kind: "native", format: value.slice("native:".length) as ImportFormat };
}

interface ImportDialogProps {
  workspaceId: string;
  parentId: string | null;
  onClose: () => void;
  /** When opened from the Environments panel, preselect "environment". */
  defaultKind?: Kind;
}

type Kind = "collection" | "environment";

type Step =
  | { phase: "input" }
  | { phase: "preview"; preview: ImportPreview }
  | { phase: "env_preview"; preview: EnvironmentPreview }
  | { phase: "done"; report: ImportReport }
  | { phase: "env_done"; report: EnvironmentImportReport };

// Per-format UI hints. `accept` filters the file picker; `placeholder` is the
// paste textarea hint; `blurb` is the one-line description under the format
// select.
const COLLECTION_FORMATS: { value: ImportFormat; label: string; accept: string; placeholder: string; blurb: string }[] = [
  {
    value: "postman",
    label: "Postman Collection v2.1",
    accept: ".json,application/json",
    placeholder: "…or paste collection JSON here",
    blurb: "Import a Postman Collection v2.1 JSON export. Choose a file or paste its contents below.",
  },
  {
    value: "open_api",
    label: "OpenAPI 3.0 / Swagger 2.0",
    accept: ".json,.yaml,.yml,application/json,application/yaml,text/yaml",
    placeholder: "…or paste OpenAPI/Swagger JSON or YAML here",
    blurb: "Import an OpenAPI 3.0 or Swagger 2.0 document (JSON or YAML).",
  },
  {
    value: "har",
    label: "HAR (HTTP Archive)",
    accept: ".har,.json,application/json",
    placeholder: "…or paste HAR JSON here",
    blurb: "Import recorded requests from a HAR (HTTP Archive) 1.2 file.",
  },
  {
    value: "curl",
    label: "cURL command",
    accept: ".sh,.txt,text/plain",
    placeholder: "…or paste a curl command here",
    blurb: "Import a single request from a curl command. Choose a file or paste it below.",
  },
  {
    value: "insomnia",
    label: "Insomnia export",
    accept: ".json,application/json",
    placeholder: "…or paste Insomnia export JSON here",
    blurb: "Import requests from an Insomnia workspace export JSON.",
  },
  {
    value: "bruno",
    label: "Bruno (.bru) request",
    accept: ".bru,.txt,text/plain",
    placeholder: "…or paste a .bru request file here",
    blurb: "Import a single request from a Bruno .bru file. (Directory imports aren't supported here — import one .bru at a time.)",
  },
  {
    value: "http_file",
    label: ".http file (JetBrains / VS Code)",
    accept: ".http,.rest,.txt,text/plain",
    placeholder: "…or paste an .http file here",
    blurb: "Import requests from a JetBrains / VS Code REST Client .http file.",
  },
];

export function ImportDialog({ workspaceId, parentId, onClose, defaultKind = "collection" }: ImportDialogProps) {
  const [kind, setKind] = useState<Kind>(defaultKind);
  const [step, setStep] = useState<Step>({ phase: "input" });
  const [source, setSource] = useState<ImportSource>({ kind: "native", format: "postman" });
  const [mode, setMode] = useState<ConflictMode>("skip");
  const [overwriteExisting, setOverwriteExisting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const { data: allImportPlugins } = usePlugins(workspaceId, "import");
  const importPlugins = allImportPlugins?.filter((p) => p.enabled);

  const activeFormat = source.kind === "native" ? COLLECTION_FORMATS.find((f) => f.value === source.format) : undefined;
  const activePlugin = source.kind === "plugin" ? importPlugins?.find((p) => p.id === source.pluginId) : undefined;

  async function loadContent(content: string) {
    if (!content.trim()) return;
    setError(null);
    setBusy(true);
    try {
      if (kind === "environment") {
        const preview = await ipc.previewEnvironmentImport(content);
        setStep({ phase: "env_preview", preview });
      } else {
        const preview = await ipc.previewImport(
          content,
          source.kind === "native" ? { format: source.format } : { pluginId: source.pluginId },
        );
        setStep({ phase: "preview", preview });
      }
    } catch (e) {
      setError(typeof e === "string" ? e : String(e));
    } finally {
      setBusy(false);
    }
  }

  async function onFile(e: ChangeEvent<HTMLInputElement>) {
    const file = e.target.files?.[0];
    if (!file) return;
    void loadContent(await file.text());
  }

  async function confirmImport() {
    setBusy(true);
    setError(null);
    try {
      if (step.phase === "preview") {
        const report = await ipc.applyCollectionImport(workspaceId, parentId, step.preview.root, mode);
        setStep({ phase: "done", report });
      } else if (step.phase === "env_preview") {
        const report = await ipc.applyEnvironmentImport(workspaceId, parentId, step.preview, overwriteExisting);
        setStep({ phase: "env_done", report });
      }
    } catch (e) {
      setError(typeof e === "string" ? e : String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div onClick={(e) => e.stopPropagation()} className="fixed inset-0 z-50 flex items-center justify-center bg-black/40">
      <div className="flex max-h-[80vh] w-[520px] max-w-[95vw] flex-col overflow-hidden rounded-xl border border-slate-200 bg-white text-sm shadow-2xl dark:border-slate-700 dark:bg-slate-900">
        <div className="flex items-center gap-2 border-b border-slate-200 px-4 py-3 dark:border-slate-700">
          <span className="font-semibold text-slate-800 dark:text-slate-100">
            Import {kind === "environment" ? "environment" : "collection"}
          </span>
          <div className="ml-2 flex rounded-lg border border-slate-200 text-[11px] dark:border-slate-700">
            {(["collection", "environment"] as Kind[]).map((k) => (
              <button
                key={k}
                type="button"
                onClick={() => {
                  setKind(k);
                  setStep({ phase: "input" });
                  setError(null);
                }}
                className={`px-2 py-0.5 ${kind === k ? "bg-accent text-white" : "text-slate-500 dark:text-slate-400"}`}
              >
                {k === "collection" ? "Collection" : "Environment"}
              </button>
            ))}
          </div>
          <button
            type="button"
            onClick={onClose}
            className="ml-auto rounded px-2 py-0.5 text-xs text-slate-500 hover:bg-slate-100 dark:text-slate-400 dark:hover:bg-slate-800"
          >
            Close
          </button>
        </div>

        <div className="min-h-0 flex-1 overflow-auto p-4">
          {step.phase === "input" && (
            <div className="flex flex-col gap-3">
              {kind === "collection" && (
                <>
                  <label className="flex items-center gap-2 text-xs text-slate-600 dark:text-slate-300">
                    Format
                    <select
                      value={sourceToOptionValue(source)}
                      onChange={(e) => setSource(optionValueToSource(e.target.value))}
                      className="rounded border border-slate-200 px-2 py-1 text-xs dark:border-slate-700 dark:bg-slate-800"
                    >
                      <optgroup label="Formats">
                        {COLLECTION_FORMATS.map((f) => (
                          <option key={f.value} value={`native:${f.value}`}>
                            {f.label}
                          </option>
                        ))}
                      </optgroup>
                      {importPlugins && importPlugins.length > 0 && (
                        <optgroup label="Plugins">
                          {importPlugins.map((p) => (
                            <option key={p.id} value={`plugin:${p.id}`}>
                              {p.name}
                            </option>
                          ))}
                        </optgroup>
                      )}
                    </select>
                  </label>
                  <p className="text-xs text-slate-500 dark:text-slate-400">
                    {activeFormat?.blurb ?? `Import using the "${activePlugin?.name ?? ""}" plugin.`}
                  </p>
                  <input
                    type="file"
                    accept={activeFormat?.accept ?? "*/*"}
                    onChange={(e) => void onFile(e)}
                    className="text-xs text-slate-600 dark:text-slate-300"
                  />
                  <textarea
                    placeholder={activeFormat?.placeholder ?? "…or paste content here"}
                    rows={10}
                    onBlur={(e) => void loadContent(e.target.value)}
                    className="w-full rounded border border-slate-200 px-2 py-1.5 font-mono text-xs focus:outline-none dark:border-slate-700 dark:bg-slate-800"
                  />
                </>
              )}
              {kind === "environment" && (
                <>
                  <p className="text-xs text-slate-500 dark:text-slate-400">
                    Import a Postman Environment JSON export (`name` + `values[]`, the shape Insomnia also emits).
                    Choose a file or paste below.
                  </p>
                  <input
                    type="file"
                    accept=".json,application/json"
                    onChange={(e) => void onFile(e)}
                    className="text-xs text-slate-600 dark:text-slate-300"
                  />
                  <textarea
                    placeholder="…or paste environment JSON here"
                    rows={10}
                    onBlur={(e) => void loadContent(e.target.value)}
                    className="w-full rounded border border-slate-200 px-2 py-1.5 font-mono text-xs focus:outline-none dark:border-slate-700 dark:bg-slate-800"
                  />
                </>
              )}
              {busy && <span className="text-xs text-slate-400">Parsing…</span>}
            </div>
          )}

          {step.phase === "preview" && (
            <div className="flex flex-col gap-3">
              <div className="flex items-center gap-4 text-xs text-slate-600 dark:text-slate-300">
                <span>{step.preview.stats.folders} folders</span>
                <span>{step.preview.stats.requests} requests</span>
                {step.preview.stats.warnings > 0 && (
                  <span className="flex items-center gap-1 text-amber-600 dark:text-amber-400">
                    <AlertTriangle size={12} /> {step.preview.stats.warnings} warnings
                  </span>
                )}
              </div>

              <div className="max-h-56 overflow-auto rounded border border-slate-200 p-2 dark:border-slate-700">
                <PreviewTree node={step.preview.root} root />
              </div>

              {step.preview.warnings.length > 0 && (
                <div className="max-h-28 overflow-auto rounded border border-amber-200 bg-amber-50 p-2 text-xs text-amber-700 dark:border-amber-900 dark:bg-amber-950/30 dark:text-amber-400">
                  {step.preview.warnings.map((w, i) => (
                    <div key={i}>{w}</div>
                  ))}
                </div>
              )}

              <label className="flex items-center gap-2 text-xs text-slate-600 dark:text-slate-300">
                On name conflict
                <select
                  value={mode}
                  onChange={(e) => setMode(e.target.value as ConflictMode)}
                  className="rounded border border-slate-200 px-2 py-1 text-xs dark:border-slate-700 dark:bg-slate-800"
                >
                  <option value="skip">Skip existing</option>
                  <option value="overwrite">Overwrite existing</option>
                  <option value="merge">Keep both</option>
                </select>
              </label>
            </div>
          )}

          {step.phase === "env_preview" && (
            <div className="flex flex-col gap-3">
              <div className="flex items-center gap-4 text-xs text-slate-600 dark:text-slate-300">
                <span className="font-medium">{step.preview.name}</span>
                <span>{step.preview.variables.length} variables</span>
                {step.preview.warnings.length > 0 && (
                  <span className="flex items-center gap-1 text-amber-600 dark:text-amber-400">
                    <AlertTriangle size={12} /> {step.preview.warnings.length} warnings
                  </span>
                )}
              </div>
              <div className="max-h-56 overflow-auto rounded border border-slate-200 p-2 text-xs dark:border-slate-700">
                <table className="w-full">
                  <tbody>
                    {step.preview.variables.map((v, i) => (
                      <tr key={i} className={v.enabled ? "" : "opacity-50"}>
                        <td className="py-0.5 pr-2 font-mono text-slate-700 dark:text-slate-300">{v.key}</td>
                        <td className="py-0.5 pr-2 font-mono text-slate-500 dark:text-slate-400">
                          {v.isSecret ? <span className="italic text-amber-600">secret</span> : v.value || <span className="italic text-slate-400">(empty)</span>}
                        </td>
                        <td className="py-0.5 text-right text-[10px] uppercase text-slate-400">
                          {v.enabled ? "" : "disabled"}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
              {step.preview.warnings.length > 0 && (
                <div className="max-h-28 overflow-auto rounded border border-amber-200 bg-amber-50 p-2 text-xs text-amber-700 dark:border-amber-900 dark:bg-amber-950/30 dark:text-amber-400">
                  {step.preview.warnings.map((w, i) => (
                    <div key={i}>{w}</div>
                  ))}
                </div>
              )}
              <label className="flex items-center gap-2 text-xs text-slate-600 dark:text-slate-300">
                <input
                  type="checkbox"
                  checked={overwriteExisting}
                  onChange={(e) => setOverwriteExisting(e.target.checked)}
                />
                Overwrite same-key variables in the new environment
              </label>
            </div>
          )}

          {step.phase === "done" && (
            <div className="flex flex-col gap-2 text-xs text-slate-600 dark:text-slate-300">
              <span className="flex items-center gap-1.5 font-semibold text-emerald-600 dark:text-emerald-400">
                <CheckCircle2 size={14} /> Import complete
              </span>
              <span>{step.report.createdCollections} folders created</span>
              <span>{step.report.createdRequests} requests created</span>
              {step.report.skipped > 0 && <span>{step.report.skipped} skipped (already existed)</span>}
              {step.report.overwritten > 0 && <span>{step.report.overwritten} overwritten</span>}
              {step.report.warnings.length > 0 && (
                <div className="mt-1 max-h-28 overflow-auto rounded border border-amber-200 bg-amber-50 p-2 text-amber-700 dark:border-amber-900 dark:bg-amber-950/30 dark:text-amber-400">
                  {step.report.warnings.map((w, i) => (
                    <div key={i}>{w}</div>
                  ))}
                </div>
              )}
            </div>
          )}

          {step.phase === "env_done" && (
            <div className="flex flex-col gap-2 text-xs text-slate-600 dark:text-slate-300">
              <span className="flex items-center gap-1.5 font-semibold text-emerald-600 dark:text-emerald-400">
                <CheckCircle2 size={14} /> Environment imported
              </span>
              <span>{step.report.createdVariables} variables created</span>
              {step.report.overwritten > 0 && <span>{step.report.overwritten} overwritten</span>}
              {step.report.warnings.length > 0 && (
                <div className="mt-1 max-h-28 overflow-auto rounded border border-amber-200 bg-amber-50 p-2 text-amber-700 dark:border-amber-900 dark:bg-amber-950/30 dark:text-amber-400">
                  {step.report.warnings.map((w, i) => (
                    <div key={i}>{w}</div>
                  ))}
                </div>
              )}
            </div>
          )}

          {error && <div className="mt-2 text-xs text-red-500">{error}</div>}
        </div>

        <div className="flex items-center justify-end gap-2 border-t border-slate-200 px-4 py-3 dark:border-slate-700">
          {(step.phase === "preview" || step.phase === "env_preview") && (
            <button
              type="button"
              disabled={busy}
              onClick={() => void confirmImport()}
              className="flex items-center gap-1.5 rounded-lg bg-accent px-3 py-1.5 text-xs font-semibold text-white hover:bg-accent-hover disabled:opacity-50"
            >
              <Upload size={12} /> Import
            </button>
          )}
          {(step.phase === "done" || step.phase === "env_done") && (
            <button
              type="button"
              onClick={onClose}
              className="rounded-lg bg-accent px-3 py-1.5 text-xs font-semibold text-white hover:bg-accent-hover"
            >
              Done
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

function PreviewTree({ node, root = false }: { node: ImportedNode; root?: boolean }) {
  const [expanded, setExpanded] = useState(true);
  return (
    <div className={root ? "" : "ml-3.5 border-l border-slate-100 pl-2 dark:border-slate-800"}>
      <div
        onClick={() => setExpanded((e) => !e)}
        className="flex cursor-pointer items-center gap-1 py-0.5 text-xs text-slate-700 dark:text-slate-200"
      >
        <ChevronRight size={11} className={`shrink-0 transition-transform ${expanded ? "rotate-90" : ""}`} />
        <Folder size={11} className="shrink-0 text-slate-400" />
        <span className="truncate font-medium">{node.name}</span>
      </div>
      {expanded && (
        <div className="ml-3.5">
          {node.requests.map((r, i) => (
            <div key={i} className="flex items-center gap-1.5 py-0.5 text-xs text-slate-500 dark:text-slate-400">
              <File size={11} className="shrink-0 text-slate-300 dark:text-slate-600" />
              <span className="shrink-0 font-mono text-[10px] uppercase text-slate-400">{r.method}</span>
              <span className="truncate">{r.name}</span>
            </div>
          ))}
          {node.children.map((child, i) => (
            <PreviewTree key={i} node={child} />
          ))}
        </div>
      )}
    </div>
  );
}
