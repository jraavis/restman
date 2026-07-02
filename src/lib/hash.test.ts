import { describe, expect, it } from "vitest";
import { hashText } from "./hash";

describe("hash", () => {
  it("computes SHA-256", async () => {
    const result = await hashText("hello", "sha256");
    expect(result).toEqual({
      ok: true,
      value: "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
    });
  });

  it("computes MD5", async () => {
    const result = await hashText("hello", "md5");
    expect(result).toEqual({ ok: true, value: "5d41402abc4b2a76b9719d911017c592" });
  });
});