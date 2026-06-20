//! Helpers for decoding the base64 response body the backend sends.

export function base64ToBytes(b64: string): Uint8Array {
  const bin = atob(b64);
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  return bytes;
}

export function bytesToText(bytes: Uint8Array): string {
  return new TextDecoder("utf-8", { fatal: false }).decode(bytes);
}

/** Reformat JSON text with indentation; returns null if not valid JSON. */
export function prettyJson(text: string): string | null {
  try {
    return JSON.stringify(JSON.parse(text), null, 2);
  } catch {
    return null;
  }
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
