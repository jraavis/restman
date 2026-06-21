import { describe, expect, it } from "vitest";
import type { ReactElement } from "react";
import { renderHighlight } from "./highlight";

type MarkElement = ReactElement<{ children: string }>;

describe("renderHighlight", () => {
  it("returns plain text unchanged when there is no match", () => {
    expect(renderHighlight("hello world")).toEqual(["hello world"]);
  });

  it("wraps a single match in a mark element", () => {
    const nodes = renderHighlight("foo \x01bar\x02 baz");
    expect(nodes).toHaveLength(3);
    expect(nodes[0]).toBe("foo ");
    expect((nodes[1] as ReactElement).type).toBe("mark");
    expect((nodes[1] as MarkElement).props.children).toBe("bar");
    expect(nodes[2]).toBe(" baz");
  });

  it("handles adjacent matches with no plain text between them", () => {
    const nodes = renderHighlight("\x01foo\x02\x01bar\x02");
    expect(nodes).toHaveLength(2);
    expect((nodes[0] as MarkElement).props.children).toBe("foo");
    expect((nodes[1] as MarkElement).props.children).toBe("bar");
  });

  it("handles a match spanning the entire string", () => {
    const nodes = renderHighlight("\x01wholestring\x02");
    expect(nodes).toHaveLength(1);
    expect((nodes[0] as ReactElement).type).toBe("mark");
    expect((nodes[0] as MarkElement).props.children).toBe("wholestring");
  });

  it("supports multiple matches separated by plain text", () => {
    const nodes = renderHighlight("a \x01b\x02 c \x01d\x02 e");
    expect(nodes).toEqual([
      "a ",
      expect.objectContaining({ type: "mark" }),
      " c ",
      expect.objectContaining({ type: "mark" }),
      " e",
    ]);
  });
});
