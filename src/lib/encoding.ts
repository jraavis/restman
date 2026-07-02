//! Helpers for converting between text/bytes and the base64 the backend
//! reads (file writes) and sends (response bodies).

export type ToolResult<T> = { ok: true; value: T } | { ok: false; error: string };

export function base64ToBytes(b64: string): Uint8Array {
  const bin = atob(b64);
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  return bytes;
}

export function bytesToText(bytes: Uint8Array): string {
  return new TextDecoder("utf-8", { fatal: false }).decode(bytes);
}

/** UTF-8 safe encode for `ipc.writeFileBytes`, which expects base64. */
export function textToBase64(text: string): string {
  const bytes = new TextEncoder().encode(text);
  let bin = "";
  for (let i = 0; i < bytes.length; i++) bin += String.fromCharCode(bytes[i]);
  return btoa(bin);
}

function normalizeBase64Url(input: string): string {
  const b64 = input.replace(/-/g, "+").replace(/_/g, "/");
  const pad = b64.length % 4;
  if (pad === 0) return b64;
  return b64 + "=".repeat(4 - pad);
}

export function base64UrlToBytes(input: string): Uint8Array {
  return base64ToBytes(normalizeBase64Url(input.trim()));
}

export function bytesToBase64Url(bytes: Uint8Array): string {
  let bin = "";
  for (let i = 0; i < bytes.length; i++) bin += String.fromCharCode(bytes[i]);
  return btoa(bin).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

export function decodeBase64(input: string): ToolResult<string> {
  const trimmed = input.trim();
  if (!trimmed) return { ok: false, error: "Input is empty" };
  try {
    return { ok: true, value: bytesToText(base64ToBytes(trimmed)) };
  } catch {
    return { ok: false, error: "Invalid base64" };
  }
}

export function encodeBase64(input: string): ToolResult<string> {
  if (!input) return { ok: true, value: "" };
  try {
    return { ok: true, value: textToBase64(input) };
  } catch {
    return { ok: false, error: "Could not encode to base64" };
  }
}

/** Reformat JSON text with indentation; returns null if not valid JSON. */
export function prettyJson(text: string): string | null {
  try {
    return JSON.stringify(JSON.parse(text), null, 2);
  } catch {
    return null;
  }
}

/** Minify JSON text to a single line; returns null if not valid JSON. */
export function minifyJson(text: string): string | null {
  try {
    return JSON.stringify(JSON.parse(text));
  } catch {
    return null;
  }
}

/** Reformat XML/HTML-ish markup with 2-space indentation; returns null if
 * the text doesn't parse as XML (callers fall back to plain text). */
export function prettyXml(text: string): string | null {
  if (!text.trim().startsWith("<")) return null;
  const doc = new DOMParser().parseFromString(text, "application/xml");
  if (doc.querySelector("parsererror")) return null;
  if (!doc.documentElement) return null;

  const lines: string[] = [];
  const walk = (el: Element, depth: number) => {
    const indent = "  ".repeat(depth);
    const attrs = Array.from(el.attributes)
      .map((a) => ` ${a.name}="${a.value}"`)
      .join("");
    const children = Array.from(el.childNodes).filter(
      (n) => n.nodeType !== Node.TEXT_NODE || !!n.textContent?.trim(),
    );
    const childElements = children.filter((n) => n.nodeType === Node.ELEMENT_NODE);
    if (children.length === 0) {
      lines.push(`${indent}<${el.tagName}${attrs}/>`);
    } else if (childElements.length === 0) {
      const inline = children.map((n) => n.textContent?.trim() ?? "").join("");
      lines.push(`${indent}<${el.tagName}${attrs}>${inline}</${el.tagName}>`);
    } else {
      lines.push(`${indent}<${el.tagName}${attrs}>`);
      childElements.forEach((n) => walk(n as Element, depth + 1));
      lines.push(`${indent}</${el.tagName}>`);
    }
  };
  walk(doc.documentElement, 0);
  return lines.join("\n");
}

/** Keep only object/array entries whose key or primitive value contains
 * `query` (case-insensitive); a matching key keeps its whole subtree.
 * Returns `undefined` when nothing in the subtree matches. */
export function filterJsonValue(value: unknown, query: string): unknown {
  const q = query.toLowerCase();
  if (!q) return value;
  if (Array.isArray(value)) {
    const out = value.map((v) => filterJsonValue(v, query)).filter((v) => v !== undefined);
    return out.length > 0 ? out : undefined;
  }
  if (value !== null && typeof value === "object") {
    const out: Record<string, unknown> = {};
    for (const [k, v] of Object.entries(value as Record<string, unknown>)) {
      if (k.toLowerCase().includes(q)) {
        out[k] = v;
        continue;
      }
      const filtered = filterJsonValue(v, query);
      if (filtered !== undefined) out[k] = filtered;
    }
    return Object.keys(out).length > 0 ? out : undefined;
  }
  return String(value).toLowerCase().includes(q) ? value : undefined;
}

/** Line-based filter for non-JSON bodies: keep lines containing `query`. */
export function filterLines(text: string, query: string): string {
  if (!query.trim()) return text;
  const q = query.toLowerCase();
  const lines = text.split("\n").filter((l) => l.toLowerCase().includes(q));
  return lines.length > 0 ? lines.join("\n") : "(no matching lines)";
}

/** Classic `xxd`-style hex + ASCII dump. */
export function formatHex(bytes: Uint8Array, maxBytes = 64 * 1024): string {
  const limit = Math.min(bytes.length, maxBytes);
  const lines: string[] = [];
  for (let off = 0; off < limit; off += 16) {
    const slice = bytes.subarray(off, off + 16);
    const hex: string[] = [];
    let ascii = "";
    for (let i = 0; i < 16; i++) {
      if (i < slice.length) {
        hex.push(slice[i].toString(16).padStart(2, "0"));
        const c = slice[i];
        ascii += c >= 0x20 && c < 0x7f ? String.fromCharCode(c) : ".";
      } else {
        hex.push("  ");
      }
    }
    const offStr = off.toString(16).padStart(8, "0");
    const hexStr = `${hex.slice(0, 8).join(" ")}  ${hex.slice(8).join(" ")}`;
    lines.push(`${offStr}  ${hexStr}  |${ascii}|`);
  }
  if (bytes.length > limit) lines.push(`… ${bytes.length - limit} more bytes`);
  return lines.join("\n");
}

export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1024 * 1024)).toFixed(2)} MB`;
}

export function formatMs(ms: number): string {
  if (ms < 1000) return `${ms.toFixed(0)} ms`;
  return `${(ms / 1000).toFixed(2)} s`;
}
