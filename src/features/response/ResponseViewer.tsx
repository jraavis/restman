//! Response viewer: status line, body (Pretty/Raw/Preview/Hex), headers, timing.

import { useMemo, useState, type ReactNode } from "react";
import {
  AlertCircle,
  AlertTriangle,
  ArrowRightCircle,
  CheckCircle2,
  Copy,
  Download,
  HelpCircle,
  Loader2,
  XCircle,
} from "lucide-react";
import { LazyCodeEditor } from "../../components/LazyCodeEditor";
import {
  base64ToBytes,
  bytesToText,
  formatBytes,
  formatHex,
  formatMs,
  prettyJson,
} from "../../lib/encoding";
import { statusColor, statusIconName } from "../../lib/methods";
import type { HttpResponse } from "../../lib/http";
import { useRequestStore } from "../../stores/requestStore";

type BodyView = "pretty" | "raw" | "preview" | "hex";
type Tab = "body" | "headers" | "timing";

const STATUS_ICONS = {
  "check-circle-2": CheckCircle2,
  "arrow-right-circle": ArrowRightCircle,
  "alert-triangle": AlertTriangle,
  "x-circle": XCircle,
  "help-circle": HelpCircle,
} as const;

export function ResponseViewer() {
  const response = useRequestStore((s) => s.response);
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

  return <ResponseBody response={response} />;
}

function ResponseBody({ response }: { response: HttpResponse }) {
  const [tab, setTab] = useState<Tab>("body");
  const [view, setView] = useState<BodyView>("pretty");
  const [wrap, setWrap] = useState(true);

  const bytes = useMemo(() => base64ToBytes(response.bodyBase64), [response.bodyBase64]);
  const text = useMemo(() => bytesToText(bytes), [bytes]);
  const pretty = useMemo(() => prettyJson(text), [text]);
  const StatusIcon = STATUS_ICONS[statusIconName(response.status)];

  const copy = () => void navigator.clipboard.writeText(text);
  const download = () => {
    const blob = new Blob([bytes as BlobPart], { type: "application/octet-stream" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = "response";
    a.click();
    URL.revokeObjectURL(url);
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
        {(["body", "headers", "timing"] as Tab[]).map((t) => (
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
          </button>
        ))}
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
              <BodyContent view={view} text={text} pretty={pretty} bytes={bytes} wrap={wrap} />
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
      </div>
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

function BodyContent({
  view,
  text,
  pretty,
  bytes,
  wrap,
}: {
  view: BodyView;
  text: string;
  pretty: string | null;
  bytes: Uint8Array;
  wrap: boolean;
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
        language={pretty ? "json" : "plaintext"}
        value={pretty ?? text}
        options={opts}
        height="100%"
      />
    );
  if (view === "raw")
    return <LazyCodeEditor language="plaintext" value={text} options={opts} height="100%" />;
  if (view === "preview")
    return (
      <iframe
        title="preview"
        sandbox=""
        srcDoc={text}
        className="h-full w-full border-0 bg-white"
      />
    );
  // hex
  return (
    <pre className="overflow-auto p-3 font-mono text-xs text-slate-600 dark:text-slate-300">
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
