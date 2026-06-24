//! Cookie jar viewer — read-only list of cookies in the shared jar. The jar
//! is app-global (`AppState.cookie_jar`), not workspace-scoped, so this
//! dialog takes no workspace prop; same modal shell as `WorkspaceSettingsDialog`.

import type { ReactNode } from "react";
import { Loader2, Trash2 } from "lucide-react";
import type { CookieEntry } from "../../lib/types";
import { useClearCookies, useCookies, useDeleteCookie } from "./hooks";

export function CookieJarDialog({ onClose }: { onClose: () => void }) {
  const { data: cookies, isLoading, isError } = useCookies();
  const deleteCookie = useDeleteCookie();
  const clearCookies = useClearCookies();

  function handleClearAll() {
    if (window.confirm("Clear all cookies? This can't be undone.")) {
      clearCookies.mutate();
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/30" onClick={onClose}>
      <div
        onClick={(e) => e.stopPropagation()}
        className="flex max-h-[85vh] w-[40rem] flex-col rounded-lg border border-slate-200 bg-white p-4 shadow-xl dark:border-slate-700 dark:bg-slate-800"
      >
        <div className="mb-3 flex items-center justify-between">
          <h2 className="text-sm font-semibold text-slate-800 dark:text-slate-100">Cookies</h2>
          <button
            type="button"
            onClick={handleClearAll}
            disabled={!cookies?.length || clearCookies.isPending}
            className="text-xs text-slate-400 hover:text-red-500 disabled:opacity-40 disabled:hover:text-slate-400"
          >
            Clear all
          </button>
        </div>

        {isLoading && (
          <div className="flex items-center justify-center gap-2 p-6 text-sm text-slate-400">
            <Loader2 size={15} className="animate-spin" /> Loading…
          </div>
        )}
        {isError && <p className="text-sm text-red-500">Couldn't load cookies.</p>}
        {!isLoading && !isError && cookies?.length === 0 && (
          <div className="flex flex-col items-center justify-center gap-1 p-6 text-center text-sm text-slate-400">
            <p>No cookies stored.</p>
            <p className="text-xs">
              Enable "Send cookies (shared jar)" on a request and they'll show up here.
            </p>
          </div>
        )}

        {cookies && cookies.length > 0 && (
          <div className="min-h-0 flex-1 overflow-auto">
            {cookies.map((c) => (
              <CookieRow
                key={`${c.domain}|${c.path}|${c.name}`}
                cookie={c}
                onDelete={() => deleteCookie.mutate({ domain: c.domain, path: c.path, name: c.name })}
              />
            ))}
          </div>
        )}

        <div className="mt-4 flex justify-end text-sm">
          <button
            type="button"
            onClick={onClose}
            className="rounded-md px-3 py-1.5 text-slate-500 hover:bg-slate-100 dark:hover:bg-slate-700"
          >
            Close
          </button>
        </div>
      </div>
    </div>
  );
}

function CookieRow({ cookie, onDelete }: { cookie: CookieEntry; onDelete: () => void }) {
  return (
    <div className="group flex items-start gap-2 border-b border-slate-100 px-1 py-2 text-xs last:border-0 hover:bg-slate-50 dark:border-slate-800 dark:hover:bg-slate-800/50">
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-1.5">
          <span className="truncate font-medium text-slate-700 dark:text-slate-200">{cookie.name}</span>
          <span className="text-slate-400">·</span>
          <span className="truncate text-slate-400">
            {cookie.domain}
            {cookie.path}
          </span>
        </div>
        <p
          className="mt-0.5 truncate font-mono text-slate-500 dark:text-slate-400"
          title={cookie.value}
        >
          {cookie.value}
        </p>
        <div className="mt-1 flex items-center gap-1 text-slate-400">
          <span>
            {cookie.expiresAt != null
              ? `Expires ${new Date(cookie.expiresAt * 1000).toLocaleString()}`
              : "Session"}
          </span>
          {cookie.secure && <Badge>Secure</Badge>}
          {cookie.httpOnly && <Badge>HttpOnly</Badge>}
          {cookie.sameSite && <Badge>{cookie.sameSite}</Badge>}
        </div>
      </div>
      <button
        type="button"
        title="Delete cookie"
        onClick={onDelete}
        className="shrink-0 rounded p-1 text-slate-400 opacity-0 hover:bg-red-100 hover:text-red-600 group-hover:opacity-100 dark:hover:bg-red-900/40"
      >
        <Trash2 size={13} />
      </button>
    </div>
  );
}

function Badge({ children }: { children: ReactNode }) {
  return <span className="rounded border border-slate-200 px-1 py-0.5 dark:border-slate-700">{children}</span>;
}
