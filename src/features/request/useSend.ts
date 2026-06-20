//! Hook returning a `send()` action that fires the current request through IPC
//! and routes the result into the request store.

import { useCallback } from "react";
import { ipc } from "../../lib/ipc";
import { useRequestStore } from "../../stores/requestStore";

export function useSend() {
  const request = useRequestStore((s) => s.request);
  const sending = useRequestStore((s) => s.sending);
  const beginSend = useRequestStore((s) => s.beginSend);
  const setResponse = useRequestStore((s) => s.setResponse);
  const setError = useRequestStore((s) => s.setError);

  const send = useCallback(async () => {
    if (!request.url.trim()) {
      setError("Enter a URL first.");
      return;
    }
    beginSend();
    try {
      setResponse(await ipc.sendRequest(request));
    } catch (e) {
      setError(typeof e === "string" ? e : String(e));
    }
  }, [request, beginSend, setResponse, setError]);

  return { send, sending };
}
