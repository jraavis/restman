//! Thin wrapper over @monaco-editor/react that wires the offline workers
//! (via the ./lib/monaco side-effect import) and syncs the editor theme to the
//! app theme. Lazy-load this component so Monaco stays out of the startup path.

import "../lib/monaco";
import { useEffect } from "react";
import Editor, { type EditorProps } from "@monaco-editor/react";
import { useIsDark } from "../hooks/useTheme";
import { useUiStore } from "../stores/uiStore";
import { ensureVariableCompletionRegistered, setVariableCompletionKeys } from "../lib/monacoVariableCompletion";

export type CodeEditorProps = Omit<EditorProps, "theme"> & {
  /** Known `{{var}}` names to offer via autocomplete, e.g. from `useResolvedVariableKeys`. */
  variableKeys?: string[];
};

export function CodeEditor({ options, variableKeys, ...props }: CodeEditorProps) {
  const isDark = useIsDark();
  const fontSize = useUiStore((s) => s.editorFontSize);

  useEffect(() => {
    ensureVariableCompletionRegistered();
  }, []);
  useEffect(() => {
    setVariableCompletionKeys(variableKeys ?? []);
  }, [variableKeys]);

  return (
    <Editor
      theme={isDark ? "vs-dark" : "light"}
      options={{ fontSize, ...options }}
      {...props}
    />
  );
}

export default CodeEditor;
