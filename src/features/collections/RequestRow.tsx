//! A single saved request row inside a collection's request list: open into
//! a tab on click, inline rename, duplicate, delete, tag assignment — all
//! via a small hover-revealed context menu.

import { useState } from "react";
import { Cable, Copy, MoreHorizontal, Network, Pencil, Radio, Tags, Trash2 } from "lucide-react";
import { methodBadgeClasses } from "../../lib/methods";
import { useDismissable } from "../../lib/useDismissable";
import type { SavedRequest } from "../../lib/types";
import { TagPicker } from "./TagPicker";

const STREAMING_KIND_ICON = { sse: Radio, ws: Cable, grpc: Network } as const;
const STREAMING_KIND_LABEL = { sse: "SSE", ws: "WS", grpc: "gRPC" } as const;

export function RequestRow({
  request,
  workspaceId,
  collectionId,
  isOpen,
  onOpen,
  onRename,
  onDuplicate,
  onDelete,
}: {
  request: SavedRequest;
  workspaceId: string | undefined;
  collectionId: string;
  isOpen: boolean;
  onOpen: () => void;
  onRename: (name: string) => void;
  onDuplicate: () => void;
  onDelete: () => void;
}) {
  const [editing, setEditing] = useState(false);
  const [name, setName] = useState(request.name);
  const [menuOpen, setMenuOpen] = useState(false);
  const [tagsOpen, setTagsOpen] = useState(false);
  const menuRef = useDismissable<HTMLDivElement>(() => setMenuOpen(false));
  const tagsRef = useDismissable<HTMLDivElement>(() => setTagsOpen(false));

  function commitRename() {
    setEditing(false);
    const trimmed = name.trim();
    if (trimmed && trimmed !== request.name) onRename(trimmed);
    else setName(request.name);
  }

  return (
    <div
      onClick={() => !editing && onOpen()}
      title={request.url}
      className={
        "group flex items-center gap-1.5 py-1 pr-1.5 text-xs cursor-pointer rounded " +
        (isOpen
          ? "bg-accent/10 text-accent"
          : "text-slate-600 hover:bg-slate-100 dark:text-slate-300 dark:hover:bg-slate-800")
      }
    >
      {request.kind === "http" ? (
        <span
          className={"shrink-0 rounded border px-1 text-[10px] font-bold " + methodBadgeClasses(request.method)}
        >
          {request.method}
        </span>
      ) : (
        <span
          title={STREAMING_KIND_LABEL[request.kind]}
          className="flex shrink-0 items-center gap-0.5 rounded border border-slate-300 px-1 text-[10px] font-bold text-slate-500 dark:border-slate-600 dark:text-slate-400"
        >
          {(() => {
            const Icon = STREAMING_KIND_ICON[request.kind];
            return <Icon size={10} />;
          })()}
          {STREAMING_KIND_LABEL[request.kind]}
        </span>
      )}

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
              setName(request.name);
              setEditing(false);
            }
          }}
          className="min-w-0 flex-1 rounded border border-accent/40 bg-white px-1 py-0.5 text-xs focus:outline-none dark:bg-slate-900"
        />
      ) : (
        <span className="min-w-0 flex-1 truncate">{request.name}</span>
      )}

      {request.tags.length > 0 && (
        <span className="flex shrink-0 gap-0.5">
          {request.tags.map((t) => (
            <span
              key={t.id}
              title={t.name}
              className="h-1.5 w-1.5 rounded-full"
              style={{ backgroundColor: t.color }}
            />
          ))}
        </span>
      )}

      <div ref={tagsRef} className="relative shrink-0">
        <button
          type="button"
          onClick={(e) => {
            e.stopPropagation();
            setMenuOpen(false);
            setTagsOpen((o) => !o);
          }}
          title="Tags"
          className="rounded p-0.5 text-slate-400 opacity-0 hover:bg-slate-200 group-hover:opacity-100 dark:hover:bg-slate-700"
        >
          <Tags size={13} />
        </button>
        {tagsOpen && (
          <div
            onClick={(e) => e.stopPropagation()}
            className="absolute right-0 top-full z-10 mt-1 rounded-md border border-slate-200 bg-white shadow-lg dark:border-slate-700 dark:bg-slate-800"
          >
            <TagPicker request={request} workspaceId={workspaceId} collectionId={collectionId} />
          </div>
        )}
      </div>

      <div ref={menuRef} className="relative shrink-0">
        <button
          type="button"
          onClick={(e) => {
            e.stopPropagation();
            setTagsOpen(false);
            setMenuOpen((o) => !o);
          }}
          title="Request actions"
          className="rounded p-0.5 text-slate-400 opacity-0 hover:bg-slate-200 group-hover:opacity-100 dark:hover:bg-slate-700"
        >
          <MoreHorizontal size={13} />
        </button>
        {menuOpen && (
          <div
            onClick={(e) => e.stopPropagation()}
            className="absolute right-0 top-full z-10 mt-1 w-32 rounded-md border border-slate-200 bg-white py-1 text-xs shadow-lg dark:border-slate-700 dark:bg-slate-800"
          >
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
                onDuplicate();
              }}
              className="flex w-full items-center gap-1.5 px-3 py-1.5 text-left hover:bg-slate-100 dark:hover:bg-slate-700"
            >
              <Copy size={12} /> Duplicate
            </button>
            <button
              type="button"
              onClick={() => {
                setMenuOpen(false);
                onDelete();
              }}
              className="flex w-full items-center gap-1.5 px-3 py-1.5 text-left text-red-500 hover:bg-red-50 dark:hover:bg-red-900/30"
            >
              <Trash2 size={12} /> Delete
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
