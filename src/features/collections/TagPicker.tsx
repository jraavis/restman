//! Popover for assigning workspace tags to a single request: toggle existing
//! tags on/off, or create a new one inline with a color swatch. Tags
//! created/deleted here are workspace-wide — same set shown as color dots on
//! every request row and in search results.

import { useState } from "react";
import { confirmDelete } from "../../lib/confirmDelete";
import { Check, Plus, X } from "lucide-react";
import type { SavedRequest, Tag } from "../../lib/types";
import { useCreateTag, useDeleteTag, useSetRequestTags, useTags } from "./hooks";

const TAG_COLORS = [
  "#ef4444",
  "#f97316",
  "#eab308",
  "#22c55e",
  "#06b6d4",
  "#3b82f6",
  "#8b5cf6",
  "#ec4899",
];

export function TagPicker({
  request,
  workspaceId,
  collectionId,
}: {
  request: SavedRequest;
  workspaceId: string | undefined;
  collectionId: string;
}) {
  const { data: tags } = useTags(workspaceId);
  const createTag = useCreateTag(workspaceId);
  const deleteTag = useDeleteTag(workspaceId);
  const setRequestTags = useSetRequestTags(collectionId);
  const [draftName, setDraftName] = useState("");
  const [draftColor, setDraftColor] = useState(TAG_COLORS[0]);

  const activeIds = new Set(request.tags.map((t) => t.id));

  function toggle(tag: Tag) {
    const currentIds = request.tags.map((t) => t.id);
    const nextIds = activeIds.has(tag.id)
      ? currentIds.filter((id) => id !== tag.id)
      : [...currentIds, tag.id];
    setRequestTags.mutate({ requestId: request.id, tagIds: nextIds });
  }

  async function commitCreate() {
    const trimmed = draftName.trim();
    if (!trimmed) return;
    const tag = await createTag.mutateAsync({ name: trimmed, color: draftColor });
    setDraftName("");
    setRequestTags.mutate({
      requestId: request.id,
      tagIds: [...request.tags.map((t) => t.id), tag.id],
    });
  }

  return (
    <div className="w-48 p-1.5">
      <div className="max-h-40 overflow-auto">
        {tags?.length === 0 && <p className="px-1.5 py-1 text-xs text-slate-400">No tags yet.</p>}
        {tags?.map((tag) => (
          <div
            key={tag.id}
            className="group flex items-center gap-1.5 rounded px-1.5 py-1 text-xs hover:bg-slate-100 dark:hover:bg-slate-700"
          >
            <button
              type="button"
              onClick={() => toggle(tag)}
              className="flex min-w-0 flex-1 items-center gap-1.5 text-left"
            >
              <span
                className="flex h-3.5 w-3.5 shrink-0 items-center justify-center rounded-full"
                style={{ backgroundColor: tag.color }}
              >
                {activeIds.has(tag.id) && <Check size={10} className="text-white" />}
              </span>
              <span className="min-w-0 flex-1 truncate">{tag.name}</span>
            </button>
            <button
              type="button"
              title="Delete tag"
              onClick={() => {
                if (confirmDelete(`Delete tag "${tag.name}"? Removes it from all requests.`)) {
                  deleteTag.mutate(tag.id);
                }
              }}
              className="shrink-0 rounded p-0.5 text-slate-400 opacity-0 hover:bg-red-100 hover:text-red-600 group-hover:opacity-100 dark:hover:bg-red-900/40"
            >
              <X size={11} />
            </button>
          </div>
        ))}
      </div>

      <div className="mt-1.5 border-t border-slate-100 pt-1.5 dark:border-slate-700">
        <div className="flex gap-1">
          {TAG_COLORS.map((c) => (
            <button
              key={c}
              type="button"
              title={c}
              onClick={() => setDraftColor(c)}
              className={
                "h-4 w-4 shrink-0 rounded-full " +
                (draftColor === c ? "ring-2 ring-offset-1 ring-accent" : "")
              }
              style={{ backgroundColor: c }}
            />
          ))}
        </div>
        <div className="mt-1 flex items-center gap-1">
          <input
            value={draftName}
            onChange={(e) => setDraftName(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") void commitCreate();
            }}
            placeholder="New tag"
            className="min-w-0 flex-1 rounded border border-slate-200 bg-transparent px-1 py-0.5 text-xs focus:outline-none dark:border-slate-700"
          />
          <button
            type="button"
            onClick={() => void commitCreate()}
            title="Create tag"
            className="shrink-0 rounded p-1 text-slate-400 hover:bg-slate-200 hover:text-slate-700 dark:hover:bg-slate-700"
          >
            <Plus size={12} />
          </button>
        </div>
      </div>
    </div>
  );
}
