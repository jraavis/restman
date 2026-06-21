import { beforeEach, describe, expect, it } from "vitest";
import { defaultRequest } from "../lib/http";
import { useRequestStore } from "./requestStore";

describe("requestStore", () => {
  beforeEach(() => {
    useRequestStore.setState({
      activeTabId: null,
      requestId: null,
      collectionId: null,
      title: "Untitled",
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

  it("loadTab replaces the draft wholesale and clears stale response/error", () => {
    useRequestStore.getState().setError("stale error");
    const draft = { ...defaultRequest(), url: "https://loaded.test" };
    useRequestStore.getState().loadTab({
      tabId: "tab-1",
      requestId: "req-1",
      collectionId: "col-1",
      title: "Loaded",
      draft,
    });
    const s = useRequestStore.getState();
    expect(s.activeTabId).toBe("tab-1");
    expect(s.requestId).toBe("req-1");
    expect(s.collectionId).toBe("col-1");
    expect(s.title).toBe("Loaded");
    expect(s.request.url).toBe("https://loaded.test");
    expect(s.error).toBeNull();
  });

  it("setRequestLink records a saved-request home without touching the draft", () => {
    useRequestStore.getState().setUrl("https://keep.test");
    useRequestStore.getState().setRequestLink("req-2", "col-2");
    const s = useRequestStore.getState();
    expect(s.requestId).toBe("req-2");
    expect(s.collectionId).toBe("col-2");
    expect(s.request.url).toBe("https://keep.test");
  });
});
