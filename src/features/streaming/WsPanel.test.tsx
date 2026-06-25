//! Tests for the WsPanel component.

import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ipc } from "../../lib/ipc";
import type { WsEvent } from "../../lib/types";
import { WsPanel } from "./WsPanel";

vi.mock("../../lib/ipc", () => ({
  ipc: { wsConnect: vi.fn(), wsSend: vi.fn(), streamDisconnect: vi.fn() },
}));

beforeEach(() => {
  vi.mocked(ipc.wsConnect).mockReset();
  vi.mocked(ipc.wsSend).mockReset();
  vi.mocked(ipc.streamDisconnect).mockReset();
});

function typeUrl(url: string) {
  fireEvent.change(screen.getByPlaceholderText("wss://echo.websocket.org"), {
    target: { value: url },
  });
}

describe("WsPanel", () => {
  it("enables Connect only for a valid ws(s):// URL", () => {
    render(<WsPanel workspaceId="ws1" onClose={() => {}} />);
    expect(screen.getByRole("button", { name: "Connect" })).toBeDisabled();

    typeUrl("not a url");
    expect(screen.getByRole("button", { name: "Connect" })).toBeDisabled();

    // A valid http URL is still rejected — this panel requires the ws scheme.
    typeUrl("https://example.com/socket");
    expect(screen.getByRole("button", { name: "Connect" })).toBeDisabled();

    typeUrl("wss://echo.websocket.org");
    expect(screen.getByRole("button", { name: "Connect" })).toBeEnabled();
  });

  it("connects with the typed URL and workspace id", async () => {
    vi.mocked(ipc.wsConnect).mockResolvedValue("conn-1");
    render(<WsPanel workspaceId="ws1" onClose={() => {}} />);

    typeUrl("wss://echo.websocket.org");
    fireEvent.click(screen.getByRole("button", { name: "Connect" }));

    await waitFor(() =>
      expect(ipc.wsConnect).toHaveBeenCalledWith(
        "ws1",
        "wss://echo.websocket.org",
        [],
        expect.any(Function),
      ),
    );
  });

  it("keeps the composer disabled until the socket is open", async () => {
    vi.mocked(ipc.wsConnect).mockImplementation(async (_ws, _url, _headers, onEvent) => {
      onEvent({ type: "open" });
      return "conn-1";
    });
    render(<WsPanel workspaceId="ws1" onClose={() => {}} />);

    // Before connecting, Send is disabled even with text in the box.
    expect(screen.getByRole("button", { name: "Send" })).toBeDisabled();

    typeUrl("wss://echo.websocket.org");
    fireEvent.click(screen.getByRole("button", { name: "Connect" }));
    await screen.findByText("Open");

    fireEvent.change(screen.getByPlaceholderText(/Message/), { target: { value: "hi" } });
    expect(screen.getByRole("button", { name: "Send" })).toBeEnabled();
  });

  it("sends a text frame and echoes it into the transcript", async () => {
    vi.mocked(ipc.wsConnect).mockImplementation(async (_ws, _url, _headers, onEvent) => {
      onEvent({ type: "open" });
      return "conn-1";
    });
    vi.mocked(ipc.wsSend).mockResolvedValue(undefined);
    render(<WsPanel workspaceId="ws1" onClose={() => {}} />);

    typeUrl("wss://echo.websocket.org");
    fireEvent.click(screen.getByRole("button", { name: "Connect" }));
    await screen.findByText("Open");

    fireEvent.change(screen.getByPlaceholderText(/Message/), { target: { value: "ping" } });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));

    await waitFor(() =>
      expect(ipc.wsSend).toHaveBeenCalledWith("conn-1", { binary: false, data: "ping" }),
    );
    expect(await screen.findByText("ping")).toBeInTheDocument();
  });

  it("renders a received frame and a close with code/reason", async () => {
    vi.mocked(ipc.wsConnect).mockImplementation(async (_ws, _url, _headers, onEvent) => {
      onEvent({ type: "open" });
      onEvent({ type: "message", binary: false, data: "echo back" } satisfies WsEvent);
      onEvent({ type: "closed", code: 1000, reason: "bye" } satisfies WsEvent);
      return "conn-1";
    });
    render(<WsPanel workspaceId="ws1" onClose={() => {}} />);

    typeUrl("wss://echo.websocket.org");
    fireEvent.click(screen.getByRole("button", { name: "Connect" }));

    expect(await screen.findByText("echo back")).toBeInTheDocument();
    expect(screen.getByText("Connection closed (1000: bye)")).toBeInTheDocument();
  });
});
