//! TanStack Query hooks for the shared cookie jar, backed by Tauri IPC.
//! The jar is app-global (not per-workspace), so there's a single query key.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ipc } from "../../lib/ipc";

export const cookieKeys = {
  all: ["cookies"] as const,
};

export function useCookies() {
  return useQuery({ queryKey: cookieKeys.all, queryFn: ipc.listCookies });
}

export function useDeleteCookie() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ domain, path, name }: { domain: string; path: string; name: string }) =>
      ipc.deleteCookie(domain, path, name),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: cookieKeys.all });
    },
  });
}

export function useClearCookies() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: () => ipc.clearCookies(),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: cookieKeys.all });
    },
  });
}
