//! WebSocket connection state. Like `useSseConnection`, this is an imperative
//! push subscription, not a TanStack Query resource — but bidirectional, so it
//! adds a `send` path and tags each transcript entry's direction (received vs
//! sent). Disconnects on unmount so closing the panel can't leak a live
//! backend connection.

import { useCallback, useEffect, useRef, useState } from "react";
import { ipc } from "../../lib/ipc";
import type { HeaderEntry } from "../../lib/http";
import type { WsEvent, WsOutbound } from "../../lib/types";

export type WsDirection = "in" | "out";

export interface WsLogEntry {
  id: number;
  receivedAt: number;
  direction: WsDirection;
  event: WsEvent;
}

export type WsStatus = "idle" | "connecting" | "open" | "closed" | "error";

export function useWsConnection() {
  const [status, setStatus] = useState<WsStatus>("idle");
  const [log, setLog] = useState<WsLogEntry[]>([]);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const connectionIdRef = useRef<string | null>(null);
  const nextLogId = useRef(0);

  const append = useCallback((direction: WsDirection, event: WsEvent) => {
    setLog((prev) => [...prev, { id: nextLogId.current++, receivedAt: Date.now(), direction, event }]);
  }, []);

  const disconnect = useCallback(() => {
    if (connectionIdRef.current) {
      void ipc.streamDisconnect(connectionIdRef.current);
      connectionIdRef.current = null;
    }
    setStatus("closed");
  }, []);

  // Reads the ref at unmount time, not a stale closure value.
  useEffect(() => {
    return () => {
      if (connectionIdRef.current) void ipc.streamDisconnect(connectionIdRef.current);
    };
  }, []);

  const connect = useCallback(
    async (workspaceId: string, url: string, headers: HeaderEntry[]) => {
      if (connectionIdRef.current) {
        void ipc.streamDisconnect(connectionIdRef.current);
        connectionIdRef.current = null;
      }
      setStatus("connecting");
      setErrorMessage(null);
      setLog([]);
      try {
        const connectionId = await ipc.wsConnect(workspaceId, url, headers, (event) => {
          append("in", event);
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
    },
    [append],
  );

  const send = useCallback(
    async (message: WsOutbound) => {
      if (!connectionIdRef.current) return;
      try {
        await ipc.wsSend(connectionIdRef.current, message);
        // Echo the sent frame into the transcript as an outbound message.
        append("out", { type: "message", binary: message.binary, data: message.data });
      } catch (e) {
        setErrorMessage(e instanceof Error ? e.message : String(e));
      }
    },
    [append],
  );

  return { status, log, errorMessage, connect, disconnect, send };
}
