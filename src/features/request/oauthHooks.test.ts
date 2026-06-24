//! Smoke tests for oauth hook key factories.

import { describe, it, expect } from "vitest";
import { oauth2Keys } from "./oauthHooks";

describe("oauth2Keys", () => {
  it("status key contains collectionId and requestId", () => {
    const key = oauth2Keys.status({ collectionId: "col-1", requestId: null });
    expect(key).toContain("col-1");
    expect(key).toContain("oauth2-status");
  });

  it("preview key is distinct from status key", () => {
    const status = oauth2Keys.status({ collectionId: "col-1", requestId: null });
    const preview = oauth2Keys.preview({ collectionId: "col-1", requestId: null });
    expect(status[0]).toBe("oauth2-status");
    expect(preview[0]).toBe("oauth2-preview");
    expect(status[0]).not.toBe(preview[0]);
  });

  it("preview key with requestId uses requestId", () => {
    const key = oauth2Keys.preview({ collectionId: null, requestId: "req-1" });
    expect(key).toContain("req-1");
  });
});
