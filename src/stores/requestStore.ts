//! The request currently being edited and its latest response. Tabs and
//! persistence arrive in Phase 2; for now this holds a single working request.

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

interface RequestState {
  request: HttpRequest;
  response: HttpResponse | null;
  sending: boolean;
  error: string | null;

  setMethod: (method: string) => void;
  setUrl: (url: string) => void;
  setHeaders: (headers: HeaderEntry[]) => void;
  setQuery: (query: KeyValue[]) => void;
  setBody: (body: RequestBody) => void;
  setOptions: (options: Partial<RequestOptions>) => void;

  beginSend: () => void;
  setResponse: (response: HttpResponse) => void;
  setError: (error: string) => void;
}

export const useRequestStore = create<RequestState>((set) => ({
  request: defaultRequest(),
  response: null,
  sending: false,
  error: null,

  setMethod: (method) => set((s) => ({ request: { ...s.request, method } })),
  setUrl: (url) => set((s) => ({ request: { ...s.request, url } })),
  setHeaders: (headers) => set((s) => ({ request: { ...s.request, headers } })),
  setQuery: (query) => set((s) => ({ request: { ...s.request, query } })),
  setBody: (body) => set((s) => ({ request: { ...s.request, body } })),
  setOptions: (options) =>
    set((s) => ({ request: { ...s.request, options: { ...s.request.options, ...options } } })),

  beginSend: () => set({ sending: true, error: null }),
  setResponse: (response) => set({ response, sending: false, error: null }),
  setError: (error) => set({ error, sending: false }),
}));
