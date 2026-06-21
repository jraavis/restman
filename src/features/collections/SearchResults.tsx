//! Flat FTS5 search results, swapped in for the collection tree while a
//! search query is active. Name/URL come pre-marked by SQLite — see
//! `renderHighlight` — rather than re-matched on the frontend.

import { Loader2 } from "lucide-react";
import { methodBadgeClasses } from "../../lib/methods";
import { useRequestStore } from "../../stores/requestStore";
import { useSearchRequests } from "./hooks";
import { useOpenRequest } from "./useOpenRequest";
import { renderHighlight } from "./highlight";

export function SearchResults({
  workspaceId,
  query,
  method,
  tag,
}: {
  workspaceId: string | undefined;
  query: string;
  method: string | null;
  tag: string | null;
}) {
  const { data: hits, isLoading } = useSearchRequests(workspaceId, query, method);
  const { open } = useOpenRequest(workspaceId);
  const activeRequestId = useRequestStore((s) => s.requestId);

  if (isLoading) {
    return (
      <div className="flex items-center justify-center gap-2 p-6 text-sm text-slate-400">
        <Loader2 size={15} className="animate-spin" /> Searching…
      </div>
    );
  }

  const filtered = tag ? hits?.filter((h) => h.request.tags.some((t) => t.id === tag)) : hits;

  if (!filtered || filtered.length === 0) {
    return <p className="p-6 text-center text-sm text-slate-400">No matching requests.</p>;
  }

  return (
    <div>
      {filtered.map((hit) => (
        <div
          key={hit.request.id}
          onClick={() => open(hit.request)}
          title={hit.request.url}
          className={
            "group flex items-center gap-1.5 rounded py-1 px-1.5 text-xs cursor-pointer " +
            (hit.request.id === activeRequestId
              ? "bg-accent/10 text-accent"
              : "text-slate-600 hover:bg-slate-100 dark:text-slate-300 dark:hover:bg-slate-800")
          }
        >
          <span
            className={"shrink-0 rounded border px-1 text-[10px] font-bold " + methodBadgeClasses(hit.request.method)}
          >
            {hit.request.method}
          </span>
          <span className="min-w-0 flex-1 truncate">
            <span className="font-medium">{renderHighlight(hit.nameHighlight)}</span>
            <span className="ml-1.5 text-slate-400">{renderHighlight(hit.urlHighlight)}</span>
          </span>
        </div>
      ))}
    </div>
  );
}
