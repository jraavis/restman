//! Live code-generation preview: pick a language, toggle auth/headers
//! inclusion, copy or download the rendered snippet. Reuses the same
//! `req`/`workspaceId`/`collectionId`/`requestId` shape `useSend` already
//! sends to `send_request` — the backend resolves auth from the DB either
//! way, so there's nothing extra to plumb in from the auth tab's local state.

import { useState, type ReactNode } from "react";
import { Copy, Download } from "lucide-react";
import { useQuery } from "@tanstack/react-query";
import { LazyCodeEditor } from "../../components/LazyCodeEditor";
import { Switch } from "../../components/Switch";
import { ipc } from "../../lib/ipc";
import type { HttpRequest } from "../../lib/http";
import { CODE_LANGUAGES, defaultCodegenOptions, type CodeLanguage } from "../../lib/types";

const FILE_EXTENSIONS: Record<CodeLanguage, string> = {
  curl: "sh",
  javascript_fetch: "js",
  python: "py",
  go: "go",
  rust: "rs",
  php: "php",
  java: "java",
  csharp: "cs",
  ruby: "rb",
};

export function CodeTab({
  request,
  workspaceId,
  collectionId,
  requestId,
}: {
  request: HttpRequest;
  workspaceId: string | undefined;
  collectionId: string | null;
  requestId: string | null;
}) {
  const [language, setLanguage] = useState<CodeLanguage>("curl");
  const [options, setOptions] = useState(defaultCodegenOptions());

  const { data: code, error } = useQuery({
    queryKey: ["codegen", workspaceId, collectionId, requestId, language, options, request],
    queryFn: () => ipc.generateCode(request, workspaceId as string, collectionId, requestId, language, options),
    enabled: !!workspaceId && request.url.trim() !== "",
    placeholderData: (prev) => prev,
  });

  const active = CODE_LANGUAGES.find((l) => l.value === language) ?? CODE_LANGUAGES[0];
  const text = code ?? "";

  const copy = () => void navigator.clipboard.writeText(text);
  const download = () => {
    const blob = new Blob([text], { type: "text/plain" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `request.${FILE_EXTENSIONS[language]}`;
    a.click();
    URL.revokeObjectURL(url);
  };

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center gap-3 px-3 py-2">
        <select
          value={language}
          onChange={(e) => setLanguage(e.target.value as CodeLanguage)}
          className="rounded-lg border border-slate-200 bg-transparent px-2 py-1 text-xs focus:outline-none focus:ring-2 focus:ring-accent/40 dark:border-slate-700"
        >
          {CODE_LANGUAGES.map((l) => (
            <option key={l.value} value={l.value} className="text-slate-900">
              {l.label}
            </option>
          ))}
        </select>
        <Switch
          checked={options.includeAuth}
          onChange={(v) => setOptions((o) => ({ ...o, includeAuth: v }))}
          label="Auth"
        />
        <Switch
          checked={options.includeHeaders}
          onChange={(v) => setOptions((o) => ({ ...o, includeHeaders: v }))}
          label="Headers"
        />
        <span className="ml-auto flex gap-1">
          <IconButton title="Copy" onClick={copy} disabled={!text}>
            <Copy size={14} />
          </IconButton>
          <IconButton title="Download" onClick={download} disabled={!text}>
            <Download size={14} />
          </IconButton>
        </span>
      </div>

      {options.includeAuth && (
        <div className="-mt-1 px-3 pb-1.5 text-[11px] text-slate-400">
          Auth reflects the saved request/collection config, not unsaved edits in the Auth tab.
        </div>
      )}

      <div className="min-h-0 flex-1">
        {request.url.trim() === "" ? (
          <div className="flex h-full items-center justify-center p-4 text-sm text-slate-400">
            Enter a URL to preview generated code.
          </div>
        ) : error ? (
          <div className="p-3 text-sm text-red-500">{String(error)}</div>
        ) : (
          <LazyCodeEditor
            language={active.monacoLanguage}
            value={text}
            options={{ readOnly: true, minimap: { enabled: false }, scrollBeyondLastLine: false }}
            height="100%"
          />
        )}
      </div>
    </div>
  );
}

function IconButton({
  children,
  title,
  onClick,
  disabled,
}: {
  children: ReactNode;
  title: string;
  onClick: () => void;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      title={title}
      disabled={disabled}
      onClick={onClick}
      className="flex h-7 w-7 items-center justify-center rounded-md text-slate-500 hover:bg-slate-100 disabled:opacity-40 dark:text-slate-400 dark:hover:bg-slate-800"
    >
      {children}
    </button>
  );
}
