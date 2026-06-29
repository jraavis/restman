//! gRPC connection state. Like `useWsConnection`, this is an imperative
//! push subscription (not a TanStack Query resource) with a `send` path —
//! but gRPC's terminal sequence is richer than WS's: a dedicated `Status`
//! event (the actual `grpc-status`/`grpc-message` verdict) always precedes
//! `Closed` on every path except `Error`, which can fire with no preceding
//! `Open` at all (a transport-level send failure before the stream opens).
//! Disconnects on unmount so closing the panel can't leak a live backend
//! connection, same contract as `useSseConnection`/`useWsConnection`.

import { useCallback, useEffect, useRef, useState } from "react";
import { ipc } from "../../lib/ipc";
import type { GrpcConnectArgs, GrpcEvent } from "../../lib/types";

export type GrpcDirection = "in" | "out";

export interface GrpcLogEntry {
  id: number;
  receivedAt: number;
  direction: GrpcDirection;
  event: GrpcEvent;
}

export type GrpcStatus = "idle" | "connecting" | "open" | "closed" | "error";

export function useGrpcConnection() {
  const [status, setStatus] = useState<GrpcStatus>("idle");
  const [log, setLog] = useState<GrpcLogEntry[]>([]);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const connectionIdRef = useRef<string | null>(null);
  const nextLogId = useRef(0);

  const append = useCallback((direction: GrpcDirection, event: GrpcEvent) => {
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
    async (workspaceId: string, args: GrpcConnectArgs) => {
      if (connectionIdRef.current) {
        void ipc.streamDisconnect(connectionIdRef.current);
        connectionIdRef.current = null;
      }
      setStatus("connecting");
      setErrorMessage(null);
      setLog([]);
      try {
        const connectionId = await ipc.grpcConnect(workspaceId, args, (event) => {
          append("in", event);
          if (event.type === "open") setStatus("open");
          else if (event.type === "error") {
            setStatus("error");
            setErrorMessage(event.message);
          } else if (event.type === "closed") {
            // `closed` is the channel's true terminal marker; don't downgrade
            // an already-set "error" status if `Error` (no preceding `Open`)
            // already fired — both Error and Closed can appear on the same
            // channel for a transport-level send failure.
            setStatus((prev) => (prev === "error" ? prev : "closed"));
          }
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
    async (requestValue: unknown) => {
      if (!connectionIdRef.current) return;
      try {
        await ipc.grpcSend(connectionIdRef.current, { request: requestValue });
        append("out", { type: "response", message: requestValue });
      } catch (e) {
        setErrorMessage(e instanceof Error ? e.message : String(e));
      }
    },
    [append],
  );

  const finishSending = useCallback(async () => {
    if (!connectionIdRef.current) return;
    try {
      await ipc.grpcFinishSending(connectionIdRef.current);
    } catch (e) {
      setErrorMessage(e instanceof Error ? e.message : String(e));
    }
  }, []);

  return { status, log, errorMessage, connect, disconnect, send, finishSending };
}
