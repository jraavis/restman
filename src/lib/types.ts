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
