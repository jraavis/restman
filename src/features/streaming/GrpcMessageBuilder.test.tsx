//! Tests for the GrpcMessageBuilder component.

import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { GrpcMessageBuilder } from "./GrpcMessageBuilder";
import type { GrpcMethodDescriptor } from "./grpcSchemaTypes";

function makeMethod(inputFields: GrpcMethodDescriptor["inputFields"]): GrpcMethodDescriptor {
  return {
    serviceName: "example.Greeter",
    methodName: "SayHello",
    fullName: "example.Greeter/SayHello",
    streamingType: "unary",
    inputFields,
    outputFields: [{ name: "message", type: "string", repeated: false }],
  };
}

describe("GrpcMessageBuilder", () => {
  it("renders one input per inputField with the correct input type", () => {
    const method = makeMethod([
      { name: "name", type: "string", repeated: false },
      { name: "count", type: "int32", repeated: false },
      { name: "enabled", type: "bool", repeated: false },
    ]);
    render(<GrpcMessageBuilder method={method} onSend={() => {}} />);

    const nameEl = screen.getByTestId("grpc-field-name") as HTMLInputElement;
    const countEl = screen.getByTestId("grpc-field-count") as HTMLInputElement;
    const enabledEl = screen.getByTestId("grpc-field-enabled") as HTMLInputElement;

    expect(nameEl.tagName).toBe("INPUT");
    expect(nameEl.type).toBe("text");
    expect(countEl.tagName).toBe("INPUT");
    expect(countEl.type).toBe("number");
    expect(enabledEl.tagName).toBe("INPUT");
    expect(enabledEl.type).toBe("checkbox");
  });

  it("emits the typed string value under the right key on Invoke", () => {
    const method = makeMethod([{ name: "name", type: "string", repeated: false }]);
    const onSend = vi.fn();
    render(<GrpcMessageBuilder method={method} onSend={onSend} />);

    fireEvent.change(screen.getByTestId("grpc-field-name"), {
      target: { value: "Ada" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Invoke" }));

    expect(onSend).toHaveBeenCalledTimes(1);
    const payload = JSON.parse(onSend.mock.calls[0][0]);
    expect(payload).toEqual({ name: "Ada" });
  });

  it("packs repeated textarea lines into a JSON array on Invoke", () => {
    const method = makeMethod([{ name: "tags", type: "string", repeated: true }]);
    const onSend = vi.fn();
    render(<GrpcMessageBuilder method={method} onSend={onSend} />);

    const ta = screen.getByTestId("grpc-field-tags") as HTMLTextAreaElement;
    fireEvent.change(ta, { target: { value: "alpha\nbeta\n\n" } });
    fireEvent.click(screen.getByRole("button", { name: "Invoke" }));

    const payload = JSON.parse(onSend.mock.calls[0][0]);
    expect(payload).toEqual({ tags: ["alpha", "beta"] });
  });

  it("pre-fills the JSON editor with current form values and reconciles back", () => {
    const method = makeMethod([{ name: "name", type: "string", repeated: false }]);
    const onSend = vi.fn();
    render(<GrpcMessageBuilder method={method} onSend={onSend} />);

    fireEvent.change(screen.getByTestId("grpc-field-name"), {
      target: { value: "Grace" },
    });

    // Switch to JSON mode — textarea should be pre-filled with current form.
    fireEvent.click(screen.getByRole("button", { name: "JSON" }));
    const jsonTa = screen.getByTestId("grpc-json-editor") as HTMLTextAreaElement;
    expect(JSON.parse(jsonTa.value)).toEqual({ name: "Grace" });

    // Edit the JSON and flip back to Form — form should reconcile.
    fireEvent.change(jsonTa, {
      target: { value: JSON.stringify({ name: "Ada" }, null, 2) },
    });
    fireEvent.click(screen.getByRole("button", { name: "Form" }));

    const nameEl = screen.getByTestId("grpc-field-name") as HTMLInputElement;
    expect(nameEl.value).toBe("Ada");
  });

  it("shows an error hint on invalid JSON and keeps the form's prior values", () => {
    const method = makeMethod([{ name: "name", type: "string", repeated: false }]);
    const onSend = vi.fn();
    render(<GrpcMessageBuilder method={method} onSend={onSend} />);

    fireEvent.change(screen.getByTestId("grpc-field-name"), {
      target: { value: "Grace" },
    });

    fireEvent.click(screen.getByRole("button", { name: "JSON" }));
    const jsonTa = screen.getByTestId("grpc-json-editor") as HTMLTextAreaElement;
    fireEvent.change(jsonTa, { target: { value: "{ this is not json" } });

    // Flip back to Form — broken JSON should NOT crash or wipe the form.
    fireEvent.click(screen.getByRole("button", { name: "Form" }));

    expect(screen.getByText(/Invalid JSON/)).toBeInTheDocument();
    const nameEl = screen.getByTestId("grpc-field-name") as HTMLInputElement;
    expect(nameEl.value).toBe("Grace");

    // Invoke still emits the prior (valid) form state.
    fireEvent.click(screen.getByRole("button", { name: "Invoke" }));
    const payload = JSON.parse(onSend.mock.calls[0][0]);
    expect(payload).toEqual({ name: "Grace" });
  });

  it("disables the Invoke button when sendDisabled is true", () => {
    const method = makeMethod([{ name: "name", type: "string", repeated: false }]);
    render(<GrpcMessageBuilder method={method} onSend={() => {}} sendDisabled />);
    expect(screen.getByRole("button", { name: "Invoke" })).toBeDisabled();
  });

  it("renders the empty state when inputFields is empty", () => {
    const method = makeMethod([]);
    render(<GrpcMessageBuilder method={method} onSend={() => {}} />);
    expect(
      screen.getByText("This method takes no request fields."),
    ).toBeInTheDocument();
  });

  it("honors a custom send label", () => {
    const method = makeMethod([{ name: "name", type: "string", repeated: false }]);
    render(
      <GrpcMessageBuilder method={method} onSend={() => {}} sendLabel="Send Request" />,
    );
    expect(screen.getByRole("button", { name: "Send Request" })).toBeInTheDocument();
  });
});