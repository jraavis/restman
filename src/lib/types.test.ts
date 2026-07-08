import { describe, expect, it } from "vitest";
import { applyAuthOption, authOptionValue, type RequestAuth } from "./types";

describe("authOptionValue", () => {
  it("is 'inherit' for an inherit auth", () => {
    expect(authOptionValue({ mode: "inherit" })).toBe("inherit");
  });

  it("is the concrete type for an own auth", () => {
    expect(authOptionValue({ mode: "own", type: "bearer", token: "tok", prefix: "Bearer" })).toBe("bearer");
  });
});

describe("applyAuthOption", () => {
  it("switches to inherit regardless of the current type", () => {
    const current: RequestAuth = { mode: "own", type: "bearer", token: "tok", prefix: "Bearer" };
    expect(applyAuthOption(current, "inherit")).toEqual({ mode: "inherit" });
  });

  it("starts a freshly-selected type empty rather than carrying over fields", () => {
    const current: RequestAuth = { mode: "own", type: "bearer", token: "tok", prefix: "Bearer" };
    expect(applyAuthOption(current, "basic")).toEqual({ mode: "own", type: "basic", username: "", password: "" });
  });

  it("re-selecting the type already in effect is a no-op, preserving its fields", () => {
    const current: RequestAuth = { mode: "own", type: "bearer", token: "tok", prefix: "Bearer" };
    expect(applyAuthOption(current, "bearer")).toBe(current);
  });

  it("selecting a concrete type from inherit starts that type empty", () => {
    expect(applyAuthOption({ mode: "inherit" }, "basic")).toEqual({
      mode: "own",
      type: "basic",
      username: "",
      password: "",
    });
  });
});
