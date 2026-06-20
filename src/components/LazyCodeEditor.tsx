//! Lazy-loaded Monaco wrapper. Importing CodeEditor pulls Monaco's chunks, so
//! we defer it behind React.lazy to keep it off the startup path.

import { lazy, Suspense } from "react";
import type { CodeEditorProps } from "./CodeEditor";

const CodeEditor = lazy(() => import("./CodeEditor"));

export function LazyCodeEditor(props: CodeEditorProps) {
  return (
    <Suspense
      fallback={<div className="p-2 text-xs text-slate-400">Loading editor…</div>}
    >
      <CodeEditor {...props} />
    </Suspense>
  );
}
