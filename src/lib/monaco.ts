//! Monaco bootstrap. Importing this module wires Monaco's web workers to the
//! local (bundled) copies so the editor works fully offline — no CDN fetch.
//! Kept out of the startup path: only `CodeEditor` imports it, and consumers
//! lazy-load `CodeEditor`, so Monaco's large chunks don't block cold start.

import * as monaco from "monaco-editor";
import editorWorker from "monaco-editor/esm/vs/editor/editor.worker?worker";
import jsonWorker from "monaco-editor/esm/vs/language/json/json.worker?worker";
import cssWorker from "monaco-editor/esm/vs/language/css/css.worker?worker";
import htmlWorker from "monaco-editor/esm/vs/language/html/html.worker?worker";
import tsWorker from "monaco-editor/esm/vs/language/typescript/ts.worker?worker";
import graphqlWorker from "monaco-graphql/esm/graphql.worker?worker";
// `monaco-graphql/lite`, not `/initializeMode` — the latter statically
// imports monaco-editor's whole "full" contrib bundle (hover/find/clipboard/
// etc.), which this app never loads elsewhere and which breaks Vite's
// dependency pre-bundling here (a CSS-in-JS file Rolldown can't parse under
// this sandbox's Node version). `lite` is monaco-graphql's documented entry
// point for apps that already bring their own Monaco setup — exactly this
// one, which configures monaco-editor itself in the rest of this file.
import { initializeMode } from "monaco-graphql/lite";
import { loader } from "@monaco-editor/react";

self.MonacoEnvironment = {
  getWorker(_workerId, label) {
    switch (label) {
      case "json":
        return new jsonWorker();
      case "css":
      case "scss":
      case "less":
        return new cssWorker();
      case "html":
      case "handlebars":
      case "razor":
        return new htmlWorker();
      case "typescript":
      case "javascript":
        return new tsWorker();
      case "graphql":
        return new graphqlWorker();
      default:
        return new editorWorker();
    }
  },
};

// Use the bundled monaco instead of @monaco-editor/react's default CDN loader.
loader.config({ monaco });

// Registers the "graphql" language's completion/hover/diagnostics providers
// against monaco-editor's own worker infrastructure. `graphqlModeApi` is the
// single handle callers use to swap in a fetched schema — with exactly one
// schema config in the array, monaco-graphql treats it as the default for
// every "graphql"-language model (no `fileMatch`/`uri` juggling needed since
// this app only ever has one GraphQL query editor active at a time).
export const graphqlModeApi = initializeMode();
export const GRAPHQL_SCHEMA_URI = "schema://active";
