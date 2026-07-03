//! Thin wrapper over @monaco-editor/react that wires the offline workers
//! (via the ./lib/monaco side-effect import) and syncs the editor theme to the
//! app theme. Lazy-load this component so Monaco stays out of the startup path.

import { GRAPHQL_SCHEMA_URI, graphqlModeApi } from "../lib/monaco";
import { useCallback, useEffect, useRef } from "react";
import Editor, { type EditorProps } from "@monaco-editor/react";
import type { editor as MonacoEditor, IDisposable } from "monaco-editor";
import type { GraphQLSchema } from "graphql";
import { useIsDark } from "../hooks/useTheme";
import { useUiStore } from "../stores/uiStore";
import { ensureVariableCompletionRegistered, setVariableCompletionKeys } from "../lib/monacoVariableCompletion";

export type CodeEditorProps = Omit<EditorProps, "theme"> & {
  /** Shown when `value` is empty, aligned to Monaco's text column (past line numbers). */
  placeholder?: string;
  /** Known `{{var}}` names to offer via autocomplete, e.g. from `useResolvedVariableKeys`. */
  variableKeys?: string[];
  /** A fetched GraphQL schema to drive the "graphql"-language editor's
   * completion/hover/diagnostics. Only meaningful when `language="graphql"`. */
  graphqlSchema?: GraphQLSchema | null;
};

export function CodeEditor({
  options,
  variableKeys,
  graphqlSchema,
  placeholder,
  onMount,
  value,
  ...props
}: CodeEditorProps) {
  const isDark = useIsDark();
  const fontSize = useUiStore((s) => s.editorFontSize);
  const wordWrap = useUiStore((s) => s.editorWordWrap);
  const tabSize = useUiStore((s) => s.editorTabSize);
  const placeholderRef = useRef<HTMLPreElement>(null);
  const layoutDisposableRef = useRef<IDisposable | null>(null);

  useEffect(() => {
    ensureVariableCompletionRegistered();
  }, []);
  useEffect(() => {
    setVariableCompletionKeys(variableKeys ?? []);
  }, [variableKeys]);
  useEffect(() => {
    graphqlModeApi.setSchemaConfig(graphqlSchema ? [{ uri: GRAPHQL_SCHEMA_URI, schema: graphqlSchema }] : []);
  }, [graphqlSchema]);
  useEffect(() => () => layoutDisposableRef.current?.dispose(), []);

  const syncPlaceholder = useCallback((editor: MonacoEditor.IStandaloneCodeEditor) => {
    if (!placeholderRef.current) return;
    placeholderRef.current.style.left = `${editor.getLayoutInfo().contentLeft}px`;
  }, []);

  const handleMount = useCallback(
    (editor: MonacoEditor.IStandaloneCodeEditor, monaco: typeof import("monaco-editor")) => {
      layoutDisposableRef.current?.dispose();
      if (placeholder) {
        syncPlaceholder(editor);
        layoutDisposableRef.current = editor.onDidLayoutChange(() => syncPlaceholder(editor));
      }
      onMount?.(editor, monaco);
    },
    [placeholder, onMount, syncPlaceholder],
  );

  const showPlaceholder = Boolean(placeholder) && !(value ?? "");

  return (
    <div className="relative h-full w-full">
      {showPlaceholder && (
        <pre
          ref={placeholderRef}
          aria-hidden
          className="pointer-events-none absolute top-0 right-0 bottom-0 z-10 overflow-hidden py-2 pr-2 font-mono text-xs leading-relaxed text-slate-400 select-none dark:text-slate-600"
        >
          {placeholder}
        </pre>
      )}
      <Editor
        theme={isDark ? "vs-dark" : "light"}
        options={{ fontSize, wordWrap: wordWrap ? "on" : "off", tabSize, ...options }}
        value={value}
        onMount={handleMount}
        {...props}
      />
    </div>
  );
}

export default CodeEditor;
