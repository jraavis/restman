//! "Save As" prompt shown the first time a draft is saved: a name and a
//! flat, indented collection picker built from the workspace's collection
//! tree. Once a request is linked, later saves skip this and overwrite in
//! place — see `useSaveRequest`.

import { useState } from "react";
import type { Collection } from "../../lib/types";

function collectionOptions(collections: Collection[]): { id: string; label: string }[] {
  const byParent = new Map<string | null, Collection[]>();
  for (const c of collections) {
    const siblings = byParent.get(c.parentId) ?? [];
    siblings.push(c);
    byParent.set(c.parentId, siblings);
  }
  for (const siblings of byParent.values()) siblings.sort((a, b) => a.sortOrder - b.sortOrder);

  const options: { id: string; label: string }[] = [];
  function walk(parentId: string | null, depth: number) {
    for (const c of byParent.get(parentId) ?? []) {
      options.push({ id: c.id, label: "  ".repeat(depth) + c.name });
      walk(c.id, depth + 1);
    }
  }
  walk(null, 0);
  return options;
}

export function SaveRequestDialog({
  defaultName,
  collections,
  saving,
  onSave,
  onClose,
}: {
  defaultName: string;
  collections: Collection[];
  saving: boolean;
  onSave: (name: string, collectionId: string) => void;
  onClose: () => void;
}) {
  const options = collectionOptions(collections);
  const [name, setName] = useState(defaultName);
  const [collectionId, setCollectionId] = useState(options[0]?.id ?? "");

  const canSave = name.trim() !== "" && collectionId !== "" && !saving;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/30"
      onClick={onClose}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        className="w-80 rounded-lg border border-slate-200 bg-white p-4 shadow-xl dark:border-slate-700 dark:bg-slate-800"
      >
        <h2 className="mb-3 text-sm font-semibold text-slate-800 dark:text-slate-100">Save request</h2>

        <label className="mb-3 block text-xs">
          <span className="mb-1 block text-slate-500 dark:text-slate-400">Name</span>
          <input
            autoFocus
            value={name}
            onChange={(e) => setName(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && canSave) onSave(name.trim(), collectionId);
              if (e.key === "Escape") onClose();
            }}
            className="w-full rounded-md border border-slate-200 bg-transparent px-2 py-1.5 text-sm focus:outline-none focus:ring-2 focus:ring-accent/40 dark:border-slate-600"
          />
        </label>

        <label className="mb-4 block text-xs">
          <span className="mb-1 block text-slate-500 dark:text-slate-400">Collection</span>
          {options.length === 0 ? (
            <p className="text-slate-400">Create a collection first.</p>
          ) : (
            <select
              value={collectionId}
              onChange={(e) => setCollectionId(e.target.value)}
              className="w-full rounded-md border border-slate-200 bg-transparent px-2 py-1.5 text-sm focus:outline-none focus:ring-2 focus:ring-accent/40 dark:border-slate-600"
            >
              {options.map((o) => (
                <option key={o.id} value={o.id} className="text-slate-900">
                  {o.label}
                </option>
              ))}
            </select>
          )}
        </label>

        <div className="flex justify-end gap-2 text-sm">
          <button
            type="button"
            onClick={onClose}
            className="rounded-md px-3 py-1.5 text-slate-500 hover:bg-slate-100 dark:hover:bg-slate-700"
          >
            Cancel
          </button>
          <button
            type="button"
            disabled={!canSave}
            onClick={() => onSave(name.trim(), collectionId)}
            className="rounded-md bg-accent px-3 py-1.5 font-medium text-white hover:bg-accent-hover disabled:opacity-50"
          >
            {saving ? "Saving…" : "Save"}
          </button>
        </div>
      </div>
    </div>
  );
}
