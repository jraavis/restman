import { describe, expect, it } from "vitest";
import { hexToText, textToHex } from "./hex";

describe("hex", () => {
  it("round-trips text through hex", () => {
    const encoded = textToHex("AB");
    expect(encoded).toEqual({ ok: true, value: "41 42" });
    expect(hexToText("41 42")).toEqual({ ok: true, value: "AB" });
  });

  it("rejects odd-length hex", () => {
    expect(hexToText("abc")).toEqual({ ok: false, error: "Hex length must be even" });
  });
});