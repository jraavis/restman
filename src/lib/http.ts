//! Frontend mirrors of the Rust HTTP model (serde camelCase / tagged enums).

export interface HeaderEntry {
  name: string;
  value: string;
  enabled: boolean;
}

export interface KeyValue {
  key: string;
  value: string;
  enabled: boolean;
}

export interface FormField {
  key: string;
  /** Text value, or absolute file path when `isFile`. */
  value: string;
  enabled: boolean;
  isFile: boolean;
  contentType?: string | null;
}

/** Tagged by `mode`, payload under `data` — matches serde tag/content. */
export type RequestBody =
  | { mode: "none" }
  | { mode: "json"; data: string }
  | { mode: "raw"; data: { content: string; language?: string | null } }
  | { mode: "urlEncoded"; data: KeyValue[] }
  | { mode: "formData"; data: FormField[] }
  | { mode: "binary"; data: { path: string } }
  | { mode: "graphql"; data: { query: string; variables?: string | null } };

export type BodyMode = RequestBody["mode"];

export interface RequestOptions {
  timeoutSecs: number;
  followRedirects: boolean;
  verifySsl: boolean;
  maxRedirects: number;
}

export interface HttpRequest {
  method: string;
  url: string;
  headers: HeaderEntry[];
  query: KeyValue[];
  body: RequestBody;
  options: RequestOptions;
}

export interface Timing {
  totalMs: number;
  dnsMs: number | null;
  connectMs: number | null;
  tlsMs: number | null;
  ttfbMs: number | null;
  downloadMs: number | null;
}

export interface HttpResponse {
  status: number;
  statusText: string;
  headers: HeaderEntry[];
  bodyBase64: string;
  sizeBytes: number;
  timing: Timing;
  finalUrl: string;
  httpVersion: string;
}

export function defaultRequest(): HttpRequest {
  return {
    method: "GET",
    url: "",
    headers: [],
    query: [],
    body: { mode: "none" },
    options: {
      timeoutSecs: 30,
      followRedirects: true,
      verifySsl: true,
      maxRedirects: 10,
    },
  };
}

/** A fresh empty `data` payload for each body mode. */
export function emptyBody(mode: BodyMode): RequestBody {
  switch (mode) {
    case "none":
      return { mode: "none" };
    case "json":
      return { mode: "json", data: "" };
    case "raw":
      return { mode: "raw", data: { content: "", language: "text" } };
    case "urlEncoded":
      return { mode: "urlEncoded", data: [] };
    case "formData":
      return { mode: "formData", data: [] };
    case "binary":
      return { mode: "binary", data: { path: "" } };
    case "graphql":
      return { mode: "graphql", data: { query: "", variables: "" } };
  }
}

export const COMMON_HEADERS = [
  "Accept",
  "Accept-Encoding",
  "Authorization",
  "Cache-Control",
  "Content-Type",
  "Cookie",
  "Origin",
  "Referer",
  "User-Agent",
];
