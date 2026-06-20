import type * as monaco from "monaco-editor";

declare global {
  interface Window {
    MonacoEnvironment?: monaco.Environment;
  }
}

export {};
