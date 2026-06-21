//! `{{var}}` completion for Monaco. Only imported from `CodeEditor.tsx`, which
//! is already behind the lazy boundary — pulling `monaco-editor` in here
//! doesn't add it to the startup bundle. The provider is registered once per
//! language and reads `currentKeys` at trigger time, since Monaco providers
//! are long-lived but the variable list changes as the user edits/switches.

import * as monaco from "monaco-editor";

const LANGUAGES = ["json", "xml", "html", "javascript", "python", "plaintext", "graphql"];

let currentKeys: string[] = [];
let registered = false;

export function setVariableCompletionKeys(keys: string[]) {
  currentKeys = keys;
}

export function ensureVariableCompletionRegistered() {
  if (registered) return;
  registered = true;

  for (const language of LANGUAGES) {
    monaco.languages.registerCompletionItemProvider(language, {
      triggerCharacters: ["{"],
      provideCompletionItems(model, position) {
        const textUntilCaret = model.getValueInRange({
          startLineNumber: position.lineNumber,
          startColumn: 1,
          endLineNumber: position.lineNumber,
          endColumn: position.column,
        });
        const match = /\{\{([^{}\s]*)$/.exec(textUntilCaret);
        if (!match) return { suggestions: [] };

        const prefix = match[1].toLowerCase();
        const wordStart = position.column - match[1].length;
        const range = {
          startLineNumber: position.lineNumber,
          startColumn: wordStart,
          endLineNumber: position.lineNumber,
          endColumn: position.column,
        };

        return {
          suggestions: currentKeys
            .filter((key) => key.toLowerCase().includes(prefix))
            .map((key) => ({
              label: key,
              kind: monaco.languages.CompletionItemKind.Variable,
              insertText: `${key}}}`,
              range,
            })),
        };
      },
    });
  }
}
