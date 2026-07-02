import { describe, expect, it } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { ToolsDialog } from "./ToolsDialog";

const HS256_TOKEN =
  "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyfQ.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c";

describe("ToolsDialog", () => {
  it("renders with Base64 tab active", () => {
    render(<ToolsDialog onClose={() => {}} />);
    expect(screen.getByRole("heading", { name: "Base64" })).toBeInTheDocument();
    expect(screen.getByText("Decode")).toBeInTheDocument();
  });

  it("decodes base64 input", () => {
    render(<ToolsDialog onClose={() => {}} />);
    const input = screen.getByLabelText("Input");
    fireEvent.change(input, { target: { value: "aGVsbG8=" } });
    expect(screen.getByText("hello")).toBeInTheDocument();
  });

  it("switches to JWT tab and decodes token", () => {
    render(<ToolsDialog onClose={() => {}} />);
    fireEvent.click(screen.getByRole("button", { name: "JWT" }));
    const input = screen.getByLabelText("JWT token");
    fireEvent.change(input, { target: { value: HS256_TOKEN } });
    expect(screen.getByText(/John Doe/)).toBeInTheDocument();
    expect(screen.getByText(/HS256/)).toBeInTheDocument();
  });

  it("switches between tool tabs", () => {
    render(<ToolsDialog onClose={() => {}} />);
    fireEvent.click(screen.getByRole("button", { name: "URL" }));
    expect(screen.getByRole("heading", { name: "URL" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Hash" }));
    expect(screen.getByRole("heading", { name: "Hash" })).toBeInTheDocument();
  });
});