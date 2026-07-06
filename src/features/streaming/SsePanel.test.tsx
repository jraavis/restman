//! Tests for the SsePanel component.

import type { ReactElement, ReactNode } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ipc } from "../../lib/ipc";
import type { SavedRequest, SseEvent } from "../../lib/types";
import { SsePanel } from "./SsePanel";

vi.mock("../../lib/ipc", () => ({
  ipc: {
    sseConnect: vi.fn(),
    streamDisconnect: vi.fn(),
    listCollections: vi.fn().mockResolvedValue([]),
    createRequest: vi.fn(),
    updateRequest: vi.fn(),
  },
}));

beforeEach(() => {
  vi.mocked(ipc.sseConnect).mockReset();
  vi.mocked(ipc.streamDisconnect).mockReset();
  vi.mocked(ipc.listCollections).mockReset().mockResolvedValue([]);
});

function typeUrl(url: string) {
  fireEvent.change(screen.getByPlaceholderText("https://api.example.com/events"), {
    target: { value: url },
  });
}

function makeSavedSseRequest(overrides: Partial<SavedRequest> = {}): SavedRequest {
  return {
    id: "req-1",
    collectionId: "col-1",
    name: "Saved SSE",
    method: "SSE",
    url: "",
    headers: [],
    query: [],
    body: { mode: "none" },
    options: { timeoutSecs: 30, followRedirects: true, verifySsl: true, maxRedirects: 10, sendCookies: false },
    auth: { mode: "inherit" },
    preRequestScript: "",
    postResponseScript: "",
    kind: "sse",
    streamConfig: { url: "https://saved.example.com/events", headers: [{ name: "X-Token", value: "abc", enabled: true }] },
    tags: [],
    sortOrder: 0,
    createdAt: 0,
    updatedAt: 0,
    lastUsedAt: null,
    ...overrides,
  };
}

function renderWithClient(ui: ReactElement) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={qc}>{children}</QueryClientProvider>
  );
  return render(ui, { wrapper });
}

describe("SsePanel", () => {
  it("disables Connect until a valid URL is entered", () => {
    renderWithClient(<SsePanel workspaceId="ws1" onClose={() => {}} />);
    expect(screen.getByRole("button", { name: "Connect" })).toBeDisabled();

    typeUrl("not a url");
    expect(screen.getByRole("button", { name: "Connect" })).toBeDisabled();

    typeUrl("https://api.example.com/events");
    expect(screen.getByRole("button", { name: "Connect" })).toBeEnabled();
  });

  it("connects with the typed URL and workspace id", async () => {
    vi.mocked(ipc.sseConnect).mockResolvedValue("conn-1");
    renderWithClient(<SsePanel workspaceId="ws1" onClose={() => {}} />);

    typeUrl("https://api.example.com/events");
    fireEvent.click(screen.getByRole("button", { name: "Connect" }));

    await waitFor(() =>
      expect(ipc.sseConnect).toHaveBeenCalledWith(
        "ws1",
        "https://api.example.com/events",
        [],
        expect.any(Function),
      ),
    );
  });

  it("renders dispatched frames and flips status to Open", async () => {
    vi.mocked(ipc.sseConnect).mockImplementation(async (_ws, _url, _headers, onEvent) => {
      onEvent({ type: "open" });
      onEvent({ type: "message", event: "ping", data: "hello world", id: "1" });
      return "conn-1";
    });
    renderWithClient(<SsePanel workspaceId="ws1" onClose={() => {}} />);

    typeUrl("https://api.example.com/events");
    fireEvent.click(screen.getByRole("button", { name: "Connect" }));

    expect(await screen.findByText("Open")).toBeInTheDocument();
    expect(screen.getByText("Connection opened")).toBeInTheDocument();
    expect(screen.getByText("hello world")).toBeInTheDocument();
    expect(screen.getByText("ping")).toBeInTheDocument();
  });

  it("shows the error message and Error status when an error event arrives", async () => {
    vi.mocked(ipc.sseConnect).mockImplementation(async (_ws, _url, _headers, onEvent) => {
      onEvent({ type: "error", message: "server returned 404 Not Found" } satisfies SseEvent);
      return "conn-1";
    });
    renderWithClient(<SsePanel workspaceId="ws1" onClose={() => {}} />);

    typeUrl("https://api.example.com/events");
    fireEvent.click(screen.getByRole("button", { name: "Connect" }));

    expect(await screen.findByText("Error")).toBeInTheDocument();
    expect(screen.getByText("server returned 404 Not Found")).toBeInTheDocument();
  });

  it("disconnects the active connection and shows Closed", async () => {
    vi.mocked(ipc.sseConnect).mockImplementation(async (_ws, _url, _headers, onEvent) => {
      onEvent({ type: "open" });
      return "conn-1";
    });
    vi.mocked(ipc.streamDisconnect).mockResolvedValue(undefined);
    renderWithClient(<SsePanel workspaceId="ws1" onClose={() => {}} />);

    typeUrl("https://api.example.com/events");
    fireEvent.click(screen.getByRole("button", { name: "Connect" }));
    await screen.findByText("Open");

    fireEvent.click(screen.getByRole("button", { name: "Disconnect" }));

    expect(ipc.streamDisconnect).toHaveBeenCalledWith("conn-1");
    expect(await screen.findByText("Closed")).toBeInTheDocument();
  });

  it("prefills url and headers from a saved request's streamConfig on reopen", () => {
    const saved = makeSavedSseRequest();
    renderWithClient(<SsePanel workspaceId="ws1" savedRequest={saved} onClose={() => {}} />);

    expect(screen.getByPlaceholderText("https://api.example.com/events")).toHaveValue(
      "https://saved.example.com/events",
    );
    expect(screen.getByRole("heading", { name: /Saved SSE/ })).toBeInTheDocument();

    fireEvent.click(screen.getByText(/Headers/));
    expect(screen.getByDisplayValue("X-Token")).toBeInTheDocument();
    expect(screen.getByDisplayValue("abc")).toBeInTheDocument();
  });

  it("updates the linked saved request in place instead of opening the save dialog", async () => {
    vi.mocked(ipc.updateRequest).mockResolvedValue(makeSavedSseRequest());
    const saved = makeSavedSseRequest();
    renderWithClient(<SsePanel workspaceId="ws1" savedRequest={saved} onClose={() => {}} />);

    typeUrl("https://changed.example.com/events");
    fireEvent.click(screen.getByRole("button", { name: /Save/ }));

    await waitFor(() =>
      expect(ipc.updateRequest).toHaveBeenCalledWith(
        "req-1",
        expect.objectContaining({
          kind: "sse",
          streamConfig: {
            url: "https://changed.example.com/events",
            headers: [{ name: "X-Token", value: "abc", enabled: true }],
          },
        }),
      ),
    );
    expect(screen.queryByText("Save request")).toBeNull();
  });
});
