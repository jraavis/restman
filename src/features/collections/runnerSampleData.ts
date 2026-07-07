//! Pure helpers behind `CollectionRunner`'s "Insert CSV/JSON sample" buttons
//! for its data-driven-run field — instead of guessing the expected shape
//! from a placeholder string, offer a starter sample built from the
//! collection's own `{{template}}` variables where possible.

import type { SavedRequest } from "../../lib/types";

const TEMPLATE_VAR_RE = /\{\{\s*([a-zA-Z_][\w.-]*)\s*\}\}/g;

/** Every distinct `{{name}}` referenced anywhere across `requests` — url,
 * headers, query, body (any of its ~7 modes), scripts — found via a JSON
 * round-trip rather than walking each field/body-mode individually, since a
 * template var can legitimately appear in any of them. Sorted for a
 * deterministic sample across runs. This can't distinguish a var meant to
 * vary per data-file row from one that's really an environment/collection
 * variable (always the same) — both look identical as `{{name}}` — so it's
 * a starting point to prune, not a precise per-iteration-variable detector. */
export function extractTemplateVarNames(requests: SavedRequest[]): string[] {
  const names = new Set<string>();
  for (const req of requests) {
    for (const match of JSON.stringify(req).matchAll(TEMPLATE_VAR_RE)) {
      names.add(match[1]);
    }
  }
  return [...names].sort();
}

const FALLBACK_CSV = "id,name\n1,Alice\n2,Bob";
const FALLBACK_JSON = JSON.stringify(
  [
    { id: "1", name: "Alice" },
    { id: "2", name: "Bob" },
  ],
  null,
  2,
);

/** Two illustrative rows — each cell named after its own column (e.g.
 * `userId1`, `userId2`) so it's obvious at a glance which placeholder maps
 * to which var, and obviously meant to be replaced with real values. */
export function buildCsvSample(varNames: string[]): string {
  if (varNames.length === 0) return FALLBACK_CSV;
  const row = (n: number) => varNames.map((v) => `${v}${n}`).join(",");
  return [varNames.join(","), row(1), row(2)].join("\n");
}

export function buildJsonSample(varNames: string[]): string {
  if (varNames.length === 0) return FALLBACK_JSON;
  const row = (n: number) => Object.fromEntries(varNames.map((v) => [v, `${v}${n}`]));
  return JSON.stringify([row(1), row(2)], null, 2);
}
