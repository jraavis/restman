//! Tests for CollectionRunner's data-driven-run sample-data helpers.

import { describe, it, expect } from "vitest";
import type { SavedRequest } from "../../lib/types";
import { buildCsvSample, buildJsonSample, extractTemplateVarNames } from "./runnerSampleData";

/** Only the fields `extractTemplateVarNames` actually scans (via
 * `JSON.stringify`) need to be populated for these tests. */
function partialRequest(fields: Partial<SavedRequest>): SavedRequest {
  return fields as SavedRequest;
}

describe("extractTemplateVarNames", () => {
  it("finds a var referenced in the URL", () => {
    const requests = [partialRequest({ url: "https://api.example.com/users/{{userId}}" })];
    expect(extractTemplateVarNames(requests)).toEqual(["userId"]);
  });

  it("finds vars nested inside headers, query, and body", () => {
    const requests = [
      partialRequest({
        url: "https://api.example.com",
        headers: [{ name: "Authorization", value: "Bearer {{token}}", enabled: true }],
        query: [{ key: "page", value: "{{pageNum}}", enabled: true }],
        body: { mode: "json", data: '{"id": "{{recordId}}"}' },
      }),
    ];
    expect(extractTemplateVarNames(requests)).toEqual(["pageNum", "recordId", "token"]);
  });

  it("dedupes a var referenced multiple times across requests", () => {
    const requests = [
      partialRequest({ url: "https://api.example.com/{{userId}}" }),
      partialRequest({ url: "https://api.example.com/{{userId}}/orders" }),
    ];
    expect(extractTemplateVarNames(requests)).toEqual(["userId"]);
  });

  it("sorts results alphabetically for a deterministic sample", () => {
    const requests = [partialRequest({ url: "https://api.example.com/{{zeta}}/{{alpha}}" })];
    expect(extractTemplateVarNames(requests)).toEqual(["alpha", "zeta"]);
  });

  it("returns an empty array when no requests reference any {{vars}}", () => {
    const requests = [partialRequest({ url: "https://api.example.com/users" })];
    expect(extractTemplateVarNames(requests)).toEqual([]);
  });

  it("ignores env/collection interpolation syntax it can't parse as a name (e.g. empty braces)", () => {
    const requests = [partialRequest({ url: "https://api.example.com/{{}}/{{userId}}" })];
    expect(extractTemplateVarNames(requests)).toEqual(["userId"]);
  });
});

describe("buildCsvSample", () => {
  it("falls back to a generic id/name example when no vars are known", () => {
    expect(buildCsvSample([])).toBe("id,name\n1,Alice\n2,Bob");
  });

  it("builds a header + two self-describing rows from real var names", () => {
    expect(buildCsvSample(["userId", "token"])).toBe("userId,token\nuserId1,token1\nuserId2,token2");
  });
});

describe("buildJsonSample", () => {
  it("falls back to a generic id/name example when no vars are known", () => {
    expect(JSON.parse(buildJsonSample([]))).toEqual([
      { id: "1", name: "Alice" },
      { id: "2", name: "Bob" },
    ]);
  });

  it("builds two self-describing rows from real var names", () => {
    expect(JSON.parse(buildJsonSample(["userId", "token"]))).toEqual([
      { userId: "userId1", token: "token1" },
      { userId: "userId2", token: "token2" },
    ]);
  });
});
