//! One collection in the tree: header (expand toggle, name, actions) plus,
//! once expanded, its sub-collections (recursively) and its requests. The
//! request list is a separate component mounted only while expanded — see
//! `RequestList` for why that's what makes the fetch lazy.

import { useState, type DragEvent, type KeyboardEvent } from "react";
import { confirmDelete } from "../../lib/confirmDelete";
import { save } from "@tauri-apps/plugin-dialog";
import {
  ChevronDown,
  ChevronRight,
  Copy,
  Download,
  FilePlus,
  FolderPlus,
  Lock,
  MoreHorizontal,
  Pencil,
  Play,
  Trash2,
  Upload,
} from "lucide-react";
import { useDismissable } from "../../lib/useDismissable";
import { textToBase64 } from "../../lib/encoding";
import { defaultRequest } from "../../lib/http";
import { ipc } from "../../lib/ipc";
import { defaultRequestAuth, type Collection, type ExportFormat } from "../../lib/types";
import { usePlugins } from "../plugins/hooks";
import { CollectionAuthDialog } from "./CollectionAuthDialog";
import { CollectionRunner } from "./CollectionRunner";
import { ImportDialog } from "./ImportDialog";
import {
  useCreateCollection,
  useCreateRequest,
  useDeleteCollection,
  useDuplicateCollection,
  useMoveCollection,
  useMoveRequest,
  useReorderCollections,
  useUpdateCollection,
} from "./hooks";
import { useOpenRequest } from "./useOpenRequest";
import { childrenOf, isDescendant, type SortMode } from "./tree";
import type { DragRef } from "./dragState";
import { RequestList } from "./RequestList";

export function CollectionNode({
  collection,
  collections,
  depth,
  workspaceId,
  expandedIds,
  onToggleExpand,
  dragRef,
  sortMode,
}: {
  collection: Collection;
  collections: Collection[];
  depth: number;
  workspaceId: string | undefined;
  expandedIds: Set<string>;
  onToggleExpand: (id: string) => void;
  dragRef: DragRef;
  sortMode: SortMode;
}) {
  const [editing, setEditing] = useState(false);
  const [name, setName] = useState(collection.name);
  const [menuOpen, setMenuOpen] = useState(false);
  const [creating, setCreating] = useState<"folder" | "request" | null>(null);
  const [draftName, setDraftName] = useState("");
  const [authDialogOpen, setAuthDialogOpen] = useState(false);
  const [runnerOpen, setRunnerOpen] = useState(false);
  const [importOpen, setImportOpen] = useState(false);
  const menuRef = useDismissable<HTMLDivElement>(() => setMenuOpen(false));

  const createCollection = useCreateCollection(workspaceId);
  const updateCollection = useUpdateCollection(workspaceId);
  const deleteCollection = useDeleteCollection(workspaceId);
  const duplicateCollection = useDuplicateCollection(workspaceId);
  const moveCollection = useMoveCollection(workspaceId);
  const reorderCollections = useReorderCollections(workspaceId);
  const createRequest = useCreateRequest(workspaceId);
  const moveRequest = useMoveRequest();
  const { open } = useOpenRequest(workspaceId);
  const { data: allExportPlugins } = usePlugins(workspaceId, "export");
  const exportPlugins = allExportPlugins?.filter((p) => p.enabled);

  const expanded = expandedIds.has(collection.id);
  const children = childrenOf(collections, collection.id, sortMode);
  const indent = { paddingLeft: 6 + depth * 14 };

  function commitRename() {
    setEditing(false);
    const trimmed = name.trim();
    if (trimmed && trimmed !== collection.name) {
      updateCollection.mutate({ id: collection.id, name: trimmed, description: collection.description });
    } else {
      setName(collection.name);
    }
  }

  function startCreating(kind: "folder" | "request") {
    setMenuOpen(false);
    if (!expanded) onToggleExpand(collection.id);
    setDraftName("");
    setCreating(kind);
  }

  async function commitCreate() {
    const trimmed = draftName.trim();
    setCreating(null);
    if (!trimmed) return;
    if (creating === "folder") {
      createCollection.mutate({ parentId: collection.id, name: trimmed });
      return;
    }
    const saved = await createRequest.mutateAsync({
      collectionId: collection.id,
      input: {
        name: trimmed,
        ...defaultRequest(),
        auth: defaultRequestAuth(),
        preRequestScript: "",
        postResponseScript: "",
      },
    });
    open(saved);
  }

  function onDraftKeyDown(e: KeyboardEvent<HTMLInputElement>) {
    if (e.key === "Enter") void commitCreate();
    if (e.key === "Escape") setCreating(null);
  }

  function handleDragStart(e: DragEvent) {
    e.stopPropagation();
    dragRef.current = { kind: "collection", id: collection.id, parentId: collection.parentId };
  }

  async function exportAs(format: ExportFormat) {
    const content = await ipc.exportCollection(collection.id, { format });
    const base = collection.name.replace(/\s+/g, "_");
    const path = await save({ defaultPath: exportFilename(format, base) });
    if (!path) return;
    try {
      await ipc.writeFileBytes(path, textToBase64(content));
    } catch (e) {
      console.error("failed to export collection:", e);
    }
  }

  async function exportAsPlugin(pluginId: string) {
    const content = await ipc.exportCollection(collection.id, { pluginId });
    const base = collection.name.replace(/\s+/g, "_");
    const path = await save({ defaultPath: `${base}.txt` });
    if (!path) return;
    try {
      await ipc.writeFileBytes(path, textToBase64(content));
    } catch (e) {
      console.error("failed to export collection via plugin:", e);
    }
  }

  // A request dropped here moves into this collection. A collection dropped
  // here either reorders among siblings (same parent as this one) or
  // reparents under this one — guarded against dropping onto itself or one
  // of its own descendants, which `move_to` would reject server-side anyway,
  // but there's no reason to round-trip for a move the UI can already see is invalid.
  function handleDrop(e: DragEvent) {
    e.stopPropagation();
    const drag = dragRef.current;
    dragRef.current = null;
    if (!drag) return;

    if (drag.kind === "request") {
      moveRequest.mutate({ id: drag.id, collectionId: collection.id });
      return;
    }
    if (drag.id === collection.id) return;
    if (drag.parentId === collection.parentId) {
      const siblings = childrenOf(collections, collection.parentId).map((c) => c.id);
      const fromIndex = siblings.indexOf(drag.id);
      const toIndex = siblings.indexOf(collection.id);
      if (fromIndex === -1 || toIndex === -1) return;
      siblings.splice(fromIndex, 1);
      siblings.splice(toIndex, 0, drag.id);
      reorderCollections.mutate(siblings);
      return;
    }
    if (isDescendant(collection.id, drag.id, collections)) return;
    moveCollection.mutate({ id: drag.id, newParentId: collection.id });
  }

  return (
    <div>
      <div
        onClick={() => onToggleExpand(collection.id)}
        draggable={sortMode === "manual"}
        onDragStart={handleDragStart}
        onDragOver={(e) => e.preventDefault()}
        onDrop={handleDrop}
        style={indent}
        className="group flex items-center gap-1 py-1 pr-1.5 text-xs text-slate-700 hover:bg-slate-100 cursor-pointer rounded dark:text-slate-200 dark:hover:bg-slate-800"
      >
        {expanded ? <ChevronDown size={13} className="shrink-0" /> : <ChevronRight size={13} className="shrink-0" />}

        {editing ? (
          <input
            autoFocus
            value={name}
            onClick={(e) => e.stopPropagation()}
            onChange={(e) => setName(e.target.value)}
            onBlur={commitRename}
            onKeyDown={(e) => {
              if (e.key === "Enter") commitRename();
              if (e.key === "Escape") {
                setName(collection.name);
                setEditing(false);
              }
            }}
            className="min-w-0 flex-1 rounded border border-accent/40 bg-white px-1 py-0.5 text-xs focus:outline-none dark:bg-slate-900"
          />
        ) : (
          <span className="min-w-0 flex-1 truncate font-medium">{collection.name}</span>
        )}

        <div ref={menuRef} className="relative shrink-0">
          <button
            type="button"
            onClick={(e) => {
              e.stopPropagation();
              setMenuOpen((o) => !o);
            }}
            title="Collection actions"
            className="rounded p-0.5 text-slate-400 opacity-0 hover:bg-slate-200 group-hover:opacity-100 dark:hover:bg-slate-700"
          >
            <MoreHorizontal size={13} />
          </button>
          {menuOpen && (
            <div
              onClick={(e) => e.stopPropagation()}
              className="absolute right-0 top-full z-10 mt-1 w-40 rounded-md border border-slate-200 bg-white py-1 text-xs shadow-lg dark:border-slate-700 dark:bg-slate-800"
            >
              <button
                type="button"
                onClick={() => startCreating("request")}
                className="flex w-full items-center gap-1.5 px-3 py-1.5 text-left hover:bg-slate-100 dark:hover:bg-slate-700"
              >
                <FilePlus size={12} /> New request
              </button>
              <button
                type="button"
                onClick={() => startCreating("folder")}
                className="flex w-full items-center gap-1.5 px-3 py-1.5 text-left hover:bg-slate-100 dark:hover:bg-slate-700"
              >
                <FolderPlus size={12} /> New subfolder
              </button>
              <button
                type="button"
                onClick={() => {
                  setMenuOpen(false);
                  setEditing(true);
                }}
                className="flex w-full items-center gap-1.5 px-3 py-1.5 text-left hover:bg-slate-100 dark:hover:bg-slate-700"
              >
                <Pencil size={12} /> Rename
              </button>
              <button
                type="button"
                onClick={() => {
                  setMenuOpen(false);
                  duplicateCollection.mutate({ id: collection.id });
                }}
                className="flex w-full items-center gap-1.5 px-3 py-1.5 text-left hover:bg-slate-100 dark:hover:bg-slate-700"
              >
                <Copy size={12} /> Duplicate
              </button>
              <button
                type="button"
                onClick={() => {
                  setMenuOpen(false);
                  setAuthDialogOpen(true);
                }}
                className="flex w-full items-center gap-1.5 px-3 py-1.5 text-left hover:bg-slate-100 dark:hover:bg-slate-700"
              >
                <Lock size={12} /> Auth…
              </button>
              <button
                type="button"
                onClick={() => {
                  setMenuOpen(false);
                  setRunnerOpen(true);
                }}
                className="flex w-full items-center gap-1.5 px-3 py-1.5 text-left hover:bg-slate-100 dark:hover:bg-slate-700"
              >
                <Play size={12} /> Run collection…
              </button>
              <button
                type="button"
                onClick={() => {
                  setMenuOpen(false);
                  setImportOpen(true);
                }}
                className="flex w-full items-center gap-1.5 px-3 py-1.5 text-left hover:bg-slate-100 dark:hover:bg-slate-700"
              >
                <Upload size={12} /> Import here…
              </button>
              <button
                type="button"
                onClick={() => {
                  setMenuOpen(false);
                  void exportAs("postman");
                }}
                className="flex w-full items-center gap-1.5 px-3 py-1.5 text-left hover:bg-slate-100 dark:hover:bg-slate-700"
              >
                <Download size={12} /> Export to Postman…
              </button>
              <button
                type="button"
                onClick={() => {
                  setMenuOpen(false);
                  void exportAs("open_api");
                }}
                className="flex w-full items-center gap-1.5 px-3 py-1.5 text-left hover:bg-slate-100 dark:hover:bg-slate-700"
              >
                <Download size={12} /> Export to OpenAPI 3.0…
              </button>
              <button
                type="button"
                onClick={() => {
                  setMenuOpen(false);
                  void exportAs("har");
                }}
                className="flex w-full items-center gap-1.5 px-3 py-1.5 text-left hover:bg-slate-100 dark:hover:bg-slate-700"
              >
                <Download size={12} /> Export to HAR…
              </button>
              <button
                type="button"
                onClick={() => {
                  setMenuOpen(false);
                  void exportAs("curl");
                }}
                className="flex w-full items-center gap-1.5 px-3 py-1.5 text-left hover:bg-slate-100 dark:hover:bg-slate-700"
              >
                <Download size={12} /> Export to cURL…
              </button>
              {(exportPlugins ?? []).map((p) => (
                <button
                  key={p.id}
                  type="button"
                  onClick={() => {
                    setMenuOpen(false);
                    void exportAsPlugin(p.id);
                  }}
                  className="flex w-full items-center gap-1.5 px-3 py-1.5 text-left hover:bg-slate-100 dark:hover:bg-slate-700"
                >
                  <Download size={12} /> Export to {p.name}…
                </button>
              ))}
              <button
                type="button"
                onClick={() => {
                  setMenuOpen(false);
                  if (
                    confirmDelete(
                      `Delete "${collection.name}" and everything inside it? This can't be undone.`,
                    )
                  ) {
                    deleteCollection.mutate(collection.id);
                  }
                }}
                className="flex w-full items-center gap-1.5 px-3 py-1.5 text-left text-red-500 hover:bg-red-50 dark:hover:bg-red-900/30"
              >
                <Trash2 size={12} /> Delete
              </button>
            </div>
          )}
        </div>
      </div>

      {expanded && (
        <div>
          {children.map((child) => (
            <CollectionNode
              key={child.id}
              collection={child}
              collections={collections}
              depth={depth + 1}
              workspaceId={workspaceId}
              expandedIds={expandedIds}
              onToggleExpand={onToggleExpand}
              dragRef={dragRef}
              sortMode={sortMode}
            />
          ))}
          {creating && (
            <div style={{ paddingLeft: 6 + (depth + 1) * 14 }} className="py-1 pr-1.5">
              <input
                autoFocus
                value={draftName}
                placeholder={creating === "folder" ? "Folder name" : "Request name"}
                onChange={(e) => setDraftName(e.target.value)}
                onBlur={() => void commitCreate()}
                onKeyDown={onDraftKeyDown}
                className="w-full rounded border border-accent/40 bg-white px-1 py-0.5 text-xs focus:outline-none dark:bg-slate-900"
              />
            </div>
          )}
          <RequestList collectionId={collection.id} workspaceId={workspaceId} dragRef={dragRef} sortMode={sortMode} />
        </div>
      )}

      {authDialogOpen && (
        <CollectionAuthDialog
          collection={collection}
          workspaceId={workspaceId}
          onClose={() => setAuthDialogOpen(false)}
        />
      )}

      {importOpen && workspaceId && (
        <ImportDialog
          workspaceId={workspaceId}
          parentId={collection.id}
          onClose={() => setImportOpen(false)}
        />
      )}

      {runnerOpen && workspaceId && (
        <div
          onClick={(e) => e.stopPropagation()}
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/40"
        >
          <div className="h-[70vh] w-[640px] max-w-[95vw] overflow-hidden rounded-xl border border-slate-200 shadow-2xl dark:border-slate-700">
            <CollectionRunner
              workspaceId={workspaceId}
              collectionId={collection.id}
              collectionName={collection.name}
              onClose={() => setRunnerOpen(false)}
            />
          </div>
        </div>
      )}
    </div>
  );
}

/** Filename for a collection-export artifact, per format. Kept here (next
 * to the only caller) rather than in `lib/` because there's no second
 * consumer yet — the codegen download in `CodeTab` already carries its own
 * per-language extension table. */
function exportFilename(format: ExportFormat, baseName: string): string {
  switch (format) {
    case "postman":
      return `${baseName}.postman_collection.json`;
    case "open_api":
      return `${baseName}.openapi.json`;
    case "har":
      return `${baseName}.har`;
    case "curl":
      return `${baseName}.sh`;
  }
}
