//! Live code-generation preview: pick a language, toggle auth/headers
//! inclusion, copy or download the rendered snippet. Reuses the same
//! `req`/`workspaceId`/`collectionId`/`requestId` shape `useSend` sends to
//! `send_request`, plus the Auth tab's live draft `RequestAuth` — the backend
//! resolves it (inheritance + keychain hydration for masked fields) so the
//! preview shows what the Auth tab currently says, saved or not.

import { useState, type ReactNode } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { Copy, Download } from "lucide-react";
import { useQuery } from "@tanstack/react-query";
import { LazyCodeEditor } from "../../components/LazyCodeEditor";
import { Switch } from "../../components/Switch";
import { useRequestStore } from "../../stores/requestStore";
import { textToBase64 } from "../../lib/encoding";
import { ipc } from "../../lib/ipc";
import type { HttpRequest } from "../../lib/http";
import { CODE_LANGUAGES, defaultCodegenOptions, type CodeLanguage, type CodegenTarget } from "../../lib/types";
import { usePlugins } from "../plugins/hooks";

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

/** Encodes a `CodegenTarget` as a single `<select>` option value (native
 * languages and plugin ids share one flat dropdown) and decodes it back. */
function targetToOptionValue(target: CodegenTarget): string {
  return target.kind === "native" ? `native:${target.language}` : `plugin:${target.pluginId}`;
}
function optionValueToTarget(value: string): CodegenTarget {
  if (value.startsWith("plugin:")) return { kind: "plugin", pluginId: value.slice("plugin:".length) };
  return { kind: "native", language: value.slice("native:".length) as CodeLanguage };
}

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
  const [target, setTarget] = useState<CodegenTarget>({ kind: "native", language: "curl" });
  const [options, setOptions] = useState(defaultCodegenOptions());
  const auth = useRequestStore((s) => s.auth);
  const { data: allPlugins } = usePlugins(workspaceId, "codegen");
  const plugins = allPlugins?.filter((p) => p.enabled);

  const { data: code, error } = useQuery({
    queryKey: ["codegen", workspaceId, collectionId, requestId, auth, target, options, request],
    queryFn: () => ipc.generateCode(request, workspaceId as string, collectionId, requestId, auth, target, options),
    enabled: !!workspaceId && request.url.trim() !== "",
    placeholderData: (prev) => prev,
  });

  const activeNative = target.kind === "native" ? CODE_LANGUAGES.find((l) => l.value === target.language) : undefined;
  const monacoLanguage = activeNative?.monacoLanguage ?? "plaintext";
  const fileExtension = activeNative ? FILE_EXTENSIONS[activeNative.value] : "txt";
  const text = code ?? "";

  const copy = () => void navigator.clipboard.writeText(text);
  const download = async () => {
    const path = await save({ defaultPath: `request.${fileExtension}` });
    if (!path) return;
    try {
      await ipc.writeFileBytes(path, textToBase64(text));
    } catch (e) {
      console.error("failed to download code snippet:", e);
    }
  };

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center gap-3 px-3 py-2">
        <select
          value={targetToOptionValue(target)}
          onChange={(e) => setTarget(optionValueToTarget(e.target.value))}
          className="rounded-lg border border-slate-200 bg-transparent px-2 py-1 text-xs focus:outline-none focus:ring-2 focus:ring-accent/40 dark:border-slate-700"
        >
          <optgroup label="Languages">
            {CODE_LANGUAGES.map((l) => (
              <option key={l.value} value={`native:${l.value}`} className="text-slate-900">
                {l.label}
              </option>
            ))}
          </optgroup>
          {plugins && plugins.length > 0 && (
            <optgroup label="Plugins">
              {plugins.map((p) => (
                <option key={p.id} value={`plugin:${p.id}`} className="text-slate-900">
                  {p.name}
                </option>
              ))}
            </optgroup>
          )}
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
          Auth reflects the Auth tab's current config (secrets come from the keychain once saved).
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
            language={monacoLanguage}
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
