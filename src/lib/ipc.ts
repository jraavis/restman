//! Typed wrappers around Tauri IPC commands. The frontend never touches the
//! network or disk directly — every backend operation goes through here.

import { invoke } from "@tauri-apps/api/core";
import type { HttpRequest, HttpResponse } from "./http";
import type {
  AuthConfig,
  CodeLanguage,
  CodegenOptions,
  Collection,
  CollectionRunOptions,
  CollectionRunSummary,
  ConflictMode,
  Environment,
  EnvironmentImportReport,
  EnvironmentPreview,
  ExportFormat,
  HistoryEntry,
  HistoryFilter,
  ImportFormat,
  ImportedNode,
  ImportPreview,
  ImportReport,
  OAuth2Status,
  SavedRequest,
  SavedRequestInput,
  SearchHit,
  SendResponse,
  Tab,
  Tag,
  VarScope,
  Variable,
  VariableInput,
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

  // Import / export
  previewImport: (format: ImportFormat, content: string) =>
    invoke<ImportPreview>("preview_import", { format, content }),
  applyCollectionImport: (
    workspaceId: string,
    parentId: string | null,
    root: ImportedNode,
    mode: ConflictMode,
  ) => invoke<ImportReport>("apply_collection_import", { workspaceId, parentId, root, mode }),
  exportCollection: (collectionId: string, format: ExportFormat) =>
    invoke<string>("export_collection", { collectionId, format }),

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

  // Files
  writeFileBytes: (path: string, contentBase64: string) =>
    invoke<void>("write_file_bytes", { path, contentBase64 }),

  // Code generation
  generateCode: (
    req: HttpRequest,
    workspaceId: string,
    collectionId: string | null,
    requestId: string | null,
    language: CodeLanguage,
    options: CodegenOptions,
  ) => invoke<string>("generate_code", { req, workspaceId, collectionId, requestId, language, options }),
};
