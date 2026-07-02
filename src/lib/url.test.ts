import { describe, expect, it } from "vitest";
import { decodeUrl, encodeUrl } from "./url";

describe("url", () => {
  it("encodes and decodes round-trip", () => {
    expect(encodeUrl("hello world")).toEqual({ ok: true, value: "hello%20world" });
    expect(decodeUrl("hello%20world")).toEqual({ ok: true, value: "hello world" });
  });

  it("rejects invalid encoded sequences", () => {
    expect(decodeUrl("%E0%A4%A")).toEqual({ ok: false, error: "Invalid URL-encoded sequence" });
  });
});