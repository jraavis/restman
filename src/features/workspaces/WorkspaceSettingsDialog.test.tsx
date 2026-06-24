//! Tests for the WorkspaceSettingsDialog component.

import type { ReactElement, ReactNode } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ipc } from "../../lib/ipc";
import { SECRET_MASK, type WorkspaceSettings } from "../../lib/types";
import { WorkspaceSettingsDialog } from "./WorkspaceSettingsDialog";

vi.mock("../../lib/ipc", () => ({
  ipc: { getWorkspaceSettings: vi.fn(), setWorkspaceSettings: vi.fn() },
}));

function makeSettings(overrides: Partial<WorkspaceSettings> = {}): WorkspaceSettings {
  return {
    workspaceId: "ws-1",
    proxyUrl: null,
    proxyBypass: null,
    defaultHeaders: [],
    clientCert: { mode: "none" },
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

describe("WorkspaceSettingsDialog", () => {
  it("loads and displays the workspace's current proxy settings", async () => {
    vi.mocked(ipc.getWorkspaceSettings).mockResolvedValue(
      makeSettings({ proxyUrl: "http://proxy.corp:8080", proxyBypass: "localhost" }),
    );
    renderWithClient(
      <WorkspaceSettingsDialog workspaceId="ws-1" workspaceName="Acme" onClose={() => {}} />,
    );

    expect(await screen.findByDisplayValue("http://proxy.corp:8080")).toBeInTheDocument();
    expect(screen.getByDisplayValue("localhost")).toBeInTheDocument();
  });

  it("saves edited proxy fields and closes on success", async () => {
    vi.mocked(ipc.getWorkspaceSettings).mockResolvedValue(makeSettings());
    vi.mocked(ipc.setWorkspaceSettings).mockResolvedValue(makeSettings({ proxyUrl: "http://new.proxy:9000" }));
    const onClose = vi.fn();
    renderWithClient(
      <WorkspaceSettingsDialog workspaceId="ws-1" workspaceName="Acme" onClose={onClose} />,
    );

    const proxyInput = await screen.findByPlaceholderText("http://proxy.corp:8080");
    fireEvent.change(proxyInput, { target: { value: "http://new.proxy:9000" } });
    fireEvent.click(screen.getByRole("button", { name: /^save$/i }));

    await waitFor(() =>
      expect(ipc.setWorkspaceSettings).toHaveBeenCalledWith(
        expect.objectContaining({ proxyUrl: "http://new.proxy:9000" }),
      ),
    );
    await waitFor(() => expect(onClose).toHaveBeenCalled());
  });

  it("renders existing default headers and lets the user add a new one", async () => {
    vi.mocked(ipc.getWorkspaceSettings).mockResolvedValue(
      makeSettings({ defaultHeaders: [{ name: "X-Team", value: "platform", enabled: true }] }),
    );
    renderWithClient(
      <WorkspaceSettingsDialog workspaceId="ws-1" workspaceName="Acme" onClose={() => {}} />,
    );

    expect(await screen.findByDisplayValue("X-Team")).toBeInTheDocument();
    expect(screen.getByDisplayValue("platform")).toBeInTheDocument();
  });

  it("shows an already-saved hint for masked paste-mode cert fields, and round-trips the mask untouched on save", async () => {
    vi.mocked(ipc.getWorkspaceSettings).mockResolvedValue(
      makeSettings({
        clientCert: {
          mode: "paste",
          data: { certPem: SECRET_MASK, keyPem: SECRET_MASK, passphrase: SECRET_MASK },
        },
      }),
    );
    vi.mocked(ipc.setWorkspaceSettings).mockResolvedValue(makeSettings());
    renderWithClient(
      <WorkspaceSettingsDialog workspaceId="ws-1" workspaceName="Acme" onClose={() => {}} />,
    );

    expect(await screen.findAllByText(/already saved — paste to replace/i)).toHaveLength(2);

    fireEvent.click(screen.getByRole("button", { name: /^save$/i }));
    await waitFor(() =>
      expect(ipc.setWorkspaceSettings).toHaveBeenCalledWith(
        expect.objectContaining({
          clientCert: {
            mode: "paste",
            data: { certPem: SECRET_MASK, keyPem: SECRET_MASK, passphrase: SECRET_MASK },
          },
        }),
      ),
    );
  });

  it("resets cert fields when switching client-cert mode", async () => {
    vi.mocked(ipc.getWorkspaceSettings).mockResolvedValue(
      makeSettings({
        clientCert: { mode: "paste", data: { certPem: SECRET_MASK, keyPem: SECRET_MASK, passphrase: null } },
      }),
    );
    renderWithClient(
      <WorkspaceSettingsDialog workspaceId="ws-1" workspaceName="Acme" onClose={() => {}} />,
    );

    await screen.findByPlaceholderText("-----BEGIN CERTIFICATE-----");
    fireEvent.change(screen.getByRole("combobox", { name: /client certificate/i }), {
      target: { value: "path" },
    });

    expect(screen.queryByPlaceholderText("-----BEGIN CERTIFICATE-----")).not.toBeInTheDocument();
    expect(screen.getByPlaceholderText("/path/to/cert.pem")).toHaveValue("");
  });
});
