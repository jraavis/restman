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
  | { mode: "graphql"; data: { query: string; variables?: string | null; operationName?: string | null } };

export type BodyMode = RequestBody["mode"];

export interface RequestOptions {
  timeoutSecs: number;
  followRedirects: boolean;
  verifySsl: boolean;
  maxRedirects: number;
  /** When true, the engine uses the shared cookie jar: Set-Cookie responses
   * are stored and Cookie headers replayed on subsequent sends. Mirrors
   * the Rust `RequestOptions::send_cookies` field. */
  sendCookies: boolean;
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

/** The `Content-Type` header value (mime type only, no `; charset=…`), or
 * null if the response didn't send one. */
export function contentTypeOf(headers: HeaderEntry[]): string | null {
  const h = headers.find((x) => x.name.toLowerCase() === "content-type");
  if (!h) return null;
  return h.value.split(";")[0]?.trim().toLowerCase() || null;
}

/** Map a mime type to a Monaco editor language id for syntax highlighting. */
export function monacoLanguageFor(contentType: string | null): string {
  if (!contentType) return "plaintext";
  if (contentType.includes("json")) return "json";
  if (contentType.includes("xml")) return "xml";
  if (contentType.includes("html")) return "html";
  if (contentType.includes("css")) return "css";
  if (contentType.includes("javascript") || contentType.includes("ecmascript")) return "javascript";
  return "plaintext";
}

/** Filename extension to suggest when saving a response body to disk. */
export function extensionFor(contentType: string | null): string {
  if (!contentType) return "txt";
  if (contentType.includes("json")) return "json";
  if (contentType.includes("xml")) return "xml";
  if (contentType.includes("html")) return "html";
  if (contentType.includes("css")) return "css";
  if (contentType.includes("javascript")) return "js";
  if (contentType.includes("csv")) return "csv";
  if (contentType.startsWith("image/")) return contentType.split("/")[1] ?? "bin";
  return "txt";
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
      sendCookies: false,
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
