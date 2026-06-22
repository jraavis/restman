import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { defaultRequest } from "../../lib/http";
import { defaultRequestAuth, type SavedRequest, type SearchHit, type Tag } from "../../lib/types";
import { SearchResults } from "./SearchResults";
import { useSearchRequests } from "./hooks";

vi.mock("./hooks", () => ({
  useSearchRequests: vi.fn(),
}));

vi.mock("./useOpenRequest", () => ({
  useOpenRequest: () => ({ open: vi.fn() }),
}));

function makeTag(overrides: Partial<Tag> = {}): Tag {
  return { id: "tag-1", workspaceId: "ws-1", name: "starred", color: "#000", ...overrides };
}

function makeHit(id: string, tags: Tag[]): SearchHit {
  const request: SavedRequest = {
    id,
    collectionId: "col-1",
    name: `Request ${id}`,
    method: "GET",
    url: `https://example.com/${id}`,
    headers: [],
    query: [],
    body: { mode: "none" },
    options: defaultRequest().options,
    auth: defaultRequestAuth(),
    tags,
    sortOrder: 0,
    createdAt: 0,
    updatedAt: 0,
    lastUsedAt: null,
  };
  return { request, nameHighlight: request.name, urlHighlight: request.url };
}

function mockHits(hits: SearchHit[]) {
  vi.mocked(useSearchRequests).mockReturnValue({ data: hits, isLoading: false } as ReturnType<
    typeof useSearchRequests
  >);
}

describe("SearchResults", () => {
  it("narrows to hits carrying the selected tag, client-side, with an empty query", () => {
    const tagged = makeHit("r1", [makeTag()]);
    const untagged = makeHit("r2", []);
    mockHits([tagged, untagged]);

    render(<SearchResults workspaceId="ws-1" query="" method={null} tag="tag-1" />);

    expect(screen.getByText("Request r1")).toBeInTheDocument();
    expect(screen.queryByText("Request r2")).not.toBeInTheDocument();
  });

  it("shows every hit when no tag is selected", () => {
    mockHits([makeHit("r1", [makeTag()]), makeHit("r2", [])]);

    render(<SearchResults workspaceId="ws-1" query="" method={null} tag={null} />);

    expect(screen.getByText("Request r1")).toBeInTheDocument();
    expect(screen.getByText("Request r2")).toBeInTheDocument();
  });

  it("shows the empty state when a tag matches nothing", () => {
    mockHits([makeHit("r1", [])]);

    render(<SearchResults workspaceId="ws-1" query="" method={null} tag="tag-1" />);

    expect(screen.getByText("No matching requests.")).toBeInTheDocument();
  });
});
