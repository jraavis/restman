import { describe, expect, it } from "vitest";
import { decodeJwt, verifyJwt } from "./jwt";

// Signature uses base64url `_` (not a typo of the common jwt.io paste).
const HS256_TOKEN =
  "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyfQ.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c";
const HS256_SECRET = "your-256-bit-secret";

describe("jwt", () => {
  it("decodes a standard JWT", () => {
    const result = decodeJwt(HS256_TOKEN);
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.value.header.alg).toBe("HS256");
      expect(result.value.payload.sub).toBe("1234567890");
      expect(result.value.payload.name).toBe("John Doe");
    }
  });

  it("strips Bearer prefix", () => {
    const result = decodeJwt(`Bearer ${HS256_TOKEN}`);
    expect(result.ok).toBe(true);
  });

  it("rejects malformed tokens", () => {
    expect(decodeJwt("only.two")).toEqual({ ok: false, error: "JWT must have exactly 3 dot-separated parts" });
  });

  it("verifies HS256 with correct secret", async () => {
    const result = await verifyJwt(HS256_TOKEN, HS256_SECRET);
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.value.verified).toBe(true);
      expect(result.value.algorithm).toBe("HS256");
    }
  });

  it("fails verification with wrong secret", async () => {
    const result = await verifyJwt(HS256_TOKEN, "wrong-secret");
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.value.verified).toBe(false);
    }
  });
});