//! Cross-cutting "which streaming panel is open" state. The SSE/WS/gRPC
//! panels are standalone modals rendered from `TopBar` (not tab-backed —
//! see PLAN.md), but they also need to be openable from a saved request deep
//! in the collection tree (`CollectionNode`/`RequestList`), which has no
//! prop path to `TopBar`'s local state. A tiny non-persisted store bridges
//! the two: the tree calls `openStreamingPanel` with the saved request to
//! reopen, `TopBar`'s toolbar buttons call it with no request (fresh
//! connect), and `TopBar` alone renders the panel based on this state.

import { create } from "zustand";
import type { RequestKind, SavedRequest } from "../lib/types";

export type StreamingKind = Exclude<RequestKind, "http">;

interface StreamingPanel {
  kind: StreamingKind;
  savedRequest: SavedRequest | null;
}

interface StreamingPanelState {
  panel: StreamingPanel | null;
  openStreamingPanel: (kind: StreamingKind, savedRequest?: SavedRequest | null) => void;
  closeStreamingPanel: () => void;
}

export const useStreamingPanelStore = create<StreamingPanelState>((set) => ({
  panel: null,
  openStreamingPanel: (kind, savedRequest = null) => set({ panel: { kind, savedRequest } }),
  closeStreamingPanel: () => set({ panel: null }),
}));
