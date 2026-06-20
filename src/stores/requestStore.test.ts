import { beforeEach, describe, expect, it } from "vitest";
import { defaultRequest } from "../lib/http";
import { useRequestStore } from "./requestStore";

describe("requestStore", () => {
  beforeEach(() => {
    useRequestStore.setState({
      request: defaultRequest(),
      response: null,
      sending: false,
      error: null,
    });
  });

  it("updates method and url", () => {
    useRequestStore.getState().setMethod("POST");
    useRequestStore.getState().setUrl("https://x.test");
    expect(useRequestStore.getState().request.method).toBe("POST");
    expect(useRequestStore.getState().request.url).toBe("https://x.test");
  });

  it("merges options without dropping others", () => {
    useRequestStore.getState().setOptions({ verifySsl: false });
    const opts = useRequestStore.getState().request.options;
    expect(opts.verifySsl).toBe(false);
    expect(opts.followRedirects).toBe(true);
    expect(opts.timeoutSecs).toBe(30);
  });

  it("tracks send lifecycle", () => {
    useRequestStore.getState().beginSend();
    expect(useRequestStore.getState().sending).toBe(true);

    useRequestStore.getState().setError("boom");
    expect(useRequestStore.getState().sending).toBe(false);
    expect(useRequestStore.getState().error).toBe("boom");
  });
});
