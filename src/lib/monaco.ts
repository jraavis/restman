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

// `javascriptDefaults` ships with semantic validation OFF by default (only
// syntax errors show out of the box) — turn it on so real mistakes in
// pre/post-request scripts (typos like `pm.tets`, wrong argument counts)
// surface as red squiggles instead of only failing at send-time. This is a
// single global setting shared by every "javascript"-language editor in the
// app (Scripts tab and the plugin source editor in `PluginManagerDialog`) —
// Monaco's JS/TS language service has no per-model diagnostics toggle, so
// there's no way to scope this to just one editor. That's fine here: the
// plugin editor's scripts don't reference any ambient globals of their own,
// so semantic validation there only ever catches genuine bugs too.
monaco.typescript.javascriptDefaults.setDiagnosticsOptions({
  noSemanticValidation: false,
  noSyntaxValidation: false,
});
// `noSemanticValidation: false` alone isn't enough — TypeScript's language
// service still suppresses semantic errors for plain `.js` files unless
// `checkJs` is on too (confirmed empirically: without this, a real worker
// RPC for `pm.tets(1)` came back with zero diagnostics even though
// `noSemanticValidation` was already `false`). `allowJs`/`allowNonTsExtensions`/
// `target` are the existing defaults — spread them forward rather than
// replacing wholesale, since `setCompilerOptions` overwrites, not merges.
monaco.typescript.javascriptDefaults.setCompilerOptions({
  ...monaco.typescript.javascriptDefaults.getCompilerOptions(),
  checkJs: true,
});

// Ambient typings for the `pm.*` test/scripting API (see
// `src-tauri/src/scripting/engine.rs`'s `inject_pm_pre`/`inject_pm_post`/
// `build_assertion_chain` — this mirrors their exact real shape, not just
// the human-readable `PM_SNIPPET` hint text in `ScriptsTab.tsx`, since a
// typing that's looser or stricter than the real runtime would produce
// false negatives or false positives here). `pm.request`/`pm.response` are
// declared as always-present even though `pm.response` is only actually
// injected for post-response scripts — Monaco has no per-model way to swap
// typings between the pre- and post-script editors, so this errs toward not
// flagging a legitimate post-script API as unknown rather than modeling the
// pre/post split exactly.
const PM_API_TYPES = `
interface PmAssertionChain {
  to: PmAssertionChain;
  be: PmAssertionChain;
  have: PmAssertionChain;
  not: PmAssertionChain;
  equal(expected: unknown): void;
  include(needle: unknown): void;
  true(): void;
  false(): void;
  null(): void;
  undefined(): void;
  a(type: string): void;
  an(type: string): void;
  length(n: number): void;
}
interface PmHeaders {
  get(name: string): string | undefined;
}
interface PmEnvironment {
  get(key: string): string | undefined;
  has(key: string): boolean;
  set(key: string, value: unknown): void;
  unset(key: string): void;
}
interface PmRequest {
  method: string;
  url: string;
  headers: PmHeaders;
}
interface PmResponse {
  status: number;
  statusText: string;
  responseTime: number;
  headers: PmHeaders;
  text(): string;
  json(): unknown;
}
declare const pm: {
  environment: PmEnvironment;
  request: PmRequest;
  response: PmResponse;
  test(name: string, fn: () => void): void;
  expect(value: unknown): PmAssertionChain;
  abort(): void;
};
declare const $guid: string;
declare const $timestamp: number;
declare const $randomInt: number;
`;
monaco.typescript.javascriptDefaults.addExtraLib(PM_API_TYPES, "ts:restman/pm-api.d.ts");
