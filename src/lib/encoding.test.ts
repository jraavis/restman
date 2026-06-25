import { describe, expect, it } from "vitest";
import {
  base64ToBytes,
  bytesToText,
  filterJsonValue,
  filterLines,
  formatBytes,
  formatHex,
  prettyJson,
  prettyXml,
  textToBase64,
} from "./encoding";

describe("encoding", () => {
  it("decodes base64 to text", () => {
    const bytes = base64ToBytes(btoa("hello"));
    expect(bytesToText(bytes)).toBe("hello");
  });

  it("encodes text to base64", () => {
    expect(textToBase64("hello")).toBe("aGVsbG8=");
  });

  it("round-trips non-ASCII text through textToBase64/base64ToBytes/bytesToText", () => {
    const text = "héllo wörld 日本語";
    expect(bytesToText(base64ToBytes(textToBase64(text)))).toBe(text);
  });

  it("pretty-prints valid JSON and rejects invalid", () => {
    expect(prettyJson('{"a":1}')).toBe('{\n  "a": 1\n}');
    expect(prettyJson("not json")).toBeNull();
  });

  it("formats byte sizes", () => {
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(2048)).toBe("2.0 KB");
  });

  it("produces a hex dump with offset and ascii", () => {
    const dump = formatHex(base64ToBytes(btoa("AB")));
    expect(dump).toContain("00000000");
    expect(dump).toContain("41 42");
    expect(dump).toContain("|AB|");
  });

  it("pretty-prints valid XML and rejects invalid", () => {
    expect(prettyXml("<a><b>1</b><c/></a>")).toBe("<a>\n  <b>1</b>\n  <c/>\n</a>");
    expect(prettyXml("<a><b></a>")).toBeNull();
    expect(prettyXml("not xml")).toBeNull();
  });

  it("keeps XML attributes on the element line", () => {
    expect(prettyXml('<a id="1"/>')).toBe('<a id="1"/>');
  });

  it("filters JSON by key or value substring, dropping empty branches", () => {
    const data = { name: "Alice", address: { city: "Springfield" }, age: 30 };
    expect(filterJsonValue(data, "spring")).toEqual({ address: { city: "Springfield" } });
    expect(filterJsonValue(data, "age")).toEqual({ age: 30 });
    expect(filterJsonValue(data, "nope")).toBeUndefined();
    expect(filterJsonValue(data, "")).toEqual(data);
  });

  it("filters JSON arrays, keeping only matching fields per element", () => {
    const data = [{ id: 1, tag: "x" }, { id: 2, tag: "keep" }];
    expect(filterJsonValue(data, "keep")).toEqual([{ tag: "keep" }]);
  });

  it("filters lines by substring with a no-match placeholder", () => {
    const text = "alpha\nbeta\ngamma";
    expect(filterLines(text, "eta")).toBe("beta");
    expect(filterLines(text, "zzz")).toBe("(no matching lines)");
    expect(filterLines(text, "")).toBe(text);
  });
});
