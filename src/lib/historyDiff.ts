//! Pure helpers turning two `HistoryEntry` snapshots into side-by-side diff
//! rows. No backend involvement — `HistoryEntry.request`/`.response` already
//! carry full snapshots (see `store/history.rs`), so there's nothing to
//! fetch beyond the two entries the user picked.

import { diffLines } from "diff";
import { prettyJson, prettyXml, base64ToBytes, bytesToText } from "./encoding";
import type { HeaderEntry, RequestBody } from "./http";
import type { HistoryEntry } from "./types";

export interface DiffRow {
  left: string | null;
  right: string | null;
  kind: "same" | "removed" | "added";
}

/** Turns a `diffLines` change list into row-aligned pairs for a two-column
 * view: removed lines only occupy the left column, added lines only the
 * right, unchanged lines occupy both — the same simplified alignment most
 * lightweight diff viewers use for adjacent removed+added pairs (a changed
 * line reads as one red row directly above one green row, not a single
 * merged row). */
export function buildSideBySideDiff(before: string, after: string): DiffRow[] {
  const changes = diffLines(before, after);
  const rows: DiffRow[] = [];
  for (const change of changes) {
    const lines = change.value.split("\n");
    // A trailing "" from the final split is an artifact of a trailing
    // newline in the source text, not a real blank line — drop it.
    if (lines.length > 0 && lines[lines.length - 1] === "") lines.pop();
    for (const line of lines) {
      if (change.added) rows.push({ left: null, right: line, kind: "added" });
      else if (change.removed) rows.push({ left: line, right: null, kind: "removed" });
      else rows.push({ left: line, right: line, kind: "same" });
    }
  }
  return rows;
}

/** Sorted `name: value` lines for enabled headers only — matches what's
 * actually sent/received, not disabled-but-saved rows. */
export function headersToLines(headers: HeaderEntry[]): string {
  return headers
    .filter((h) => h.enabled)
    .map((h) => `${h.name}: ${h.value}`)
    .sort()
    .join("\n");
}

/** Best-effort readable text for any request body mode — pretty-prints JSON
 * where the mode implies it, otherwise falls back to the raw text a user
 * would recognize from the Body tab. */
export function requestBodyToText(body: RequestBody): string {
  switch (body.mode) {
    case "none":
      return "";
    case "json":
      return prettyJson(body.data) ?? body.data;
    case "raw":
      return prettyJson(body.data.content) ?? prettyXml(body.data.content) ?? body.data.content;
    case "urlEncoded":
      return body.data
        .filter((kv) => kv.enabled)
        .map((kv) => `${kv.key}=${kv.value}`)
        .join("\n");
    case "formData":
      return body.data
        .filter((f) => f.enabled)
        .map((f) => (f.isFile ? `${f.key}=@${f.value}` : `${f.key}=${f.value}`))
        .join("\n");
    case "binary":
      return `@${body.data.path}`;
    case "graphql":
      return [body.data.query, body.data.operationName ? `# operationName: ${body.data.operationName}` : "", body.data.variables ?? ""]
        .filter(Boolean)
        .join("\n\n");
  }
}

/** Best-effort readable text for a response body — decodes the base64
 * transport encoding, then pretty-prints JSON/XML where recognizable. */
export function responseBodyToText(bodyBase64: string): string {
  const text = bytesToText(base64ToBytes(bodyBase64));
  return prettyJson(text) ?? prettyXml(text) ?? text;
}

export interface HistoryDiffSection {
  label: string;
  before: string;
  after: string;
}

/** All the diffable sections for two history entries — method+URL, headers,
 * body for both request and response, in that display order. */
export function historyDiffSections(a: HistoryEntry, b: HistoryEntry): HistoryDiffSection[] {
  return [
    {
      label: "Request line",
      before: `${a.request.method} ${a.request.url}`,
      after: `${b.request.method} ${b.request.url}`,
    },
    { label: "Request headers", before: headersToLines(a.request.headers), after: headersToLines(b.request.headers) },
    {
      label: "Request body",
      before: requestBodyToText(a.request.body),
      after: requestBodyToText(b.request.body),
    },
    {
      label: "Response status",
      before: a.response ? `${a.response.status} ${a.response.statusText}` : a.error ?? "(no response)",
      after: b.response ? `${b.response.status} ${b.response.statusText}` : b.error ?? "(no response)",
    },
    {
      label: "Response headers",
      before: a.response ? headersToLines(a.response.headers) : "",
      after: b.response ? headersToLines(b.response.headers) : "",
    },
    {
      label: "Response body",
      before: a.response ? responseBodyToText(a.response.bodyBase64) : "",
      after: b.response ? responseBodyToText(b.response.bodyBase64) : "",
    },
  ];
}
