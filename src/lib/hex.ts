//! Hex encode/decode helpers for the developer tools panel.

import type { ToolResult } from "./encoding";

export function textToHex(text: string): ToolResult<string> {
  const bytes = new TextEncoder().encode(text);
  const hex = Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join(" ");
  return { ok: true, value: hex };
}

export function hexToText(hex: string): ToolResult<string> {
  const cleaned = hex.replace(/\s+/g, "").replace(/^0x/i, "");
  if (!cleaned) return { ok: false, error: "Input is empty" };
  if (!/^[0-9a-fA-F]*$/.test(cleaned)) return { ok: false, error: "Invalid hex characters" };
  if (cleaned.length % 2 !== 0) return { ok: false, error: "Hex length must be even" };
  const bytes = new Uint8Array(cleaned.length / 2);
  for (let i = 0; i < bytes.length; i++) {
    bytes[i] = parseInt(cleaned.slice(i * 2, i * 2 + 2), 16);
  }
  return { ok: true, value: new TextDecoder("utf-8", { fatal: false }).decode(bytes) };
}