//! Tests for the SsePanel component.

import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ipc } from "../../lib/ipc";
import type { SseEvent } from "../../lib/types";
import { SsePanel } from "./SsePanel";

vi.mock("../../lib/ipc", () => ({
  ipc: { sseConnect: vi.fn(), sseDisconnect: vi.fn() },
}));

beforeEach(() => {
  vi.mocked(ipc.sseConnect).mockReset();
  vi.mocked(ipc.sseDisconnect).mockReset();
});

function typeUrl(url: string) {
  fireEvent.change(screen.getByPlaceholderText("https://api.example.com/events"), {
    target: { value: url },
  });
}

describe("SsePanel", () => {
  it("disables Connect until a valid URL is entered", () => {
    render(<SsePanel workspaceId="ws1" onClose={() => {}} />);
    expect(screen.getByRole("button", { name: "Connect" })).toBeDisabled();

    typeUrl("not a url");
    expect(screen.getByRole("button", { name: "Connect" })).toBeDisabled();

    typeUrl("https://api.example.com/events");
    expect(screen.getByRole("button", { name: "Connect" })).toBeEnabled();
  });

  it("connects with the typed URL and workspace id", async () => {
    vi.mocked(ipc.sseConnect).mockResolvedValue("conn-1");
    render(<SsePanel workspaceId="ws1" onClose={() => {}} />);

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
    render(<SsePanel workspaceId="ws1" onClose={() => {}} />);

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
    render(<SsePanel workspaceId="ws1" onClose={() => {}} />);

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
    vi.mocked(ipc.sseDisconnect).mockResolvedValue(undefined);
    render(<SsePanel workspaceId="ws1" onClose={() => {}} />);

    typeUrl("https://api.example.com/events");
    fireEvent.click(screen.getByRole("button", { name: "Connect" }));
    await screen.findByText("Open");

    fireEvent.click(screen.getByRole("button", { name: "Disconnect" }));

    expect(ipc.sseDisconnect).toHaveBeenCalledWith("conn-1");
    expect(await screen.findByText("Closed")).toBeInTheDocument();
  });
});
