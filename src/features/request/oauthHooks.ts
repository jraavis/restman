//! OAuth2 connection status + the browser-flow "Connect" action, shared by
//! the request-level and collection-level Auth editors (both just pass
//! whichever id is in scope — see `ipc.startOAuth2Authorization`'s doc comment).

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ipc } from "../../lib/ipc";

type OAuth2Scope = { collectionId?: string | null; requestId?: string | null };

export const oauth2Keys = {
  status: (scope: OAuth2Scope) => ["oauth2-status", scope.collectionId ?? null, scope.requestId ?? null] as const,
};

export function useOAuth2Status(scope: OAuth2Scope) {
  return useQuery({
    queryKey: oauth2Keys.status(scope),
    queryFn: () => ipc.getOAuth2Status(scope.collectionId, scope.requestId),
    enabled: !!(scope.collectionId || scope.requestId),
  });
}

export function useStartOAuth2Authorization(scope: OAuth2Scope) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: () => ipc.startOAuth2Authorization(scope.collectionId, scope.requestId),
    onSuccess: () => qc.invalidateQueries({ queryKey: oauth2Keys.status(scope) }),
  });
}
