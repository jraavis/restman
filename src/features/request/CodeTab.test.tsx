//! Tests for the CodeTab component.

import type { ReactElement, ReactNode } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { defaultRequest } from "../../lib/http";
import { ipc } from "../../lib/ipc";
import { CodeTab } from "./CodeTab";

vi.mock("../../lib/ipc", () => ({
  ipc: { generateCode: vi.fn() },
}));

// Monaco editors are heavy; mock LazyCodeEditor so this test doesn't need a
// real browser renderer.
vi.mock("../../components/LazyCodeEditor", () => ({
  LazyCodeEditor: ({ value }: { value: string }) => <textarea data-testid="monaco" defaultValue={value} />,
}));

function renderWithClient(ui: ReactElement) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={qc}>{children}</QueryClientProvider>
  );
  return render(ui, { wrapper });
}

describe("CodeTab", () => {
  it("prompts for a URL instead of calling generateCode when the request has none", () => {
    renderWithClient(
      <CodeTab request={defaultRequest()} workspaceId="ws-1" collectionId={null} requestId={null} />,
    );
    expect(screen.getByText(/Enter a URL/i)).toBeTruthy();
    expect(ipc.generateCode).not.toHaveBeenCalled();
  });

  it("requests code for the selected language and renders the result", async () => {
    vi.mocked(ipc.generateCode).mockResolvedValue("curl -X GET 'https://a.test'");
    renderWithClient(
      <CodeTab
        request={{ ...defaultRequest(), url: "https://a.test" }}
        workspaceId="ws-1"
        collectionId="col-1"
        requestId="req-1"
      />,
    );
    await waitFor(() => expect(ipc.generateCode).toHaveBeenCalledWith(
      { ...defaultRequest(), url: "https://a.test" },
      "ws-1",
      "col-1",
      "req-1",
      "curl",
      { includeAuth: true, includeHeaders: true },
    ));
    expect(await screen.findByTestId("monaco")).toHaveValue("curl -X GET 'https://a.test'");
  });

  it("shows the auth-staleness hint only while auth is included", () => {
    vi.mocked(ipc.generateCode).mockResolvedValue("");
    renderWithClient(
      <CodeTab
        request={{ ...defaultRequest(), url: "https://a.test" }}
        workspaceId="ws-1"
        collectionId={null}
        requestId={null}
      />,
    );
    expect(screen.getByText(/reflects the saved request/i)).toBeTruthy();
  });
});
