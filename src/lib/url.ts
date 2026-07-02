//! URL encode/decode helpers for the developer tools panel.

import type { ToolResult } from "./encoding";

export function encodeUrl(text: string): ToolResult<string> {
  if (!text) return { ok: true, value: "" };
  try {
    return { ok: true, value: encodeURIComponent(text) };
  } catch {
    return { ok: false, error: "Could not URL-encode input" };
  }
}

export function decodeUrl(text: string): ToolResult<string> {
  const trimmed = text.trim();
  if (!trimmed) return { ok: false, error: "Input is empty" };
  try {
    return { ok: true, value: decodeURIComponent(trimmed) };
  } catch {
    return { ok: false, error: "Invalid URL-encoded sequence" };
  }
}