//! Hook returning a `send()` action that fires the current request through IPC
//! and routes the result into the request store.

import { useCallback } from "react";
import { ipc } from "../../lib/ipc";
import { useRequestStore } from "../../stores/requestStore";
import { useActiveWorkspace } from "../workspaces/hooks";
import { useHistoryInvalidation } from "../history/hooks";

export function useSend() {
  const request = useRequestStore((s) => s.request);
  const sending = useRequestStore((s) => s.sending);
  const requestId = useRequestStore((s) => s.requestId);
  const collectionId = useRequestStore((s) => s.collectionId);
  const title = useRequestStore((s) => s.title);
  const beginSend = useRequestStore((s) => s.beginSend);
  const setSendResponse = useRequestStore((s) => s.setSendResponse);
  const setError = useRequestStore((s) => s.setError);
  const { data: workspace } = useActiveWorkspace();
  const invalidateHistory = useHistoryInvalidation();

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
      });
      setSendResponse(result);
    } catch (e) {
      setError(typeof e === "string" ? e : String(e));
    } finally {
      invalidateHistory(workspace.id);
    }
  }, [request, workspace, collectionId, requestId, title, beginSend, setSendResponse, setError, invalidateHistory]);

  return { send, sending };
}
