//! `SyncMode: "live"` auto-export trigger (Phase 8). Called from the
//! `onSuccess` of collection/request/environment/variable mutations that
//! already have a `workspaceId` in scope — see `crate::sync` module doc
//! (Rust side) for why this is one-directional (DB -> folder) and why
//! import never auto-triggers.
//!
//! Resolves `syncMode` via `queryClient.fetchQuery` — the *same* query key
//! `useWorkspaceSettings` uses, so a still-fresh cache entry (e.g. the
//! settings dialog was just open) is reused instead of double-fetched, but
//! an entry evicted by the default 5-minute `gcTime` (the common case: this
//! fires from mutations made long after the dialog was closed) still
//! resolves correctly instead of silently going quiet. This still tells
//! "auto-trigger" apart from "user clicked Sync now" (which calls
//! `ipc.syncExport` directly, bypassing this file) because the gate is on
//! `syncMode === "live"` specifically — `sync_export` itself works whenever
//! *either* `Manual` or `Live` is configured, so without this file's own
//! mode check, wiring it in unconditionally would spam an export after
//! every mutation even in `Manual` mode.
//!
//! Wired into every mutation that changes what `sync_export` writes:
//! collection CRUD/move/reorder/duplicate, request CRUD/move/reorder/
//! duplicate, environment CRUD, and environment-scoped variable CRUD.
//! Hooks that don't carry a `workspaceId` call this with `undefined` and
//! the active workspace is resolved here instead (every mutation the UI
//! can issue operates on the active workspace by construction). Tag CRUD
//! and tag assignment are deliberately *not* wired: tags are not part of
//! the exported `ImportedNode`/environment formats, so they can't change
//! the synced files.
//!
//! Fire-and-forget: a live-sync failure (e.g. the configured folder was
//! deleted out from under the app, or `syncMode` couldn't be resolved) must
//! never surface as a mutation error to the user who just saved a request —
//! it's logged to the console only.

import type { QueryClient } from "@tanstack/react-query";
import { ipc } from "./ipc";
import { workspaceKeys } from "../features/workspaces/hooks";

export function triggerLiveSyncIfEnabled(qc: QueryClient, workspaceId?: string | null) {
  void resolveWorkspaceId(qc, workspaceId)
    .then((id) => {
      if (!id) return;
      return qc
        .fetchQuery({
          queryKey: workspaceKeys.settings(id),
          queryFn: () => ipc.getWorkspaceSettings(id),
        })
        .then((settings) => {
          if (settings.syncMode !== "live") return;
          return ipc.syncExport(id);
        });
    })
    .catch((e) => console.error("live sync export failed:", e));
}

/** Callers without a `workspaceId` in scope fall back to the active
 * workspace — same query key `useActiveWorkspace` uses, so a fresh cache
 * entry is reused rather than double-fetched. */
async function resolveWorkspaceId(
  qc: QueryClient,
  workspaceId: string | undefined | null,
): Promise<string | null> {
  if (workspaceId) return workspaceId;
  const active = await qc.fetchQuery({
    queryKey: workspaceKeys.active,
    queryFn: ipc.activeWorkspace,
  });
  return active?.id ?? null;
}
