//! Pre-request and post-response script editors.
//! Each pane is a Monaco editor with JavaScript language mode.

import { Suspense } from "react";
import { LazyCodeEditor } from "../../components/LazyCodeEditor";

const PM_SNIPPET = `// pm.* API quick reference
// pm.environment.get("key")          → string | undefined
// pm.environment.set("key", "value")
// pm.request.method / .url / .headers.get("name")
// pm.response.status / .statusText / .json() / .text()
// pm.response.headers.get("name")
// pm.response.responseTime           → ms
// pm.test("name", () => { pm.expect(x).to.equal(y); })
// pm.expect(x).to.equal(v) / .include(s) / .a("type")
//              .to.be.true / .false / .null / .undefined
//              .to.have.length(n)
// pm.abort()                         → cancel this request
// $guid  $timestamp  $randomInt      → template tags
`;

interface ScriptsTabProps {
  preScript: string;
  postScript: string;
  onPreChange: (v: string) => void;
  onPostChange: (v: string) => void;
}

export function ScriptsTab({
  preScript,
  postScript,
  onPreChange,
  onPostChange,
}: ScriptsTabProps) {
  return (
    <div className="flex h-full flex-col gap-0 overflow-hidden">
      {/* Pre-request */}
      <ScriptPane
        label="Pre-request script"
        description="Runs before the request is sent. Can mutate environment variables or call pm.abort() to cancel."
        value={preScript}
        onChange={onPreChange}
        placeholder={PM_SNIPPET}
      />

      {/* Divider */}
      <div className="h-px bg-slate-200 dark:bg-slate-700 shrink-0" />

      {/* Post-response */}
      <ScriptPane
        label="Post-response script"
        description="Runs after the response arrives. Can read pm.response and call pm.test() to assert."
        value={postScript}
        onChange={onPostChange}
        placeholder={PM_SNIPPET}
      />
    </div>
  );
}

interface ScriptPaneProps {
  label: string;
  description: string;
  value: string;
  onChange: (v: string) => void;
  placeholder: string;
}

function ScriptPane({
  label,
  description,
  value,
  onChange,
  placeholder,
}: ScriptPaneProps) {
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      {/* Header */}
      <div className="flex items-center gap-2 border-b border-slate-200 px-3 py-1.5 dark:border-slate-700">
        <span className="text-xs font-semibold text-slate-700 dark:text-slate-300">
          {label}
        </span>
        <span className="text-xs text-slate-500 dark:text-slate-400">
          — {description}
        </span>
      </div>

      {/* Editor */}
      <div className="relative min-h-0 flex-1">
        {value === "" && (
          <pre className="pointer-events-none absolute inset-0 z-10 overflow-hidden p-2 font-mono text-xs leading-relaxed text-slate-400 dark:text-slate-600 select-none">
            {placeholder}
          </pre>
        )}
        <Suspense
          fallback={
            <div className="flex h-full items-center justify-center text-xs text-slate-400">
              Loading editor…
            </div>
          }
        >
          <LazyCodeEditor
            value={value}
            onChange={(v) => onChange(v ?? "")}
            language="javascript"
            options={{
              minimap: { enabled: false },
              lineNumbers: "on",
              fontSize: 12,
              scrollBeyondLastLine: false,
              wordWrap: "on",
              automaticLayout: true,
              suggest: {
                showWords: false,
              },
            }}
          />
        </Suspense>
      </div>
    </div>
  );
}
