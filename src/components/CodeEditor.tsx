//! Thin wrapper over @monaco-editor/react that wires the offline workers
//! (via the ./lib/monaco side-effect import) and syncs the editor theme to the
//! app theme. Lazy-load this component so Monaco stays out of the startup path.

import { GRAPHQL_SCHEMA_URI, graphqlModeApi } from "../lib/monaco";
import { useEffect } from "react";
import Editor, { type EditorProps } from "@monaco-editor/react";
import type { GraphQLSchema } from "graphql";
import { useIsDark } from "../hooks/useTheme";
import { useUiStore } from "../stores/uiStore";
import { ensureVariableCompletionRegistered, setVariableCompletionKeys } from "../lib/monacoVariableCompletion";

export type CodeEditorProps = Omit<EditorProps, "theme"> & {
  /** Known `{{var}}` names to offer via autocomplete, e.g. from `useResolvedVariableKeys`. */
  variableKeys?: string[];
  /** A fetched GraphQL schema to drive the "graphql"-language editor's
   * completion/hover/diagnostics. Only meaningful when `language="graphql"`. */
  graphqlSchema?: GraphQLSchema | null;
};

export function CodeEditor({ options, variableKeys, graphqlSchema, ...props }: CodeEditorProps) {
  const isDark = useIsDark();
  const fontSize = useUiStore((s) => s.editorFontSize);
  const wordWrap = useUiStore((s) => s.editorWordWrap);
  const tabSize = useUiStore((s) => s.editorTabSize);

  useEffect(() => {
    ensureVariableCompletionRegistered();
  }, []);
  useEffect(() => {
    setVariableCompletionKeys(variableKeys ?? []);
  }, [variableKeys]);
  useEffect(() => {
    graphqlModeApi.setSchemaConfig(graphqlSchema ? [{ uri: GRAPHQL_SCHEMA_URI, schema: graphqlSchema }] : []);
  }, [graphqlSchema]);

  return (
    <Editor
      theme={isDark ? "vs-dark" : "light"}
      options={{ fontSize, wordWrap: wordWrap ? "on" : "off", tabSize, ...options }}
      {...props}
    />
  );
}

export default CodeEditor;
