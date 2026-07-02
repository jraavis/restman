import { describe, expect, it } from "vitest";
import { parseTimestamp } from "./timestamp";

describe("timestamp", () => {
  const now = Date.parse("2026-01-15T12:00:00.000Z");

  it("parses unix seconds", () => {
    const result = parseTimestamp("1516239022", now);
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.value.inputKind).toBe("seconds");
      expect(result.value.isoUtc).toBe("2018-01-18T01:30:22.000Z");
    }
  });

  it("parses unix milliseconds", () => {
    const result = parseTimestamp("1516239022000", now);
    expect(result.ok).toBe(true);
    if (result.ok) expect(result.value.inputKind).toBe("milliseconds");
  });

  it("parses ISO strings", () => {
    const result = parseTimestamp("2026-01-15T12:00:00.000Z", now);
    expect(result.ok).toBe(true);
    if (result.ok) expect(result.value.inputKind).toBe("iso");
  });
});