//! Hook returning a `send()` action that fires the current request through IPC
//! and routes the result into the request store.

import { useCallback } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { ipc } from "../../lib/ipc";
import { useRequestStore } from "../../stores/requestStore";
import { useActiveWorkspace } from "../workspaces/hooks";
import { useHistoryInvalidation } from "../history/hooks";
import { useActiveEnvironment, variableKeys } from "../environments/hooks";

export function useSend() {
  const request = useRequestStore((s) => s.request);
  const sending = useRequestStore((s) => s.sending);
  const requestId = useRequestStore((s) => s.requestId);
  const collectionId = useRequestStore((s) => s.collectionId);
  const title = useRequestStore((s) => s.title);
  const preRequestScript = useRequestStore((s) => s.preRequestScript);
  const postResponseScript = useRequestStore((s) => s.postResponseScript);
  const beginSend = useRequestStore((s) => s.beginSend);
  const setSendResponse = useRequestStore((s) => s.setSendResponse);
  const setError = useRequestStore((s) => s.setError);
  const { data: workspace } = useActiveWorkspace();
  const { data: activeEnv } = useActiveEnvironment(workspace?.id);
  const invalidateHistory = useHistoryInvalidation();
  const qc = useQueryClient();

  const send = useCallback(async () => {
    if (!request.url.trim()) {
      setError("Enter a URL first.");
      return;
    }
    if (!workspace) {
      setError("No active workspace.");
      return;
    }
    beginSend();
    try {
      const result = await ipc.sendRequest({
        req: request,
        workspaceId: workspace.id,
        collectionId,
        requestId,
        name: title,
        preRequestScript,
        postResponseScript,
      });
      setSendResponse(result);
      const hasEnvChanges = (r: typeof result.preScript) =>
        (r?.envMutations.length ?? 0) > 0 || (r?.envUnsets.length ?? 0) > 0;
      if (hasEnvChanges(result.preScript) || hasEnvChanges(result.postScript)) {
        if (activeEnv) {
          void qc.invalidateQueries({ queryKey: variableKeys.scope({ kind: "environment", id: activeEnv.id }) });
        }
        void qc.invalidateQueries({ queryKey: variableKeys.scope({ kind: "workspace", id: workspace.id }) });
      }
    } catch (e) {
      setError(typeof e === "string" ? e : String(e));
    } finally {
      invalidateHistory(workspace.id);
    }
  }, [
    request,
    workspace,
    collectionId,
    requestId,
    title,
    preRequestScript,
    postResponseScript,
    activeEnv,
    beginSend,
    setSendResponse,
    setError,
    invalidateHistory,
    qc,
  ]);

  return { send, sending };
}
