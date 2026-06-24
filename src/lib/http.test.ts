import { describe, expect, it } from "vitest";
import { contentTypeOf, extensionFor, monacoLanguageFor } from "./http";
import type { HeaderEntry } from "./http";

const headers = (contentType: string | null): HeaderEntry[] =>
  contentType ? [{ name: "Content-Type", value: contentType, enabled: true }] : [];

describe("contentTypeOf", () => {
  it("extracts the mime type, stripping charset and case", () => {
    expect(contentTypeOf(headers("Application/JSON; charset=utf-8"))).toBe("application/json");
  });

  it("returns null when there is no content-type header", () => {
    expect(contentTypeOf(headers(null))).toBeNull();
  });
});

describe("monacoLanguageFor", () => {
  it.each([
    ["application/json", "json"],
    ["application/xml", "xml"],
    ["text/html", "html"],
    ["text/css", "css"],
    ["application/javascript", "javascript"],
    ["text/plain", "plaintext"],
    [null, "plaintext"],
  ] as const)("maps %s to %s", (contentType, lang) => {
    expect(monacoLanguageFor(contentType)).toBe(lang);
  });
});

describe("extensionFor", () => {
  it.each([
    ["application/json", "json"],
    ["application/xml", "xml"],
    ["text/html", "html"],
    ["text/css", "css"],
    ["application/javascript", "js"],
    ["text/csv", "csv"],
    ["image/png", "png"],
    ["text/plain", "txt"],
    [null, "txt"],
  ] as const)("maps %s to .%s", (contentType, ext) => {
    expect(extensionFor(contentType)).toBe(ext);
  });
});
