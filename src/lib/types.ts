//! Frontend mirrors of the Rust Phase 2 model types (serde camelCase / tagged enums).

import type { HttpRequest, KeyValue, HeaderEntry, RequestBody, RequestOptions, HttpResponse } from "./http";

export type ApiKeyLocation = "header" | "query";

export type OAuth2GrantType = "authorization_code" | "client_credentials" | "password" | "refresh_token";

export type PkceMethod = "s256" | "plain" | "none";

export interface OAuth2Config {
  grantType: OAuth2GrantType;
  authUrl: string;
  tokenUrl: string;
  clientId: string;
  clientSecret: string;
  scope: string;
  redirectUri: string;
  pkce: PkceMethod;
  username: string;
  password: string;
  /** Seeded refresh token for the `refresh_token` grant only — the
   * Authorization Code grant's refresh token lives server-side in the token
   * cache and never crosses IPC. */
  refreshToken: string;
}

export interface AwsSigV4Config {
  accessKey: string;
  secretKey: string;
  region: string;
  service: string;
  sessionToken: string;
}

/** Tagged by `type` — mirrors serde's internally-tagged `AuthConfig` enum. */
export type AuthConfig =
  | { type: "none" }
  | { type: "bearer"; token: string }
  | { type: "basic"; username: string; password: string }
  | { type: "api_key"; key: string; value: string; location: ApiKeyLocation }
  | ({ type: "o_auth2" } & OAuth2Config)
  | ({ type: "aws_sig_v4" } & AwsSigV4Config);

export type AuthType = AuthConfig["type"];

/** Request-level auth: inherit the owning collection's `AuthConfig`, or
 * override it. `Own`'s fields flatten onto this same object — serde renders
 * an adjacently-tagged newtype around an internally-tagged enum as one
 * object with both tags as sibling keys, not nested under `value`/`data`. */
export type RequestAuth = { mode: "inherit" } | ({ mode: "own" } & AuthConfig);

export function defaultOAuth2Config(): OAuth2Config {
  return {
    grantType: "client_credentials",
    authUrl: "",
    tokenUrl: "",
    clientId: "",
    clientSecret: "",
    scope: "",
    redirectUri: "",
    pkce: "s256",
    username: "",
    password: "",
    refreshToken: "",
  };
}

export function defaultAwsSigV4Config(): AwsSigV4Config {
  return { accessKey: "", secretKey: "", region: "", service: "", sessionToken: "" };
}

/** Fresh, empty `AuthConfig` for `type` — for switching the auth-type picker
 * without carrying over stale fields from whatever type was selected before. */
export function emptyAuthConfig(type: AuthType): AuthConfig {
  switch (type) {
    case "none":
      return { type: "none" };
    case "bearer":
      return { type: "bearer", token: "" };
    case "basic":
      return { type: "basic", username: "", password: "" };
    case "api_key":
      return { type: "api_key", key: "", value: "", location: "header" };
    case "o_auth2":
      return { type: "o_auth2", ...defaultOAuth2Config() };
    case "aws_sig_v4":
      return { type: "aws_sig_v4", ...defaultAwsSigV4Config() };
  }
}

export const defaultRequestAuth = (): RequestAuth => ({ mode: "inherit" });

/** Drives the single type-picker dropdown that doubles as the inherit/override
 * control: `"inherit"` plus every concrete `AuthType`. */
export type AuthOptionValue = "inherit" | AuthType;

export function authOptionValue(auth: RequestAuth): AuthOptionValue {
  return auth.mode === "inherit" ? "inherit" : auth.type;
}

/** Applies a dropdown selection to the current `RequestAuth`. Re-selecting
 * the type already in effect is a no-op (preserves its fields); any other
 * selection starts that type fresh via `emptyAuthConfig` rather than
 * carrying over stale fields from whatever was selected before. */
export function applyAuthOption(current: RequestAuth, option: AuthOptionValue): RequestAuth {
  if (option === "inherit") return { mode: "inherit" };
  if (current.mode === "own" && current.type === option) return current;
  return { mode: "own", ...emptyAuthConfig(option) };
}

/** Non-secret OAuth2 connection summary — never the token itself. */
export interface OAuth2Status {
  connected: boolean;
  expiresAt: number | null;
  scope: string | null;
}

// ---------------------------------------------------------------------------
// Workspace transport settings (Interlude / Phase 6) — mirrors
// `model::workspace_settings::{ClientCertConfig, WorkspaceSettings}`
// ---------------------------------------------------------------------------

export type ClientCertMode = "none" | "paste" | "path";

/** Adjacently-tagged (`mode` + `data`) — mirrors serde's
 * `#[serde(tag = "mode", content = "data")]`. `paste`'s `certPem`/`keyPem`/
 * `passphrase` are masked (`SECRET_MASK`) whenever they cross IPC for
 * display; round-tripping them unchanged means "keep the stored value",
 * same contract as every other secret field in this app. */
export type ClientCertConfig =
  | { mode: "none" }
  | { mode: "paste"; data: { certPem: string; keyPem: string; passphrase: string | null } }
  | { mode: "path"; data: { certPath: string; keyPath: string; passphrase: string | null } };

/** How this workspace's `.restman/` folder relates to the DB — see the
 * Rust `crate::sync` module doc for what each mode does. Import is always a
 * manual, explicit action regardless of mode; only export auto-triggers in
 * `"live"`. */
export type SyncMode = "off" | "manual" | "live";
export type SyncFormat = "json" | "yaml";

export interface WorkspaceSettings {
  workspaceId: string;
  proxyUrl: string | null;
  proxyBypass: string | null;
  defaultHeaders: HeaderEntry[];
  clientCert: ClientCertConfig;
  syncFolderPath: string | null;
  syncMode: SyncMode;
  syncFormat: SyncFormat;
}

export function emptyWorkspaceSettings(workspaceId: string): WorkspaceSettings {
  return {
    workspaceId,
    proxyUrl: null,
    proxyBypass: null,
    defaultHeaders: [],
    clientCert: { mode: "none" },
    syncFolderPath: null,
    syncMode: "off",
    syncFormat: "json",
  };
}

/** Fresh `ClientCertConfig` for `mode` — for switching the mode picker
 * without carrying over stale fields from whatever mode was selected
 * before. */
export function emptyClientCertConfig(mode: ClientCertMode): ClientCertConfig {
  switch (mode) {
    case "none":
      return { mode: "none" };
    case "paste":
      return { mode: "paste", data: { certPem: "", keyPem: "", passphrase: null } };
    case "path":
      return { mode: "path", data: { certPath: "", keyPath: "", passphrase: null } };
  }
}

// ---------------------------------------------------------------------------
// Scripting / test runner types
// ---------------------------------------------------------------------------

/** Outcome of a single `pm.test(name, fn)` call. */
export interface TestResult {
  name: string;
  passed: boolean;
  error: string | null;
}

/** Aggregate result from running a pre- or post-request script. */
export interface ScriptResult {
  tests: TestResult[];
  error: string | null;
  envMutations: [string, string][];
  envUnsets: string[];
  aborted: boolean;
}

/** The HTTP response plus any script outcomes — returned by `send_request`. */
export interface SendResponse {
  response: HttpResponse;
  preScript: ScriptResult | null;
  postScript: ScriptResult | null;
}

/** Per-request outcome in a collection run. */
export interface RequestRunResult {
  status: number | null;
  durationMs: number;
  passed: number;
  failed: number;
  tests: TestResult[];
  error: string | null;
}

/** Emitted as `runner:progress` Tauri event during a collection run. */
export interface RunnerProgress {
  runId: string;
  requestId: string;
  requestName: string;
  index: number;
  total: number;
  result: RequestRunResult | null;
}

/** Returned when a collection run completes. */
export interface CollectionRunSummary {
  runId: string;
  totalRequests: number;
  passedRequests: number;
  failedRequests: number;
  totalTests: number;
  passedTests: number;
  failedTests: number;
  durationMs: number;
  results: RequestRunResult[];
  junitXml: string;
}

/** Options for `run_collection_tests`. */
export interface CollectionRunOptions {
  workspaceId: string;
  collectionId: string;
  data?: string | null;
  iterations?: number;
  /** Ignored when `parallel` is true. */
  delayMs?: number;
  /** Run each iteration's requests concurrently (waves of up to 5). */
  parallel?: boolean;
}

export interface Collection {
  id: string;
  workspaceId: string;
  parentId: string | null;
  name: string;
  description: string | null;
  auth: AuthConfig;
  sortOrder: number;
  createdAt: number;
  updatedAt: number;
}

export interface Tag {
  id: string;
  workspaceId: string;
  name: string;
  color: string;
}

export interface SavedRequest {
  id: string;
  collectionId: string;
  name: string;
  method: string;
  url: string;
  headers: HeaderEntry[];
  query: KeyValue[];
  body: RequestBody;
  options: RequestOptions;
  auth: RequestAuth;
  preRequestScript: string;
  postResponseScript: string;
  tags: Tag[];
  sortOrder: number;
  createdAt: number;
  updatedAt: number;
  lastUsedAt: number | null;
}

export interface SavedRequestInput {
  name: string;
  method: string;
  url: string;
  headers: HeaderEntry[];
  query: KeyValue[];
  body: RequestBody;
  options: RequestOptions;
  auth: RequestAuth;
  preRequestScript: string;
  postResponseScript: string;
}

export interface SearchHit {
  request: SavedRequest;
  nameHighlight: string;
  urlHighlight: string;
}

// Sentinels FTS5's `highlight()` wraps matches with (Rust passes `char(1)`/
// `char(2)` instead of HTML tags, so a raw search hit can never inject markup
// if rendered carelessly) — split on these and render `<mark>` yourself.
export const HIGHLIGHT_OPEN = String.fromCharCode(1);
export const HIGHLIGHT_CLOSE = String.fromCharCode(2);

export interface Environment {
  id: string;
  workspaceId: string;
  collectionId: string | null;
  name: string;
  groupName: string | null;
  isActive: boolean;
  createdAt: number;
  updatedAt: number;
}

export type VarType = "string" | "number" | "boolean" | "json";

export type VarScope =
  | { kind: "global" }
  | { kind: "workspace"; id: string }
  | { kind: "collection"; id: string }
  | { kind: "environment"; id: string };

export interface Variable {
  id: string;
  scope: VarScope;
  key: string;
  value: string;
  varType: VarType;
  isSecret: boolean;
  enabled: boolean;
  sortOrder: number;
  createdAt: number;
  updatedAt: number;
}

export interface VariableInput {
  key: string;
  value: string;
  varType: VarType;
  isSecret: boolean;
  enabled: boolean;
}

/** Mirrors `model::SECRET_MASK` — round-tripped unchanged on update means "keep the stored value". */
export const SECRET_MASK = "••••••••";

export interface HistoryEntry {
  id: string;
  workspaceId: string;
  requestId: string | null;
  name: string;
  method: string;
  url: string;
  status: number | null;
  durationMs: number | null;
  request: HttpRequest;
  response: HttpResponse | null;
  error: string | null;
  createdAt: number;
}

export interface HistoryFilter {
  text?: string | null;
  method?: string | null;
  statusMin?: number | null;
  statusMax?: number | null;
  dateMin?: number | null;
  dateMax?: number | null;
  limit?: number | null;
}

export interface Tab {
  id: string;
  workspaceId: string;
  requestId: string | null;
  title: string;
  draft: HttpRequest;
  sortOrder: number;
  isActive: boolean;
  createdAt: number;
  updatedAt: number;
}

// ---------------------------------------------------------------------------
// Import / export (Phase 5) — mirrors `interop::{ImportedNode, ImportedRequest, ...}`
// ---------------------------------------------------------------------------

export interface ImportedRequest {
  name: string;
  method: string;
  url: string;
  headers: HeaderEntry[];
  query: KeyValue[];
  body: RequestBody;
  options: RequestOptions;
  auth: RequestAuth;
  preRequestScript: string;
  postResponseScript: string;
}

export interface ImportedNode {
  name: string;
  description: string | null;
  auth: AuthConfig;
  requests: ImportedRequest[];
  children: ImportedNode[];
}

export type ImportFormat =
  | "postman"
  | "curl"
  | "open_api"
  | "har"
  | "insomnia"
  | "bruno"
  | "http_file";
export type ExportFormat = "postman" | "curl" | "open_api" | "har";

// ---------------------------------------------------------------------------
// Plugins (Phase 6 task 5) — mirrors `model::plugin::{Plugin, PluginInput, PluginKind}`
// ---------------------------------------------------------------------------

export type PluginKind = "codegen" | "import" | "export";

export interface Plugin {
  id: string;
  workspaceId: string;
  name: string;
  kind: PluginKind;
  /** Display label: language name for `codegen`, format name for `import`/`export`. */
  languageLabel: string;
  source: string;
  enabled: boolean;
  createdAt: number;
  updatedAt: number;
}

export interface PluginInput {
  name: string;
  kind: PluginKind;
  languageLabel: string;
  source: string;
  enabled: boolean;
}

/** Starter source for a new plugin of `kind` — names the entry-point
 * function the sandbox calls (see `plugins::runtime`), so a blank editor
 * isn't a blank slate. */
export const PLUGIN_SOURCE_TEMPLATES: Record<PluginKind, string> = {
  codegen: 'function generate(request, options) {\n  return request.method + " " + request.url;\n}',
  import: 'function parse(content) {\n  return { root: { name: "Imported", requests: [] }, warnings: [] };\n}',
  export: 'function exportCollection(node) {\n  return JSON.stringify(node, null, 2);\n}',
};

export function emptyPluginInput(kind: PluginKind): PluginInput {
  return { name: "", kind, languageLabel: "", source: PLUGIN_SOURCE_TEMPLATES[kind], enabled: true };
}

/** Mirrors `codegen::CodegenTarget` — the mutually exclusive native-language
 * vs. plugin-id selector `generate_code` dispatches on. */
export type CodegenTarget = { kind: "native"; language: CodeLanguage } | { kind: "plugin"; pluginId: string };

export interface ImportStats {
  folders: number;
  requests: number;
  warnings: number;
}

export interface ImportPreview {
  root: ImportedNode;
  warnings: string[];
  stats: ImportStats;
}

export type ConflictMode = "skip" | "overwrite" | "merge";

export interface ImportReport {
  createdCollections: number;
  createdRequests: number;
  skipped: number;
  overwritten: number;
  warnings: string[];
}

// ---------------------------------------------------------------------------
// File-based `.restman/` sync + ZIP backup/restore (Phase 8)
// ---------------------------------------------------------------------------

export interface SyncExportReport {
  collections: number;
  environments: number;
}

export interface SyncImportReport {
  collectionsImported: number;
  environmentsImported: number;
  warnings: string[];
}

export interface RestoreReport {
  secretsRestored: number;
  workspaces: number;
  collections: number;
  requests: number;
  environments: number;
  historyEntries: number;
}

// ---------------------------------------------------------------------------
// Environment import/export (Phase 5) — mirrors `interop::environment`
// ---------------------------------------------------------------------------

export interface ImportedVariable {
  key: string;
  value: string;
  enabled: boolean;
  isSecret: boolean;
}

export interface EnvironmentPreview {
  name: string;
  variables: ImportedVariable[];
  warnings: string[];
}

export interface EnvironmentImportReport {
  createdVariables: number;
  overwritten: number;
  warnings: string[];
}

// ---------------------------------------------------------------------------
// Code generation (Phase 5) — mirrors `codegen::{CodeLanguage, CodegenOptions}`
// ---------------------------------------------------------------------------

export type CodeLanguage =
  | "curl"
  | "javascript_fetch"
  | "python"
  | "go"
  | "rust"
  | "php"
  | "java"
  | "csharp"
  | "ruby";

export interface CodegenOptions {
  includeAuth: boolean;
  includeHeaders: boolean;
}

export function defaultCodegenOptions(): CodegenOptions {
  return { includeAuth: true, includeHeaders: true };
}

/** Display label plus the Monaco language id used to syntax-highlight the
 * generated snippet. */
export const CODE_LANGUAGES: { value: CodeLanguage; label: string; monacoLanguage: string }[] = [
  { value: "curl", label: "cURL", monacoLanguage: "shell" },
  { value: "javascript_fetch", label: "JavaScript (fetch)", monacoLanguage: "javascript" },
  { value: "python", label: "Python (requests)", monacoLanguage: "python" },
  { value: "go", label: "Go (net/http)", monacoLanguage: "go" },
  { value: "rust", label: "Rust (reqwest)", monacoLanguage: "rust" },
  { value: "php", label: "PHP (Guzzle)", monacoLanguage: "php" },
  { value: "java", label: "Java (OkHttp)", monacoLanguage: "java" },
  { value: "csharp", label: "C# (HttpClient)", monacoLanguage: "csharp" },
  { value: "ruby", label: "Ruby (Net::HTTP)", monacoLanguage: "ruby" },
];

// ---------------------------------------------------------------------------
// Cookie jar (Phase 6) — mirrors `model::http::CookieEntry`
// ---------------------------------------------------------------------------

export interface CookieEntry {
  name: string;
  value: string;
  domain: string;
  path: string;
  secure: boolean;
  httpOnly: boolean;
  sameSite: string | null;
  /** Unix seconds. `null` means a session cookie. */
  expiresAt: number | null;
}

// ---------------------------------------------------------------------------
// Streaming (Phase 6, #17a) — mirrors `model::streaming::SseEvent`
// ---------------------------------------------------------------------------

/** Tagged by `type` — matches serde's `tag = "type"` on `SseEvent`. */
export type SseEvent =
  | { type: "open" }
  | { type: "message"; event: string | null; data: string; id: string | null }
  | { type: "error"; message: string }
  | { type: "closed" };

// ---------------------------------------------------------------------------
// Streaming (Phase 6, #17b) — mirrors `model::streaming::WsEvent`/`WsOutbound`
// ---------------------------------------------------------------------------

/**
 * Tagged by `type` — matches serde's `tag = "type"` on `WsEvent`. Binary
 * frames carry base64 in `data` with `binary: true`.
 */
export type WsEvent =
  | { type: "open" }
  | { type: "message"; binary: boolean; data: string }
  | { type: "closed"; code: number | null; reason: string | null }
  | { type: "error"; message: string };

/** Outbound WS frame; `data` is text verbatim or base64 when `binary`. */
export interface WsOutbound {
  binary: boolean;
  data: string;
}

// ---------------------------------------------------------------------------
// Streaming (Phase 6, #17d) — mirrors `model::grpc::{GrpcEvent, GrpcConnectArgs, GrpcOutbound}`
// ---------------------------------------------------------------------------

/**
 * Tagged by `type` — matches serde's `tag = "type"` on `GrpcEvent`. Both
 * `call_unary` and `drive_streaming_call` emit this same enum regardless of
 * RPC mode: `Response` fires once for unary/client-streaming and once per
 * server message for server-streaming/bidi; `Status`/`Closed` always end the
 * call the same way. `message`'s payload type is `unknown` rather than a
 * concrete shape — it's a decoded protobuf message converted to JSON via
 * `prost-reflect`'s dynamic `serde` support, so its fields depend entirely
 * on whichever method's schema this connection was opened against.
 */
export type GrpcEvent =
  | { type: "open" }
  | { type: "response"; message: unknown }
  | { type: "status"; code: number; message: string | null }
  | { type: "error"; message: string }
  | { type: "closed" };

/**
 * Arguments for `grpcConnect`. `protoFiles` is a `path -> source text` map
 * (mirrors `engine::grpc::schema::ProtoFileSet`) compiled into a
 * `DescriptorPool` server-side — schema discovery via live server reflection
 * (#33) hasn't landed yet, so this is how a pool is sourced today; passing
 * the schema's already-discovered proto source (e.g. from `GrpcSchemaPicker`'s
 * proto-upload mode) here is the bridge until #33 adds a discovery-to-connect
 * handoff that doesn't require re-sending source text. `methodFullName` is
 * the same slash-separated `"package.Service/Method"` form used throughout
 * this feature (see `GrpcMethodDescriptor.fullName` in `grpcSchemaTypes.ts`).
 * There is no streaming-mode field — the backend derives unary vs.
 * client/server-streaming vs. bidi from the compiled schema itself, so it
 * can never disagree with what `protoFiles`/`entryPoint` actually describe.
 */
export interface GrpcConnectArgs {
  url: string;
  methodFullName: string;
  request: unknown;
  protoFiles: Record<string, string>;
  entryPoint: string;
}

/**
 * Outbound request message for `grpcSend`, sent on an already-open
 * client-streaming/bidi connection. No `methodFullName` — a connection is
 * bound to one method for its lifetime, set once at `grpcConnect`.
 */
export interface GrpcOutbound {
  request: unknown;
}

// ---------------------------------------------------------------------------
// Mock servers — mirrors `model::mock::{MockServer, MockServerInput, MockRule,
// MockRuleInput}`. Running state (is this one currently serving, and on what
// port) isn't part of the config row — see `listRunningMockServerIds`.
// ---------------------------------------------------------------------------

export interface MockServer {
  id: string;
  workspaceId: string;
  name: string;
  port: number;
  createdAt: number;
  updatedAt: number;
}

export interface MockServerInput {
  name: string;
  port: number;
}

/** `method: null` matches any method. `pathPattern` supports `:name`
 * segments matching any single path segment (e.g. `/users/:id`). */
export interface MockRule {
  id: string;
  mockServerId: string;
  method: string | null;
  pathPattern: string;
  status: number;
  headers: HeaderEntry[];
  body: string;
  delayMs: number;
  sortOrder: number;
}

export interface MockRuleInput {
  method: string | null;
  pathPattern: string;
  status: number;
  headers: HeaderEntry[];
  body: string;
  delayMs: number;
  sortOrder: number;
}
