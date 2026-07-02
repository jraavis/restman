import { describe, expect, it } from "vitest";
import { jsonPathAtOffset, pathToPmExpression, stripJsonStringQuotes } from "./jsonPath";

describe("jsonPathAtOffset", () => {
  const json = JSON.stringify({ token: "abc", data: { count: 42, "weird-key": true } }, null, 2);

  it("finds a top-level string value", () => {
    const tokenIdx = json.indexOf('"abc"');
    expect(jsonPathAtOffset(json, tokenIdx + 2)).toEqual(["token"]);
  });

  it("finds a nested number value", () => {
    const countIdx = json.indexOf("42");
    expect(jsonPathAtOffset(json, countIdx)).toEqual(["data", "count"]);
  });

  it("uses bracket notation path for odd keys", () => {
    const idx = json.indexOf("true");
    expect(jsonPathAtOffset(json, idx)).toEqual(["data", "weird-key"]);
  });

  it("returns null outside any value", () => {
    expect(jsonPathAtOffset(json, 0)).toBeNull();
  });

  it("returns null for invalid JSON", () => {
    expect(jsonPathAtOffset("{bad", 1)).toBeNull();
  });
});

describe("pathToPmExpression", () => {
  it("uses dot notation for simple keys", () => {
    expect(pathToPmExpression(["data", "token"])).toBe("pm.response.json().data.token");
  });

  it("uses bracket notation for numeric indices", () => {
    expect(pathToPmExpression(["items", 0, "id"])).toBe("pm.response.json().items[0].id");
  });

  it("uses bracket notation for odd keys", () => {
    expect(pathToPmExpression(["data", "weird-key"])).toBe('pm.response.json().data["weird-key"]');
  });
});

describe("stripJsonStringQuotes", () => {
  it("removes JSON string quotes", () => {
    expect(stripJsonStringQuotes('"hello"')).toBe("hello");
  });

  it("leaves non-quoted text unchanged", () => {
    expect(stripJsonStringQuotes("hello")).toBe("hello");
  });
});