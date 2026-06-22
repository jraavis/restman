//! Collection-level auth editor — same modal shell as `SaveRequestDialog`,
//! wrapping the shared per-type fields. No "Inherit" option here: a
//! collection's auth is always a plain `AuthConfig` (default `None`) — see
//! `model::auth::RequestAuth`'s doc comment.

import { useState } from "react";
import { emptyAuthConfig, type AuthConfig, type AuthType, type Collection } from "../../lib/types";
import { ALL_AUTH_TYPES, AUTH_TYPE_LABELS, AuthConfigFields, inputClass } from "../request/AuthConfigFields";
import { useUpdateCollectionAuth } from "./hooks";

export function CollectionAuthDialog({
  collection,
  workspaceId,
  onClose,
}: {
  collection: Collection;
  workspaceId: string | undefined;
  onClose: () => void;
}) {
  const [auth, setAuth] = useState<AuthConfig>(collection.auth);
  const updateAuth = useUpdateCollectionAuth(workspaceId);

  function save() {
    updateAuth.mutate({ id: collection.id, auth }, { onSuccess: onClose });
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/30" onClick={onClose}>
      <div
        onClick={(e) => e.stopPropagation()}
        className="w-96 rounded-lg border border-slate-200 bg-white p-4 shadow-xl dark:border-slate-700 dark:bg-slate-800"
      >
        <h2 className="mb-3 text-sm font-semibold text-slate-800 dark:text-slate-100">
          Auth — {collection.name}
        </h2>

        <div className="flex max-h-[60vh] flex-col gap-3 overflow-y-auto">
          <label className="flex flex-col gap-1 text-sm">
            <span className="text-xs text-slate-500 dark:text-slate-400">Type</span>
            <select
              className={inputClass}
              value={auth.type}
              onChange={(e) => setAuth(emptyAuthConfig(e.target.value as AuthType))}
            >
              {ALL_AUTH_TYPES.map((t) => (
                <option key={t} value={t}>
                  {AUTH_TYPE_LABELS[t]}
                </option>
              ))}
            </select>
          </label>

          <AuthConfigFields value={auth} onChange={setAuth} scope={{ collectionId: collection.id }} />
        </div>

        <div className="mt-4 flex justify-end gap-2 text-sm">
          <button
            type="button"
            onClick={onClose}
            className="rounded-md px-3 py-1.5 text-slate-500 hover:bg-slate-100 dark:hover:bg-slate-700"
          >
            Cancel
          </button>
          <button
            type="button"
            disabled={updateAuth.isPending}
            onClick={save}
            className="rounded-md bg-accent px-3 py-1.5 font-medium text-white hover:bg-accent-hover disabled:opacity-50"
          >
            {updateAuth.isPending ? "Saving…" : "Save"}
          </button>
        </div>
      </div>
    </div>
  );
}
