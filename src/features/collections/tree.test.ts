import { describe, expect, it } from "vitest";
import type { Collection, SavedRequest } from "../../lib/types";
import { childrenOf, isDescendant, sortRequests } from "./tree";

function makeCollection(overrides: Partial<Collection> = {}): Collection {
  return {
    id: "c1",
    workspaceId: "ws-1",
    parentId: null,
    name: "Untitled",
    description: null,
    sortOrder: 0,
    createdAt: 0,
    updatedAt: 0,
    ...overrides,
  };
}

function makeRequest(overrides: Partial<SavedRequest> = {}): SavedRequest {
  return {
    id: "r1",
    collectionId: "c1",
    name: "Untitled",
    method: "GET",
    url: "",
    headers: [],
    query: [],
    body: { mode: "none" } as SavedRequest["body"],
    options: {} as SavedRequest["options"],
    tags: [],
    sortOrder: 0,
    createdAt: 0,
    updatedAt: 0,
    lastUsedAt: null,
    ...overrides,
  };
}

// root
//  ├─ a
//  │   └─ b
//  │       └─ c
//  └─ d
const TREE: Collection[] = [
  makeCollection({ id: "a", parentId: null, sortOrder: 0 }),
  makeCollection({ id: "b", parentId: "a", sortOrder: 0 }),
  makeCollection({ id: "c", parentId: "b", sortOrder: 0 }),
  makeCollection({ id: "d", parentId: null, sortOrder: 1 }),
];

describe("childrenOf", () => {
  it("returns direct children sorted by sortOrder", () => {
    const unsorted = [
      makeCollection({ id: "second", parentId: "p", sortOrder: 1 }),
      makeCollection({ id: "first", parentId: "p", sortOrder: 0 }),
    ];
    expect(childrenOf(unsorted, "p").map((c) => c.id)).toEqual(["first", "second"]);
  });

  it("returns root-level collections for a null parentId", () => {
    expect(childrenOf(TREE, null).map((c) => c.id)).toEqual(["a", "d"]);
  });

  const mixed = [
    makeCollection({ id: "a", name: "Charlie", sortOrder: 2, createdAt: 10, updatedAt: 10 }),
    makeCollection({ id: "b", name: "Alpha", sortOrder: 0, createdAt: 30, updatedAt: 5 }),
    makeCollection({ id: "c", name: "Bravo", sortOrder: 1, createdAt: 20, updatedAt: 40 }),
  ];

  it("sorts by name when mode is 'name'", () => {
    expect(childrenOf(mixed, null, "name").map((c) => c.id)).toEqual(["b", "c", "a"]);
  });

  it("sorts by createdAt, newest first, when mode is 'created'", () => {
    expect(childrenOf(mixed, null, "created").map((c) => c.id)).toEqual(["b", "c", "a"]);
  });

  it("sorts by updatedAt, newest first, when mode is 'used'", () => {
    expect(childrenOf(mixed, null, "used").map((c) => c.id)).toEqual(["c", "a", "b"]);
  });
});

describe("sortRequests", () => {
  const requests = [
    makeRequest({ id: "a", name: "Charlie", sortOrder: 2, createdAt: 10, lastUsedAt: null }),
    makeRequest({ id: "b", name: "Alpha", sortOrder: 0, createdAt: 30, lastUsedAt: 50 }),
    makeRequest({ id: "c", name: "Bravo", sortOrder: 1, createdAt: 20, lastUsedAt: 100 }),
  ];

  it("defaults to manual sortOrder", () => {
    expect(sortRequests(requests, "manual").map((r) => r.id)).toEqual(["b", "c", "a"]);
  });

  it("sorts by name", () => {
    expect(sortRequests(requests, "name").map((r) => r.id)).toEqual(["b", "c", "a"]);
  });

  it("sorts by createdAt, newest first", () => {
    expect(sortRequests(requests, "created").map((r) => r.id)).toEqual(["b", "c", "a"]);
  });

  it("sorts by lastUsedAt, newest first, with never-used last", () => {
    expect(sortRequests(requests, "used").map((r) => r.id)).toEqual(["c", "b", "a"]);
  });

  it("does not mutate the input array", () => {
    const original = [...requests];
    sortRequests(requests, "name");
    expect(requests).toEqual(original);
  });
});

describe("isDescendant", () => {
  it("is true for a direct child", () => {
    expect(isDescendant("b", "a", TREE)).toBe(true);
  });

  it("is true for a deeply nested descendant", () => {
    expect(isDescendant("c", "a", TREE)).toBe(true);
  });

  it("is true when the two ids are the same node — guards a drop onto self", () => {
    expect(isDescendant("a", "a", TREE)).toBe(true);
  });

  it("is false for an unrelated sibling", () => {
    expect(isDescendant("d", "a", TREE)).toBe(false);
  });

  it("is false for an ancestor checked against its own descendant — direction matters", () => {
    expect(isDescendant("a", "c", TREE)).toBe(false);
  });
});
