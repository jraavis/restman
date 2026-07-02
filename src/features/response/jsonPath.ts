//! Map a cursor offset in formatted JSON text to a value path for
//! `pm.response.json().<path>` expressions.

export type JsonPath = (string | number)[];

/** Return the JSON path to the value at `offset`, or null if not inside a value. */
export function jsonPathAtOffset(jsonText: string, offset: number): JsonPath | null {
  let parsed: unknown;
  try {
    parsed = JSON.parse(jsonText);
  } catch {
    return null;
  }
  if (offset < 0 || offset > jsonText.length) return null;

  const path = findPathAtOffset(jsonText, offset);
  if (!path) return null;

  // Verify the path resolves to a real value in the parsed tree.
  let cur: unknown = parsed;
  for (const seg of path) {
    if (cur == null || typeof cur !== "object") return null;
    cur = (cur as Record<string | number, unknown>)[seg];
  }
  return path;
}

function findPathAtOffset(text: string, offset: number): JsonPath | null {
  const stack: { path: JsonPath; start: number; end: number; kind: "value" }[] = [];
  let i = 0;
  const len = text.length;

  function skipWs() {
    while (i < len && /\s/.test(text[i]!)) i++;
  }

  function parseValue(path: JsonPath): boolean {
    skipWs();
    const valueStart = i;

    if (text[i] === '"') {
      i++;
      while (i < len) {
        if (text[i] === "\\") {
          i += 2;
          continue;
        }
        if (text[i] === '"') {
          i++;
          stack.push({ path, start: valueStart, end: i, kind: "value" });
          return true;
        }
        i++;
      }
      return false;
    }

    if (text.startsWith("true", i)) {
      i += 4;
      stack.push({ path, start: valueStart, end: i, kind: "value" });
      return true;
    }
    if (text.startsWith("false", i)) {
      i += 5;
      stack.push({ path, start: valueStart, end: i, kind: "value" });
      return true;
    }
    if (text.startsWith("null", i)) {
      i += 4;
      stack.push({ path, start: valueStart, end: i, kind: "value" });
      return true;
    }

    if (text[i] === "-" || (text[i]! >= "0" && text[i]! <= "9")) {
      if (text[i] === "-") i++;
      while (i < len && text[i]! >= "0" && text[i]! <= "9") i++;
      if (text[i] === ".") {
        i++;
        while (i < len && text[i]! >= "0" && text[i]! <= "9") i++;
      }
      if (text[i] === "e" || text[i] === "E") {
        i++;
        if (text[i] === "+" || text[i] === "-") i++;
        while (i < len && text[i]! >= "0" && text[i]! <= "9") i++;
      }
      stack.push({ path, start: valueStart, end: i, kind: "value" });
      return true;
    }

    if (text[i] === "[") {
      i++;
      skipWs();
      let idx = 0;
      if (text[i] === "]") {
        i++;
        return true;
      }
      while (i < len) {
        if (!parseValue([...path, idx])) return false;
        skipWs();
        if (text[i] === "]") {
          i++;
          return true;
        }
        if (text[i] !== ",") return false;
        i++;
        idx++;
        skipWs();
      }
      return false;
    }

    if (text[i] === "{") {
      i++;
      skipWs();
      if (text[i] === "}") {
        i++;
        return true;
      }
      while (i < len) {
        skipWs();
        if (text[i] !== '"') return false;
        i++;
        const keyStart = i;
        while (i < len) {
          if (text[i] === "\\") {
            i += 2;
            continue;
          }
          if (text[i] === '"') break;
          i++;
        }
        if (i >= len) return false;
        const key = text.slice(keyStart, i);
        i++;
        skipWs();
        if (text[i] !== ":") return false;
        i++;
        if (!parseValue([...path, key])) return false;
        skipWs();
        if (text[i] === "}") {
          i++;
          return true;
        }
        if (text[i] !== ",") return false;
        i++;
      }
      return false;
    }

    return false;
  }

  skipWs();
  if (!parseValue([])) return null;

  for (const entry of stack) {
    if (offset >= entry.start && offset <= entry.end) {
      return entry.path.length > 0 ? entry.path : null;
    }
  }
  return null;
}

/** Build a `pm.response.json()` accessor for a path segment list. */
export function pathToPmExpression(path: JsonPath): string {
  if (path.length === 0) return "pm.response.json()";
  let expr = "pm.response.json()";
  for (const seg of path) {
    if (typeof seg === "number") {
      expr += `[${seg}]`;
    } else if (/^[A-Za-z_$][A-Za-z0-9_$]*$/.test(seg)) {
      expr += `.${seg}`;
    } else {
      expr += `[${JSON.stringify(seg)}]`;
    }
  }
  return expr;
}

/** Strip surrounding JSON string quotes from a selection when present. */
export function stripJsonStringQuotes(text: string): string {
  const t = text.trim();
  if (t.length >= 2 && t.startsWith('"') && t.endsWith('"')) {
    try {
      return JSON.parse(t) as string;
    } catch {
      return t.slice(1, -1);
    }
  }
  return text;
}