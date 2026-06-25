//! SSE connection state. Deliberately not a TanStack Query resource — this is
//! a live subscription with push events, not cacheable request/response data.

import { useCallback, useEffect, useRef, useState } from "react";
import { ipc } from "../../lib/ipc";
import type { HeaderEntry } from "../../lib/http";
import type { SseEvent } from "../../lib/types";

export interface SseLogEntry {
  id: number;
  receivedAt: number;
  event: SseEvent;
}

export type SseStatus = "idle" | "connecting" | "open" | "closed" | "error";

export function useSseConnection() {
  const [status, setStatus] = useState<SseStatus>("idle");
  const [log, setLog] = useState<SseLogEntry[]>([]);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const connectionIdRef = useRef<string | null>(null);
  const nextLogId = useRef(0);

  const disconnect = useCallback(() => {
    if (connectionIdRef.current) {
      void ipc.sseDisconnect(connectionIdRef.current);
      connectionIdRef.current = null;
    }
    setStatus("closed");
  }, []);

  // Disconnect on unmount so closing the panel doesn't leak a live backend
  // connection. Reads the ref at unmount time, not a stale closure value.
  useEffect(() => {
    return () => {
      if (connectionIdRef.current) void ipc.sseDisconnect(connectionIdRef.current);
    };
  }, []);

  const connect = useCallback(async (workspaceId: string, url: string, headers: HeaderEntry[]) => {
    if (connectionIdRef.current) {
      void ipc.sseDisconnect(connectionIdRef.current);
      connectionIdRef.current = null;
    }
    setStatus("connecting");
    setErrorMessage(null);
    setLog([]);
    try {
      const connectionId = await ipc.sseConnect(workspaceId, url, headers, (event) => {
        setLog((prev) => [...prev, { id: nextLogId.current++, receivedAt: Date.now(), event }]);
        if (event.type === "open") setStatus("open");
        else if (event.type === "error") {
          setStatus("error");
          setErrorMessage(event.message);
        } else if (event.type === "closed") setStatus("closed");
      });
      connectionIdRef.current = connectionId;
    } catch (e) {
      setStatus("error");
      setErrorMessage(e instanceof Error ? e.message : String(e));
    }
  }, []);

  return { status, log, errorMessage, connect, disconnect };
}
