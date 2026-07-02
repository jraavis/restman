//! Response viewer: status line, body (Pretty/Raw/Preview/Hex), headers, timing.

import { useMemo, useState, type ReactNode } from "react";
import type { editor as MonacoEditor } from "monaco-editor";
import { save } from "@tauri-apps/plugin-dialog";
import {
  AlertCircle,
  AlertTriangle,
  ArrowRightCircle,
  CheckCircle2,
  Copy,
  Download,
  HelpCircle,
  Loader2,
  Search,
  XCircle,
} from "lucide-react";
import { LazyCodeEditor } from "../../components/LazyCodeEditor";
import {
  base64ToBytes,
  bytesToText,
  filterJsonValue,
  filterLines,
  formatBytes,
  formatHex,
  formatMs,
  prettyJson,
  prettyXml,
} from "../../lib/encoding";
import { statusColor, statusIconName } from "../../lib/methods";
import { contentTypeOf, extensionFor, monacoLanguageFor, type HttpResponse } from "../../lib/http";
import { ipc } from "../../lib/ipc";
import { useRequestStore } from "../../stores/requestStore";
import { useActiveWorkspace } from "../workspaces/hooks";
import { TestResultsPanel } from "./TestResultsPanel";
import { SetVariableDialog } from "./SetVariableDialog";
import { jsonPathAtOffset, stripJsonStringQuotes, type JsonPath } from "./jsonPath";

type BodyView = "pretty" | "raw" | "preview" | "hex";
type Tab = "body" | "headers" | "timing" | "tests";

const STATUS_ICONS = {
  "check-circle-2": CheckCircle2,
  "arrow-right-circle": ArrowRightCircle,
  "alert-triangle": AlertTriangle,
  "x-circle": XCircle,
  "help-circle": HelpCircle,
} as const;

export function ResponseViewer() {
  const response = useRequestStore((s) => s.response);
  const preScript = useRequestStore((s) => s.preScript);
  const postScript = useRequestStore((s) => s.postScript);
  const sending = useRequestStore((s) => s.sending);
  const error = useRequestStore((s) => s.error);

  if (sending)
    return (
      <Centered>
        <Loader2 size={18} className="animate-spin text-accent" />
        Sending request…
      </Centered>
    );
  if (error)
    return (
      <Centered className="text-red-500">
        <AlertCircle size={18} />
        {error}
      </Centered>
    );
  if (!response)
    return (
      <Centered>
        <ArrowRightCircle size={18} className="text-slate-300 dark:text-slate-600" />
        Send a request to see the response.
      </Centered>
    );

  return <ResponseBody response={response} preScript={preScript} postScript={postScript} />;
}

function ResponseBody({
  response,
  preScript,
  postScript,
}: {
  response: HttpResponse;
  preScript: import("../../lib/types").ScriptResult | null;
  postScript: import("../../lib/types").ScriptResult | null;
}) {
  const { data: workspace } = useActiveWorkspace();
  const collectionId = useRequestStore((s) => s.collectionId);
  const [tab, setTab] = useState<Tab>("body");
  const [view, setView] = useState<BodyView>("pretty");
  const [wrap, setWrap] = useState(true);
  const [filter, setFilter] = useState("");
  const [setVarDialog, setSetVarDialog] = useState<{
    value: string;
    jsonPath: JsonPath | null;
  } | null>(null);

  const bytes = useMemo(() => base64ToBytes(response.bodyBase64), [response.bodyBase64]);
  const text = useMemo(() => bytesToText(bytes), [bytes]);
  const contentType = useMemo(() => contentTypeOf(response.headers), [response.headers]);
  const prettyJsonText = useMemo(() => prettyJson(text), [text]);
  const prettyXmlText = useMemo(() => (prettyJsonText ? null : prettyXml(text)), [text, prettyJsonText]);
  const pretty = prettyJsonText ?? prettyXmlText;
  const prettyLang = prettyJsonText ? "json" : prettyXmlText ? "xml" : monacoLanguageFor(contentType);
  const rawLang = monacoLanguageFor(contentType);

  const filteredPretty = useMemo(() => {
    const base = pretty ?? text;
    if (!filter.trim()) return base;
    if (prettyJsonText) {
      try {
        return JSON.stringify(filterJsonValue(JSON.parse(text), filter) ?? {}, null, 2);
      } catch {
        // fall through to line filter
      }
    }
    return filterLines(base, filter);
  }, [pretty, text, filter, prettyJsonText]);
  const filteredRaw = useMemo(() => filterLines(text, filter), [text, filter]);

  const StatusIcon = STATUS_ICONS[statusIconName(response.status)];

  const openSetVariable = (rawValue: string, offset: number | null, sourceText: string) => {
    const value = stripJsonStringQuotes(rawValue);
    const baseText = pretty ?? text;
    const jsonPath =
      offset != null && prettyJsonText && !filter.trim() && sourceText === baseText
        ? jsonPathAtOffset(baseText, offset)
        : null;
    setSetVarDialog({ value, jsonPath });
  };

  const copy = () => void navigator.clipboard.writeText(text);
  const download = async () => {
    const path = await save({ defaultPath: `response.${extensionFor(contentType)}` });
    if (!path) return;
    try {
      await ipc.writeFileBytes(path, response.bodyBase64);
    } catch (e) {
      console.error("failed to save response body:", e);
    }
  };

  return (
    <div className="flex h-full flex-col bg-white dark:bg-slate-900">
      {/* status line */}
      <div className="flex flex-wrap items-center gap-3 border-b border-slate-200 px-3 py-2 text-sm dark:border-slate-800">
        <span
          className={
            "flex items-center gap-1.5 rounded-full px-2.5 py-0.5 font-semibold " +
            statusColor(response.status)
          }
        >
          <StatusIcon size={13} />
          {response.status} {response.statusText}
        </span>
        <span className="text-slate-500 dark:text-slate-400">{formatMs(response.timing.totalMs)}</span>
        <span className="text-slate-500 dark:text-slate-400">{formatBytes(response.sizeBytes)}</span>
        <span className="text-slate-400 dark:text-slate-500">{response.httpVersion}</span>
        <span className="ml-auto flex gap-1">
          <IconButton title="Copy body" onClick={copy}>
            <Copy size={14} />
          </IconButton>
          <IconButton title="Download body" onClick={download}>
            <Download size={14} />
          </IconButton>
        </span>
      </div>

      {/* tabs */}
      <nav className="flex gap-1 border-b border-slate-100 px-3 dark:border-slate-800">
        {(["body", "headers", "timing", "tests"] as Tab[]).map((t) => {
          const hasTests =
            (preScript?.tests.length ?? 0) > 0 ||
            (postScript?.tests.length ?? 0) > 0 ||
            !!preScript?.error ||
            !!postScript?.error;
          const failCount =
            (preScript?.tests.filter((x) => !x.passed).length ?? 0) +
            (postScript?.tests.filter((x) => !x.passed).length ?? 0);
          return (
            <button
              key={t}
              type="button"
              onClick={() => setTab(t)}
              className={
                "border-b-2 px-3 py-2 text-xs font-medium capitalize transition-colors " +
                (tab === t
                  ? "border-accent text-accent"
                  : "border-transparent text-slate-500 hover:text-slate-800 dark:hover:text-slate-200")
              }
            >
              {t}
              {t === "headers" ? ` (${response.headers.length})` : ""}
              {t === "tests" && hasTests ? (
                <span
                  className={
                    "ml-1 rounded-full px-1.5 py-0.5 text-[10px] font-semibold " +
                    (failCount > 0
                      ? "bg-red-100 text-red-600 dark:bg-red-900/30 dark:text-red-400"
                      : "bg-emerald-100 text-emerald-600 dark:bg-emerald-900/30 dark:text-emerald-400")
                  }
                >
                  {failCount > 0 ? `${failCount} fail` : "pass"}
                </span>
              ) : null}
            </button>
          );
        })}
      </nav>

      <div className="min-h-0 flex-1 overflow-auto">
        {tab === "body" && (
          <div className="flex h-full flex-col">
            <div className="flex items-center gap-2 px-3 py-2">
              <div className="flex w-fit gap-0.5 rounded-lg bg-slate-100 p-0.5 dark:bg-slate-800">
                {(["pretty", "raw", "preview", "hex"] as BodyView[]).map((v) => (
                  <button
                    key={v}
                    type="button"
                    onClick={() => setView(v)}
                    className={
                      "rounded-md px-2.5 py-1 text-xs font-medium capitalize transition-colors " +
                      (view === v
                        ? "bg-white text-slate-900 shadow-sm dark:bg-slate-700 dark:text-white"
                        : "text-slate-500 hover:text-slate-800 dark:text-slate-400 dark:hover:text-slate-200")
                    }
                  >
                    {v}
                  </button>
                ))}
              </div>
              {(view === "pretty" || view === "raw") && (
                <div className="ml-2 flex items-center gap-1 rounded-md border border-slate-200 px-2 py-1 dark:border-slate-700">
                  <Search size={12} className="text-slate-400" />
                  <input
                    value={filter}
                    onChange={(e) => setFilter(e.target.value)}
                    placeholder="Filter body…"
                    className="w-32 bg-transparent text-xs outline-none placeholder:text-slate-400 dark:text-slate-200"
                  />
                </div>
              )}
              <button
                type="button"
                onClick={() => setWrap((w) => !w)}
                className="ml-auto rounded-md px-2 py-1 text-xs text-slate-500 hover:bg-slate-100 dark:text-slate-400 dark:hover:bg-slate-800"
                title="Toggle word wrap"
              >
                Wrap: {wrap ? "on" : "off"}
              </button>
            </div>
            <div className="min-h-0 flex-1">
              <BodyContent
                view={view}
                previewText={text}
                prettyText={filteredPretty}
                prettySourceText={pretty ?? text}
                prettyLang={prettyLang}
                rawText={filteredRaw}
                rawLang={rawLang}
                bytes={bytes}
                wrap={wrap}
                onSetVariable={openSetVariable}
              />
            </div>
          </div>
        )}

        {tab === "headers" && (
          <table className="w-full text-sm">
            <tbody>
              {response.headers.map((h, i) => (
                <tr key={i} className="border-b border-slate-100 dark:border-slate-800">
                  <td className="w-1/3 px-3 py-1.5 font-medium text-slate-600 dark:text-slate-300">
                    {h.name}
                  </td>
                  <td className="break-all px-3 py-1.5 text-slate-500 dark:text-slate-400">{h.value}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}

        {tab === "timing" && <TimingView response={response} />}

        {tab === "tests" && (
          <TestResultsPanel preScript={preScript} postScript={postScript} />
        )}
      </div>

      {setVarDialog && workspace && (
        <SetVariableDialog
          workspaceId={workspace.id}
          collectionId={collectionId}
          value={setVarDialog.value}
          jsonPath={setVarDialog.jsonPath}
          onClose={() => setSetVarDialog(null)}
        />
      )}
    </div>
  );
}

function IconButton({
  children,
  title,
  onClick,
}: {
  children: ReactNode;
  title: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      title={title}
      onClick={onClick}
      className="flex h-7 w-7 items-center justify-center rounded-md text-slate-500 hover:bg-slate-100 dark:text-slate-400 dark:hover:bg-slate-800"
    >
      {children}
    </button>
  );
}

function mountSetVariableAction(
  editor: MonacoEditor.IStandaloneCodeEditor,
  sourceText: string,
  onSetVariable: (value: string, offset: number | null, sourceText: string) => void,
) {
  editor.addAction({
    id: "restman.set-as-variable",
    label: "Set as variable value",
    precondition: "editorHasSelection",
    contextMenuGroupId: "navigation",
    contextMenuOrder: 1,
    run: (ed) => {
      const sel = ed.getSelection();
      const model = ed.getModel();
      if (!sel || !model) return;
      const selected = model.getValueInRange(sel);
      const offset = model.getOffsetAt(sel.getStartPosition());
      onSetVariable(selected, offset, sourceText);
    },
  });
}

function BodyContent({
  view,
  previewText,
  prettyText,
  prettySourceText,
  prettyLang,
  rawText,
  rawLang,
  bytes,
  wrap,
  onSetVariable,
}: {
  view: BodyView;
  previewText: string;
  prettyText: string;
  prettySourceText: string;
  prettyLang: string;
  rawText: string;
  rawLang: string;
  bytes: Uint8Array;
  wrap: boolean;
  onSetVariable: (value: string, offset: number | null, sourceText: string) => void;
}) {
  const opts = {
    readOnly: true,
    minimap: { enabled: false },
    wordWrap: wrap ? ("on" as const) : ("off" as const),
    scrollBeyondLastLine: false,
  };

  if (view === "pretty")
    return (
      <LazyCodeEditor
        language={prettyLang}
        value={prettyText}
        options={opts}
        height="100%"
        onMount={(editor) => mountSetVariableAction(editor, prettySourceText, onSetVariable)}
      />
    );
  if (view === "raw")
    return (
      <LazyCodeEditor
        language={rawLang}
        value={rawText}
        options={opts}
        height="100%"
        onMount={(editor) => mountSetVariableAction(editor, rawText, onSetVariable)}
      />
    );
  if (view === "preview")
    return (
      <iframe
        title="preview"
        sandbox=""
        srcDoc={previewText}
        className="h-full w-full border-0 bg-white"
      />
    );
  // hex
  return (
    <pre
      className="overflow-auto p-3 font-mono text-xs text-slate-600 dark:text-slate-300"
      onContextMenu={(e) => {
        const sel = window.getSelection()?.toString().trim();
        if (sel) {
          e.preventDefault();
          onSetVariable(sel, null, "");
        }
      }}
    >
      {formatHex(bytes)}
    </pre>
  );
}

function TimingView({ response }: { response: HttpResponse }) {
  const t = response.timing;
  const rows: [string, number | null][] = [
    ["DNS lookup", t.dnsMs],
    ["TCP connect", t.connectMs],
    ["TLS handshake", t.tlsMs],
    ["Time to first byte", t.ttfbMs],
    ["Content download", t.downloadMs],
    ["Total", t.totalMs],
  ];
  return (
    <table className="w-full text-sm">
      <tbody>
        {rows.map(([label, ms]) => (
          <tr key={label} className="border-b border-slate-100 dark:border-slate-800">
            <td className="px-3 py-1.5 text-slate-600 dark:text-slate-300">{label}</td>
            <td className="px-3 py-1.5 text-right text-slate-500 dark:text-slate-400">
              {ms == null ? "—" : formatMs(ms)}
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function Centered({
  children,
  className = "text-slate-400",
}: {
  children: ReactNode;
  className?: string;
}) {
  return (
    <div
      className={
        "flex h-full flex-col items-center justify-center gap-2 p-4 text-sm " +
        className
      }
    >
      {children}
    </div>
  );
}
