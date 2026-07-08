//! Root of the collections sidebar panel: a "+" to create a top-level
//! collection, and the recursive tree of collections/folders/requests below
//! it. Replaces the old "Coming in Phase 2." placeholder in `Sidebar`.

import { useRef, useState, type DragEvent, type KeyboardEvent } from "react";
import { ArrowDownUp, ChevronDown, FolderPlus, Loader2, Search, Upload } from "lucide-react";
import { HTTP_METHODS } from "../../lib/methods";
import { useActiveWorkspace } from "../workspaces/hooks";
import { useCollections, useCreateCollection, useMoveCollection, useTags } from "./hooks";
import { childrenOf, type SortMode } from "./tree";
import { clearDrag, resolveDragItem, type DragItem } from "./dragState";
import { CollectionNode } from "./CollectionNode";
import { ImportDialog } from "./ImportDialog";
import { SearchResults } from "./SearchResults";

export function CollectionsPanel() {
  const { data: workspace } = useActiveWorkspace();
  const workspaceId = workspace?.id;
  const { data: collections, isLoading } = useCollections(workspaceId);
  const { data: tags } = useTags(workspaceId);
  const createCollection = useCreateCollection(workspaceId);
  const moveCollection = useMoveCollection(workspaceId);

  const [expandedIds, setExpandedIds] = useState<Set<string>>(new Set());
  const [creatingRoot, setCreatingRoot] = useState(false);
  const [draftName, setDraftName] = useState("");
  const [query, setQuery] = useState("");
  const [methodFilter, setMethodFilter] = useState("any");
  const [tagFilter, setTagFilter] = useState("any");
  const [sortMode, setSortMode] = useState<SortMode>("manual");
  const [importOpen, setImportOpen] = useState(false);
  const dragRef = useRef<DragItem | null>(null);
  // A method/tag filter alone (no typed query) is also "searching" — the
  // filter dropdowns below must work without first typing a search term.
  const searching = query.trim().length > 0 || methodFilter !== "any" || tagFilter !== "any";

  function toggleExpand(id: string) {
    setExpandedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }

  function commitCreateRoot() {
    setCreatingRoot(false);
    const trimmed = draftName.trim();
    if (!trimmed) return;
    createCollection.mutate({ parentId: null, name: trimmed });
  }

  function onDraftKeyDown(e: KeyboardEvent<HTMLInputElement>) {
    if (e.key === "Enter") commitCreateRoot();
    if (e.key === "Escape") setCreatingRoot(false);
  }

  // Dropping on empty space below the tree (i.e. not on any row, which would
  // have already stopped propagation) moves a dragged collection to the top
  // level. Requests can't live at workspace root, so those drops are no-ops.
  function handleDropOnRoot(e: DragEvent) {
    const drag = resolveDragItem(e, dragRef);
    clearDrag(dragRef);
    if (!drag || drag.kind !== "collection" || drag.parentId === null) return;
    moveCollection.mutate({ id: drag.id, newParentId: null });
  }

  const roots = childrenOf(collections ?? [], null, sortMode);

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center justify-between border-b border-slate-100 px-2 py-1.5 dark:border-slate-800">
        <span className="text-xs font-medium text-slate-500 dark:text-slate-400">Collections</span>
        <div className="flex items-center gap-1">
          <div className="relative">
            <select
              value={sortMode}
              onChange={(e) => setSortMode(e.target.value as SortMode)}
              title="Sort collections and requests"
              aria-label="Sort collections and requests"
              className="appearance-none rounded-md border border-slate-200 bg-white py-1 pl-6 pr-5 text-[11px] text-slate-600 focus:outline-none focus:ring-2 focus:ring-accent/40 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-300"
            >
              <option value="manual">Manual</option>
              <option value="name">Name</option>
              <option value="created">Created</option>
              <option value="used">Last used</option>
            </select>
            <ArrowDownUp
              size={12}
              className="pointer-events-none absolute left-1.5 top-1/2 -translate-y-1/2 text-slate-400 dark:text-slate-500"
            />
            <ChevronDown
              size={11}
              className="pointer-events-none absolute right-1 top-1/2 -translate-y-1/2 text-slate-400 dark:text-slate-500"
            />
          </div>
          <button
            type="button"
            title="Import collection"
            onClick={() => setImportOpen(true)}
            className="rounded p-1 text-slate-400 hover:bg-slate-100 hover:text-slate-700 dark:hover:bg-slate-800 dark:hover:text-slate-200"
          >
            <Upload size={14} />
          </button>
          <button
            type="button"
            title="New collection"
            onClick={() => {
              setDraftName("");
              setCreatingRoot(true);
            }}
            className="rounded p-1 text-slate-400 hover:bg-slate-100 hover:text-slate-700 dark:hover:bg-slate-800 dark:hover:text-slate-200"
          >
            <FolderPlus size={14} />
          </button>
        </div>
      </div>

      <div className="flex gap-1.5 border-b border-slate-100 p-1.5 dark:border-slate-800">
        <div className="relative min-w-0 flex-1">
          <Search size={12} className="absolute left-2 top-1/2 -translate-y-1/2 text-slate-400" />
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search requests"
            className="w-full rounded-md border border-slate-200 bg-transparent py-1 pl-6 pr-2 text-xs focus:outline-none focus:ring-2 focus:ring-accent/40 dark:border-slate-700"
          />
        </div>
        <select
          value={methodFilter}
          onChange={(e) => setMethodFilter(e.target.value)}
          className="shrink-0 rounded-md border border-slate-200 bg-transparent px-1.5 py-1 text-xs focus:outline-none focus:ring-2 focus:ring-accent/40 dark:border-slate-700"
        >
          <option value="any">Any method</option>
          {HTTP_METHODS.map((m) => (
            <option key={m} value={m}>
              {m}
            </option>
          ))}
        </select>
        {tags && tags.length > 0 && (
          <select
            value={tagFilter}
            onChange={(e) => setTagFilter(e.target.value)}
            className="shrink-0 rounded-md border border-slate-200 bg-transparent px-1.5 py-1 text-xs focus:outline-none focus:ring-2 focus:ring-accent/40 dark:border-slate-700"
          >
            <option value="any">Any tag</option>
            {tags.map((t) => (
              <option key={t.id} value={t.id}>
                {t.name}
              </option>
            ))}
          </select>
        )}
      </div>

      {searching ? (
        <div className="min-h-0 flex-1 overflow-auto p-1.5">
          <SearchResults
            workspaceId={workspaceId}
            query={query.trim()}
            method={methodFilter === "any" ? null : methodFilter}
            tag={tagFilter === "any" ? null : tagFilter}
          />
        </div>
      ) : (
        <div
          className="min-h-0 flex-1 overflow-auto p-1.5"
          onDragOver={(e) => e.preventDefault()}
          onDrop={handleDropOnRoot}
        >
          {isLoading && (
            <div className="flex items-center justify-center gap-2 p-6 text-sm text-slate-400">
              <Loader2 size={15} className="animate-spin" /> Loading…
            </div>
          )}

          {!isLoading && roots.length === 0 && !creatingRoot && (
            <div className="flex flex-col items-center justify-center gap-1 p-6 text-center text-sm text-slate-400">
              <p>No collections yet.</p>
              <p className="text-xs">Create one to start saving requests.</p>
            </div>
          )}

          {creatingRoot && (
            <div className="px-1.5 py-1">
              <input
                autoFocus
                value={draftName}
                placeholder="Collection name"
                onChange={(e) => setDraftName(e.target.value)}
                onBlur={commitCreateRoot}
                onKeyDown={onDraftKeyDown}
                className="w-full rounded border border-accent/40 bg-white px-1 py-0.5 text-xs focus:outline-none dark:bg-slate-900"
              />
            </div>
          )}

          {roots.map((c) => (
            <CollectionNode
              key={c.id}
              collection={c}
              collections={collections ?? []}
              depth={0}
              workspaceId={workspaceId}
              expandedIds={expandedIds}
              onToggleExpand={toggleExpand}
              dragRef={dragRef}
              sortMode={sortMode}
            />
          ))}
        </div>
      )}

      {importOpen && workspaceId && (
        <ImportDialog workspaceId={workspaceId} parentId={null} onClose={() => setImportOpen(false)} />
      )}
    </div>
  );
}
