//! Tests for the CookieJarDialog component.

import type { ReactElement, ReactNode } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ipc } from "../../lib/ipc";
import type { CookieEntry } from "../../lib/types";
import { CookieJarDialog } from "./CookieJarDialog";

vi.mock("../../lib/ipc", () => ({
  ipc: { listCookies: vi.fn(), deleteCookie: vi.fn(), clearCookies: vi.fn() },
}));

beforeEach(() => {
  vi.mocked(ipc.listCookies).mockReset();
  vi.mocked(ipc.deleteCookie).mockReset();
  vi.mocked(ipc.clearCookies).mockReset();
});

function makeCookie(overrides: Partial<CookieEntry> = {}): CookieEntry {
  return {
    name: "session_id",
    value: "abc123",
    domain: "example.com",
    path: "/",
    secure: true,
    httpOnly: true,
    sameSite: "Lax",
    expiresAt: null,
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

describe("CookieJarDialog", () => {
  it("loads and displays cookies with their flags", async () => {
    vi.mocked(ipc.listCookies).mockResolvedValue([makeCookie()]);
    renderWithClient(<CookieJarDialog onClose={() => {}} />);

    expect(await screen.findByText("session_id")).toBeInTheDocument();
    expect(screen.getByText("abc123")).toBeInTheDocument();
    expect(screen.getByText("Secure")).toBeInTheDocument();
    expect(screen.getByText("HttpOnly")).toBeInTheDocument();
    expect(screen.getByText("Lax")).toBeInTheDocument();
    expect(screen.getByText("Session")).toBeInTheDocument();
  });

  it("shows a formatted expiry for persistent cookies", async () => {
    const expiresAt = Math.floor(Date.UTC(2030, 0, 1, 0, 0, 0) / 1000);
    vi.mocked(ipc.listCookies).mockResolvedValue([makeCookie({ expiresAt })]);
    renderWithClient(<CookieJarDialog onClose={() => {}} />);

    expect(await screen.findByText(`Expires ${new Date(expiresAt * 1000).toLocaleString()}`)).toBeInTheDocument();
  });

  it("shows an empty state when there are no cookies", async () => {
    vi.mocked(ipc.listCookies).mockResolvedValue([]);
    renderWithClient(<CookieJarDialog onClose={() => {}} />);

    expect(await screen.findByText("No cookies stored.")).toBeInTheDocument();
  });

  it("deletes a single cookie by its domain/path/name triple", async () => {
    vi.mocked(ipc.listCookies).mockResolvedValue([makeCookie()]);
    vi.mocked(ipc.deleteCookie).mockResolvedValue(undefined);
    renderWithClient(<CookieJarDialog onClose={() => {}} />);

    await screen.findByText("session_id");
    fireEvent.click(screen.getByTitle("Delete cookie"));

    await waitFor(() =>
      expect(ipc.deleteCookie).toHaveBeenCalledWith("example.com", "/", "session_id"),
    );
  });

  it("clears all cookies after confirmation", async () => {
    vi.spyOn(window, "confirm").mockReturnValue(true);
    vi.mocked(ipc.listCookies).mockResolvedValue([makeCookie()]);
    vi.mocked(ipc.clearCookies).mockResolvedValue(undefined);
    renderWithClient(<CookieJarDialog onClose={() => {}} />);

    await screen.findByText("session_id");
    fireEvent.click(screen.getByRole("button", { name: /clear all/i }));

    await waitFor(() => expect(ipc.clearCookies).toHaveBeenCalled());
  });

  it("does not clear cookies when the confirmation is dismissed", async () => {
    vi.spyOn(window, "confirm").mockReturnValue(false);
    vi.mocked(ipc.listCookies).mockResolvedValue([makeCookie()]);
    renderWithClient(<CookieJarDialog onClose={() => {}} />);

    await screen.findByText("session_id");
    fireEvent.click(screen.getByRole("button", { name: /clear all/i }));

    expect(ipc.clearCookies).not.toHaveBeenCalled();
  });
});
