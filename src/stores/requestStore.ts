//! The request currently being edited (the active tab's live draft) and its
//! latest response. Tab persistence itself lives in the DB via
//! `features/tabs/hooks.ts`; this store only holds the in-memory draft that
//! gets debounce-flushed there — see `TabsBar`'s sync effect.

import { create } from "zustand";
import {
  defaultRequest,
  type HeaderEntry,
  type HttpRequest,
  type HttpResponse,
  type KeyValue,
  type RequestBody,
  type RequestOptions,
} from "../lib/http";
import {
  defaultRequestAuth,
  type RequestAuth,
  type ScriptResult,
  type SendResponse,
} from "../lib/types";

interface RequestState {
  /** Tab this draft belongs to. Null only for the brief window before the first tab loads. */
  activeTabId: string | null;
  /** Linked `SavedRequest`, if this draft was opened from (or saved to) a collection. */
  requestId: string | null;
  /** The linked request's collection, for collection-scoped variable resolution on send. */
  collectionId: string | null;
  title: string;
  request: HttpRequest;
  /** Pre/post scripts for the currently-edited request. */
  preRequestScript: string;
  postResponseScript: string;
  /** This draft's own auth, separate from `request` — like `title`, it's
   * `SavedRequest`/`SavedRequestInput` state, not part of the `HttpRequest`
   * wire shape (which only ever carries the already-resolved, never-Inherit
   * config, and is never round-tripped back from the backend — see
   * `HttpRequest.auth`'s doc comment). */
  auth: RequestAuth;
  response: HttpResponse | null;
  preScript: ScriptResult | null;
  postScript: ScriptResult | null;
  sending: boolean;
  error: string | null;

  setMethod: (method: string) => void;
  setUrl: (url: string) => void;
  setHeaders: (headers: HeaderEntry[]) => void;
  setQuery: (query: KeyValue[]) => void;
  setBody: (body: RequestBody) => void;
  setOptions: (options: Partial<RequestOptions>) => void;
  setTitle: (title: string) => void;
  setAuth: (auth: RequestAuth) => void;
  setPreRequestScript: (script: string) => void;
  setPostResponseScript: (script: string) => void;

  /** Replace the whole draft wholesale — used on tab switch/restore, never on a single field edit. */
  loadTab: (args: {
    tabId: string;
    requestId: string | null;
    collectionId: string | null;
    title: string;
    draft: HttpRequest;
    auth: RequestAuth;
    preRequestScript?: string;
    postResponseScript?: string;
  }) => void;
  /** Record that the current draft now has a saved-request home (after a first Save). */
  setRequestLink: (requestId: string, collectionId: string) => void;
  /** Replace just the draft + title in place — e.g. loading a history entry for replay. Unlike `loadTab`, leaves tab/request/collection linkage untouched. */
  loadDraft: (draft: HttpRequest, title: string) => void;

  beginSend: () => void;
  setSendResponse: (result: SendResponse) => void;
  setError: (error: string) => void;
}

export const useRequestStore = create<RequestState>((set) => ({
  activeTabId: null,
  requestId: null,
  collectionId: null,
  title: "Untitled",
  request: defaultRequest(),
  preRequestScript: "",
  postResponseScript: "",
  auth: defaultRequestAuth(),
  response: null,
  preScript: null,
  postScript: null,
  sending: false,
  error: null,

  setMethod: (method) => set((s) => ({ request: { ...s.request, method } })),
  setUrl: (url) => set((s) => ({ request: { ...s.request, url } })),
  setHeaders: (headers) => set((s) => ({ request: { ...s.request, headers } })),
  setQuery: (query) => set((s) => ({ request: { ...s.request, query } })),
  setBody: (body) => set((s) => ({ request: { ...s.request, body } })),
  setOptions: (options) =>
    set((s) => ({
      request: { ...s.request, options: { ...s.request.options, ...options } },
    })),
  setTitle: (title) => set({ title }),
  setAuth: (auth) => set({ auth }),
  setPreRequestScript: (preRequestScript) => set({ preRequestScript }),
  setPostResponseScript: (postResponseScript) => set({ postResponseScript }),

  loadTab: ({
    tabId,
    requestId,
    collectionId,
    title,
    draft,
    auth,
    preRequestScript = "",
    postResponseScript = "",
  }) =>
    set({
      activeTabId: tabId,
      requestId,
      collectionId,
      title,
      request: draft,
      auth,
      preRequestScript,
      postResponseScript,
      response: null,
      preScript: null,
      postScript: null,
      error: null,
      sending: false,
    }),
  setRequestLink: (requestId, collectionId) => set({ requestId, collectionId }),
  loadDraft: (draft, title) =>
    set({
      request: draft,
      title,
      response: null,
      preScript: null,
      postScript: null,
      error: null,
      sending: false,
    }),

  beginSend: () => set({ sending: true, error: null }),
  setSendResponse: ({ response, preScript, postScript }) =>
    set({ response, preScript, postScript, sending: false, error: null }),
  setError: (error) => set({ error, sending: false }),
}));
