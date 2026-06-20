import { describe, expect, it } from "vitest";
import {
  base64ToBytes,
  bytesToText,
  formatBytes,
  formatHex,
  prettyJson,
} from "./encoding";

describe("encoding", () => {
  it("decodes base64 to text", () => {
    const bytes = base64ToBytes(btoa("hello"));
    expect(bytesToText(bytes)).toBe("hello");
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
});
