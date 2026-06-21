//! Frontend mirrors of the Rust Phase 2 model types (serde camelCase / tagged enums).

import type { HttpRequest, KeyValue, HeaderEntry, RequestBody, RequestOptions, HttpResponse } from "./http";

export interface Collection {
  id: string;
  workspaceId: string;
  parentId: string | null;
  name: string;
  description: string | null;
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
