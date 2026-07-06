//! Typed wrappers around Tauri IPC commands. The frontend never touches the
//! network or disk directly — every backend operation goes through here.

import { Channel, invoke } from "@tauri-apps/api/core";
import type { HeaderEntry, HttpRequest, HttpResponse } from "./http";
import type {
  AuthConfig,
  CodegenOptions,
  CodegenTarget,
  Collection,
  CollectionRunOptions,
  CollectionRunSummary,
  ConflictMode,
  CookieEntry,
  Environment,
  EnvironmentImportReport,
  EnvironmentPreview,
  ExportFormat,
  FullImportPreview,
  FullImportReport,
  GrpcConnectArgs,
  GrpcEvent,
  GrpcOutbound,
  GrpcSchemaDiscoveryResult,
  HistoryEntry,
  HistoryFilter,
  ImportFormat,
  ImportedNode,
  ImportPreview,
  ImportReport,
  MockRule,
  MockRuleInput,
  MockServer,
  MockServerInput,
  OAuth2Status,
  Plugin,
  PluginInput,
  PluginKind,
  RestoreReport,
  SavedRequest,
  SavedRequestInput,
  SearchHit,
  SendResponse,
  SseEvent,
  SyncExportReport,
  SyncImportReport,
  Tab,
  Tag,
  VarScope,
  Variable,
  VariableInput,
  WorkspaceSettings,
  WsEvent,
  WsOutbound,
} from "./types";

export interface Workspace {
  id: string;
  name: string;
  createdAt: number;
  updatedAt: number;
  isActive: boolean;
}

export interface SendRequestArgs {
  req: HttpRequest;
  workspaceId: string;
  collectionId?: string | null;
  requestId?: string | null;
  name?: string | null;
  preRequestScript?: string | null;
  postResponseScript?: string | null;
}

export const ipc = {
  ping: () => invoke<string>("ping"),
  sendRequest: (args: SendRequestArgs) => invoke<SendResponse>("send_request", { ...args }),

  // Workspaces
  listWorkspaces: () => invoke<Workspace[]>("list_workspaces"),
  activeWorkspace: () => invoke<Workspace | null>("active_workspace"),
  createWorkspace: (name: string) => invoke<Workspace>("create_workspace", { name }),
  updateWorkspace: (id: string, name: string) => invoke<Workspace>("update_workspace", { id, name }),
  deleteWorkspace: (id: string) => invoke<void>("delete_workspace", { id }),
  setActiveWorkspace: (id: string) => invoke<void>("set_active_workspace", { id }),
  getWorkspaceSettings: (workspaceId: string) =>
    invoke<WorkspaceSettings>("get_workspace_settings", { workspaceId }),
  setWorkspaceSettings: (settings: WorkspaceSettings) =>
    invoke<WorkspaceSettings>("set_workspace_settings", { settings }),

  // Collections
  listCollections: (workspaceId: string) => invoke<Collection[]>("list_collections", { workspaceId }),
  createCollection: (workspaceId: string, parentId: string | null, name: string, description?: string | null) =>
    invoke<Collection>("create_collection", { workspaceId, parentId, name, description: description ?? null }),
  updateCollection: (id: string, name: string, description?: string | null) =>
    invoke<Collection>("update_collection", { id, name, description: description ?? null }),
  updateCollectionAuth: (id: string, auth: AuthConfig) => invoke<Collection>("update_collection_auth", { id, auth }),
  deleteCollection: (id: string) => invoke<void>("delete_collection", { id }),
  moveCollection: (id: string, newParentId: string | null) =>
    invoke<Collection>("move_collection", { id, newParentId }),
  reorderCollections: (ids: string[]) => invoke<void>("reorder_collections", { ids }),
  duplicateCollection: (id: string, newName?: string | null) =>
    invoke<Collection>("duplicate_collection", { id, newName: newName ?? null }),

  // Requests
  listRequests: (collectionId: string) => invoke<SavedRequest[]>("list_requests", { collectionId }),
  getRequest: (id: string) => invoke<SavedRequest>("get_request", { id }),
  createRequest: (collectionId: string, input: SavedRequestInput) =>
    invoke<SavedRequest>("create_request", { collectionId, input }),
  updateRequest: (id: string, input: SavedRequestInput) => invoke<SavedRequest>("update_request", { id, input }),
  deleteRequest: (id: string) => invoke<void>("delete_request", { id }),
  moveRequest: (id: string, collectionId: string) => invoke<SavedRequest>("move_request", { id, collectionId }),
  reorderRequests: (ids: string[]) => invoke<void>("reorder_requests", { ids }),
  duplicateRequest: (id: string, newName?: string | null) =>
    invoke<SavedRequest>("duplicate_request", { id, newName: newName ?? null }),
  setRequestTags: (requestId: string, tagIds: string[]) =>
    invoke<void>("set_request_tags", { requestId, tagIds }),
  searchRequests: (workspaceId: string, query: string, method?: string | null) =>
    invoke<SearchHit[]>("search_requests", { workspaceId, query, method: method ?? null }),

  // OAuth2 — both resolve the same collection/request inheritance chain as
  // `sendRequest`'s auth resolution; pass whichever id(s) are in scope.
  startOAuth2Authorization: (collectionId?: string | null, requestId?: string | null) =>
    invoke<OAuth2Status>("start_oauth2_authorization", { collectionId: collectionId ?? null, requestId: requestId ?? null }),
  getOAuth2Status: (collectionId?: string | null, requestId?: string | null) =>
    invoke<OAuth2Status | null>("get_oauth2_status", { collectionId: collectionId ?? null, requestId: requestId ?? null }),
  getOAuthTokenPreview: (collectionId?: string | null, requestId?: string | null) =>
    invoke<string | null>("get_oauth_token_preview", { collectionId: collectionId ?? null, requestId: requestId ?? null }),

  // Scripting / test runner
  runCollectionTests: (options: CollectionRunOptions) =>
    invoke<CollectionRunSummary>("run_collection_tests", { options }),

  // Tags
  listTags: (workspaceId: string) => invoke<Tag[]>("list_tags", { workspaceId }),
  createTag: (workspaceId: string, name: string, color: string) =>
    invoke<Tag>("create_tag", { workspaceId, name, color }),
  updateTag: (id: string, name: string, color: string) => invoke<void>("update_tag", { id, name, color }),
  deleteTag: (id: string) => invoke<void>("delete_tag", { id }),

  // Environments
  listEnvironments: (workspaceId: string) => invoke<Environment[]>("list_environments", { workspaceId }),
  createEnvironment: (workspaceId: string, collectionId: string | null, name: string, groupName?: string | null) =>
    invoke<Environment>("create_environment", { workspaceId, collectionId, name, groupName: groupName ?? null }),
  updateEnvironment: (id: string, name: string, groupName?: string | null) =>
    invoke<Environment>("update_environment", { id, name, groupName: groupName ?? null }),
  deleteEnvironment: (id: string) => invoke<void>("delete_environment", { id }),
  setActiveEnvironment: (workspaceId: string, id: string | null) =>
    invoke<void>("set_active_environment", { workspaceId, id }),
  activeEnvironment: (workspaceId: string) => invoke<Environment | null>("active_environment", { workspaceId }),

  // Variables
  listVariables: (scope: VarScope) => invoke<Variable[]>("list_variables", { scope }),
  createVariable: (scope: VarScope, input: VariableInput) => invoke<Variable>("create_variable", { scope, input }),
  updateVariable: (id: string, input: VariableInput) => invoke<Variable>("update_variable", { id, input }),
  deleteVariable: (id: string) => invoke<void>("delete_variable", { id }),

  // History
  listHistory: (workspaceId: string, filter: HistoryFilter) =>
    invoke<HistoryEntry[]>("list_history", { workspaceId, filter }),
  deleteHistoryEntry: (id: string) => invoke<void>("delete_history_entry", { id }),
  clearHistory: (workspaceId: string) => invoke<void>("clear_history", { workspaceId }),
  replayHistoryEntry: (id: string) => invoke<HttpResponse>("replay_history_entry", { id }),
  getHistoryRetention: () => invoke<number>("get_history_retention"),
  setHistoryRetention: (count: number) => invoke<void>("set_history_retention", { count }),

  // Tabs
  listTabs: (workspaceId: string) => invoke<Tab[]>("list_tabs", { workspaceId }),
  createTab: (workspaceId: string, requestId: string | null, title: string, draft: HttpRequest) =>
    invoke<Tab>("create_tab", { workspaceId, requestId, title, draft }),
  updateTabDraft: (id: string, title: string, draft: HttpRequest) =>
    invoke<Tab>("update_tab_draft", { id, title, draft }),
  setTabRequestId: (id: string, requestId: string) => invoke<Tab>("set_tab_request_id", { id, requestId }),
  setActiveTab: (workspaceId: string, id: string) => invoke<void>("set_active_tab", { workspaceId, id }),
  reorderTabs: (ids: string[]) => invoke<void>("reorder_tabs", { ids }),
  closeTab: (workspaceId: string, id: string) => invoke<void>("close_tab", { workspaceId, id }),
  closeOtherTabs: (workspaceId: string, keepId: string) =>
    invoke<void>("close_other_tabs", { workspaceId, keepId }),
  closeAllTabs: (workspaceId: string) => invoke<void>("close_all_tabs", { workspaceId }),

  // Import / export. `source`/`target` are mutually exclusive native-format
  // vs. plugin-id selectors — mirrors `commands::interop`'s `format`/
  // `plugin_id` pair on the Rust side.
  previewImport: (content: string, source: { format: ImportFormat } | { pluginId: string }) =>
    invoke<ImportPreview>("preview_import", {
      content,
      format: "format" in source ? source.format : null,
      pluginId: "pluginId" in source ? source.pluginId : null,
    }),
  previewImportBrunoDirectory: (path: string) =>
    invoke<ImportPreview>("preview_import_bruno_directory", { path }),
  applyCollectionImport: (
    workspaceId: string,
    parentId: string | null,
    root: ImportedNode,
    mode: ConflictMode,
  ) => invoke<ImportReport>("apply_collection_import", { workspaceId, parentId, root, mode }),
  exportCollection: (collectionId: string, target: { format: ExportFormat } | { pluginId: string }) =>
    invoke<string>("export_collection", {
      collectionId,
      format: "format" in target ? target.format : null,
      pluginId: "pluginId" in target ? target.pluginId : null,
    }),

  // Environment import / export
  previewEnvironmentImport: (content: string) =>
    invoke<EnvironmentPreview>("preview_environment_import", { content }),
  applyEnvironmentImport: (
    workspaceId: string,
    collectionId: string | null,
    preview: EnvironmentPreview,
    overwriteExisting: boolean,
  ) =>
    invoke<EnvironmentImportReport>("apply_environment_import", {
      workspaceId,
      collectionId,
      preview,
      overwriteExisting,
    }),
  exportEnvironment: (environmentId: string) =>
    invoke<string>("export_environment", { environmentId }),

  // Restman-native full export/import (`.restman.json`) — selected whole
  // workspaces incl. collections, scripts, environments, and variables.
  exportRestman: (workspaceIds: string[], includeSecrets: boolean, includeSettings: boolean) =>
    invoke<string>("export_restman", { workspaceIds, includeSecrets, includeSettings }),
  previewRestmanImport: (content: string) =>
    invoke<FullImportPreview>("preview_restman_import", { content }),
  applyRestmanImport: (content: string, mode: ConflictMode) =>
    invoke<FullImportReport>("apply_restman_import", { content, mode }),

  // Files
  writeFileBytes: (path: string, contentBase64: string) =>
    invoke<void>("write_file_bytes", { path, contentBase64 }),

  // Cookies
  listCookies: () => invoke<CookieEntry[]>("list_cookies"),
  deleteCookie: (domain: string, path: string, name: string) =>
    invoke<void>("delete_cookie", { domain, path, name }),
  clearCookies: () => invoke<void>("clear_cookies"),

  // Streaming (SSE)
  sseConnect: (
    workspaceId: string,
    url: string,
    headers: HeaderEntry[],
    onEvent: (event: SseEvent) => void,
  ) => {
    const channel = new Channel<SseEvent>();
    channel.onmessage = onEvent;
    return invoke<string>("sse_connect", { channel, workspaceId, url, headers });
  },

  // Streaming (WebSocket)
  wsConnect: (
    workspaceId: string,
    url: string,
    headers: HeaderEntry[],
    onEvent: (event: WsEvent) => void,
  ) => {
    const channel = new Channel<WsEvent>();
    channel.onmessage = onEvent;
    return invoke<string>("ws_connect", { channel, workspaceId, url, headers });
  },
  wsSend: (connectionId: string, message: WsOutbound) =>
    invoke<void>("ws_send", { connectionId, message }),

  // Streaming (gRPC, #17d)
  // Live server reflection discovery — the reflection-to-connect handoff.
  // `target` is a bare "host:port" or a full "grpc(s)://…" URL; see
  // `grpc_discover_schema`'s doc comment for the scheme-defaulting rule.
  grpcDiscoverSchema: (workspaceId: string, target: string) =>
    invoke<GrpcSchemaDiscoveryResult>("grpc_discover_schema", { workspaceId, target }),
  grpcConnect: (
    workspaceId: string,
    args: GrpcConnectArgs,
    onEvent: (event: GrpcEvent) => void,
  ) => {
    const channel = new Channel<GrpcEvent>();
    channel.onmessage = onEvent;
    return invoke<string>("grpc_connect", { channel, workspaceId, args });
  },
  // Sends another request message on a live client-streaming/bidi
  // connection. Unary/server-streaming connections have nothing to send
  // after the initial request — the backend rejects a grpcSend on those with
  // a clean error rather than silently no-op-ing.
  grpcSend: (connectionId: string, message: GrpcOutbound) =>
    invoke<void>("grpc_send", { connectionId, message }),
  // Half-closes the request side of a live client-streaming/bidi connection
  // (no more request messages will be sent) without tearing the connection
  // down — the server doesn't reply to a client-streaming call until the
  // request side half-closes, so this is required for that mode to ever
  // produce a response. Distinct from streamDisconnect, which aborts the
  // connection outright and would lose that response.
  grpcFinishSending: (connectionId: string) =>
    invoke<void>("grpc_finish_sending", { connectionId }),

  // Disconnect any live stream (SSE/WS/gRPC) — one generic backend command.
  streamDisconnect: (connectionId: string) =>
    invoke<void>("stream_disconnect", { connectionId }),

  // Code generation
  generateCode: (
    req: HttpRequest,
    workspaceId: string,
    collectionId: string | null,
    requestId: string | null,
    target: CodegenTarget,
    options: CodegenOptions,
  ) => invoke<string>("generate_code", { req, workspaceId, collectionId, requestId, target, options }),

  // GraphQL schema introspection — a genuine live fetch (real auth/transport),
  // but not a `send_request` call: skips scripts/history, see commands/graphql.rs.
  introspectGraphqlSchema: (
    req: HttpRequest,
    workspaceId: string,
    collectionId: string | null,
    requestId: string | null,
  ) => invoke<string>("introspect_graphql_schema", { req, workspaceId, collectionId, requestId }),

  // Plugins (Phase 6 task 5) — JS plugins, sandbox-executed via `plugins::runtime`.
  listPlugins: (workspaceId: string, kind?: PluginKind | null) =>
    invoke<Plugin[]>("list_plugins", { workspaceId, kind: kind ?? null }),
  createPlugin: (workspaceId: string, input: PluginInput) =>
    invoke<Plugin>("create_plugin", { workspaceId, input }),
  updatePlugin: (id: string, input: PluginInput) => invoke<Plugin>("update_plugin", { id, input }),
  deletePlugin: (id: string) => invoke<void>("delete_plugin", { id }),
  // "Test before saving" previews — run raw, unpersisted plugin source
  // through the same sandbox the saved-plugin dispatch path uses.
  previewPluginCodegen: (source: string, req: HttpRequest, options: CodegenOptions) =>
    invoke<string>("preview_plugin_codegen", { source, req, options }),
  previewPluginImport: (source: string, content: string) =>
    invoke<ImportPreview>("preview_plugin_import", { source, content }),
  previewPluginExport: (source: string, node: ImportedNode) =>
    invoke<string>("preview_plugin_export", { source, node }),

  // Mock servers — local method+path -> canned-response stand-ins.
  listMockServers: (workspaceId: string) => invoke<MockServer[]>("list_mock_servers", { workspaceId }),
  createMockServer: (workspaceId: string, input: MockServerInput) =>
    invoke<MockServer>("create_mock_server", { workspaceId, input }),
  createMockServerFromCollection: (workspaceId: string, collectionId: string, name: string, port: number) =>
    invoke<MockServer>("create_mock_server_from_collection", { workspaceId, collectionId, name, port }),
  updateMockServer: (id: string, input: MockServerInput) => invoke<MockServer>("update_mock_server", { id, input }),
  deleteMockServer: (id: string) => invoke<void>("delete_mock_server", { id }),
  listMockRules: (mockServerId: string) => invoke<MockRule[]>("list_mock_rules", { mockServerId }),
  createMockRule: (mockServerId: string, input: MockRuleInput) =>
    invoke<MockRule>("create_mock_rule", { mockServerId, input }),
  updateMockRule: (id: string, input: MockRuleInput) => invoke<MockRule>("update_mock_rule", { id, input }),
  deleteMockRule: (id: string) => invoke<void>("delete_mock_rule", { id }),
  /** Resolves to the actually-bound port (matches the config's `port` unless
   * that was 0). Rejects if already running or if the port couldn't be bound. */
  startMockServer: (id: string) => invoke<number>("start_mock_server", { id }),
  stopMockServer: (id: string) => invoke<void>("stop_mock_server", { id }),
  listRunningMockServerIds: () => invoke<string[]>("list_running_mock_server_ids"),
  /** Config-only export (name/port/rules incl. every matcher field) — no
   * secrets live in a mock rule, unlike `exportEnvironment`, so no masking. */
  exportMockServer: (id: string) => invoke<string>("export_mock_server", { id }),
  importMockServer: (workspaceId: string, content: string) =>
    invoke<MockServer>("import_mock_server", { workspaceId, content }),

  // File-based `.restman/` sync (Phase 8) — reads folder path/format from
  // the workspace's own settings row, so both callers only ever need a
  // workspace id. `syncExport` doubles as the manual "Sync now" trigger and
  // the automatic post-mutation call in `syncMode: "live"`.
  syncExport: (workspaceId: string) => invoke<SyncExportReport>("sync_export", { workspaceId }),
  syncImport: (workspaceId: string, mode: ConflictMode) =>
    invoke<SyncImportReport>("sync_import", { workspaceId, mode }),

  // Full-app ZIP backup/restore (Phase 8) — bytes cross IPC base64-encoded,
  // same convention as `writeFileBytes`.
  createBackup: (password: string) => invoke<string>("create_backup", { password }),
  restoreBackup: (contentBase64: string, password: string) =>
    invoke<RestoreReport>("restore_backup", { contentBase64, password }),
};
