//! Request-level Auth tab: one type picker — "Inherit from collection" plus
//! every concrete `AuthType` — wrapping the shared per-type fields. See
//! `lib/types.ts`'s `applyAuthOption` for why re-selecting the type already
//! in effect preserves its fields instead of resetting them.

import { applyAuthOption, authOptionValue, type AuthOptionValue, type RequestAuth } from "../../lib/types";
import { ALL_AUTH_TYPES, AUTH_TYPE_LABELS, AuthConfigFields, inputClass } from "./AuthConfigFields";

export function RequestAuthTab({
  auth,
  onChange,
  collectionId,
  requestId,
}: {
  auth: RequestAuth;
  onChange: (auth: RequestAuth) => void;
  collectionId: string | null;
  requestId: string | null;
}) {
  const selected = authOptionValue(auth);

  return (
    <div className="flex flex-col gap-3">
      <label className="flex flex-col gap-1 text-sm">
        <span className="text-xs text-slate-500 dark:text-slate-400">Type</span>
        <select
          className={inputClass}
          value={selected}
          onChange={(e) => onChange(applyAuthOption(auth, e.target.value as AuthOptionValue))}
        >
          <option value="inherit">Inherit from collection</option>
          {ALL_AUTH_TYPES.map((t) => (
            <option key={t} value={t}>
              {AUTH_TYPE_LABELS[t]}
            </option>
          ))}
        </select>
      </label>

      {auth.mode === "inherit" ? (
        <p className="text-sm text-slate-400">
          {collectionId ? "Using this request's collection auth." : "Not in a collection — no auth applied."}
        </p>
      ) : (
        <>
          {requestId === null && (
            <p className="text-xs text-amber-600 dark:text-amber-400">
              Save this request to apply its authentication.
            </p>
          )}
          <AuthConfigFields
            value={auth}
            onChange={(cfg) => onChange({ mode: "own", ...cfg })}
            scope={{ collectionId, requestId }}
          />
        </>
      )}
    </div>
  );
}
