//! Thin wrapper over @monaco-editor/react that wires the offline workers
//! (via the ./lib/monaco side-effect import) and syncs the editor theme to the
//! app theme. Lazy-load this component so Monaco stays out of the startup path.

import "../lib/monaco";
import Editor, { type EditorProps } from "@monaco-editor/react";
import { useIsDark } from "../hooks/useTheme";
import { useUiStore } from "../stores/uiStore";

export type CodeEditorProps = Omit<EditorProps, "theme">;

export function CodeEditor({ options, ...props }: CodeEditorProps) {
  const isDark = useIsDark();
  const fontSize = useUiStore((s) => s.editorFontSize);

  return (
    <Editor
      theme={isDark ? "vs-dark" : "light"}
      options={{ fontSize, ...options }}
      {...props}
    />
  );
}

export default CodeEditor;
