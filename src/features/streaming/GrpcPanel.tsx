//! Ephemeral gRPC client — the panel that finally wires `GrpcSchemaPicker`
//! (17d-9, schema discovery) and `GrpcMessageBuilder` (17d-10, request
//! building) into the real `ipc.grpcConnect`/`grpcSend`/`grpcFinishSending`
//! commands (17d-8). Same standalone connect/watch/disconnect surface and
//! modal-shell convention as `SsePanel`/`WsPanel`.
//!
//! ## Bridging the picker's mocked shape onto the real `GrpcConnectArgs`
//!
//! `GrpcSchemaPicker.onMethodSelected` only ever hands back a
//! `GrpcMethodDescriptor` (service/method names, streaming type, field
//! descriptors) — it does NOT surface the proto source text or filename it
//! compiled that descriptor from; those are local state trapped inside the
//! picker (`protoContent`/`protoFileName`), and #17d-11's scope is "consume
//! `GrpcSchemaPicker`/`GrpcMessageBuilder` as child components, don't rewrite
//! them." `grpc_connect` (`src-tauri/src/commands/streaming.rs`) has no
//! descriptor-pool cache to reference by id — it compiles a fresh
//! `DescriptorPool` from `protoFiles`/`entryPoint` on every connect — so the
//! ONLY way this panel can connect at all is to also hold its own proto
//! source text, entered in parallel with (and necessarily duplicating) the
//! picker's own proto-upload textarea. This is a deliberate, known
//! duplication forced by the picker's real prop contract (locked by
//! `GrpcSchemaPicker.test.tsx`), not an oversight — see PLAN.md #17d-11.
//!
//! One consequence: a method discovered via the picker's "Reflection" mode
//! cannot be connected to from this panel at all, because reflection yields
//! no proto source and `grpc_connect` has no other way to source a
//! `DescriptorPool` yet (the same discovery-to-connect gap #17d-8 flagged).
//! The Connect button is gated on proto source being non-empty for exactly
//! this reason — it has nothing to do with which discovery mode the picker
//! happened to use.
//!
//! `GrpcMessageBuilder`'s `onSend` callback emits the built request as a JSON
//! *string*; `GrpcConnectArgs.request`/`GrpcOutbound.request` are
//! `serde_json::Value` server-side, so this panel `JSON.parse`s that string
//! before handing it to `ipc.grpcConnect`/`useGrpcConnection.send` — a parse
//! failure (which `GrpcMessageBuilder` itself prevents via its own JSON-mode
//! guard, but defense in depth costs nothing) surfaces as an inline error
//! rather than connecting with garbage.

import { useState } from "react";
import { Network, Save } from "lucide-react";
import { protocolOf } from "../../lib/methods";
import { defaultRequest } from "../../lib/http";
import { defaultRequestAuth, type GrpcStreamConfig, type SavedRequest } from "../../lib/types";
import { useCollections, useCreateRequest, useUpdateRequest } from "../collections/hooks";
import { SaveRequestDialog } from "../request/SaveRequestDialog";
import { GrpcSchemaPicker } from "./GrpcSchemaPicker";
import { GrpcMessageBuilder } from "./GrpcMessageBuilder";
import type { GrpcMethodDescriptor } from "./grpcSchemaTypes";
import { useGrpcConnection, type GrpcLogEntry, type GrpcStatus } from "./grpcHooks";

const DEFAULT_ENTRY_POINT = "main.proto";

function grpcConfigOf(request: SavedRequest | null | undefined): GrpcStreamConfig | null {
  if (!request || request.kind !== "grpc" || !request.streamConfig) return null;
  return request.streamConfig as GrpcStreamConfig;
}

export function GrpcPanel({
  workspaceId,
  savedRequest,
  onClose,
}: {
  workspaceId: string;
  savedRequest?: SavedRequest | null;
  onClose: () => void;
}) {
  const initial = grpcConfigOf(savedRequest);
  const [url, setUrl] = useState(initial?.url ?? "");
  const [method, setMethod] = useState<GrpcMethodDescriptor | null>(null);
  // Panel-owned proto source — see the module doc comment above for why this
  // can't come from the picker. Free-form text + an optional filename used
  // as the descriptor pool's entry point key.
  const [protoSource, setProtoSource] = useState(initial?.protoSource ?? "");
  const [protoFileName, setProtoFileName] = useState(initial?.protoFileName ?? "");
  const [buildError, setBuildError] = useState<string | null>(null);
  const [linkedRequest, setLinkedRequest] = useState<SavedRequest | null>(savedRequest ?? null);
  const [saveDialogOpen, setSaveDialogOpen] = useState(false);
  const { status, log, errorMessage, connect, disconnect, send, finishSending } = useGrpcConnection();
  const { data: collections } = useCollections(workspaceId);
  const createRequest = useCreateRequest(workspaceId);
  const updateRequest = useUpdateRequest(workspaceId);

  // `grpc_connect` only accepts `grpc://`/`grpcs://` (see `parse_target` in
  // `engine::grpc::transport`) — unlike SSE/WS, plain http(s) is never a
  // valid scheme here, even though the transport rides over HTTP/2 under
  // the hood. `isValidUrl` (which uses `new URL()`) rejects "grpc://host"
  // forms with no path in some environments, so the scheme check via
  // `protocolOf` is load-bearing, not just a UX nicety.
  const scheme = protocolOf(url);
  const urlOk = url.trim() !== "" && (scheme === "grpc" || scheme === "grpcs");
  const busy = status === "connecting" || status === "open";
  const protoOk = protoSource.trim() !== "";
  // Pre-connect, GrpcMessageBuilder's own button doubles as the Connect
  // trigger — there's no separate "Connect" button once a method is picked,
  // since the initial request rides in `grpc_connect`'s args (the backend
  // pre-queues it onto the streaming drive loop). See module doc comment.
  const canConnect = !busy && urlOk && method != null && protoOk;
  // Only client-streaming/bidi connections keep a live sender after
  // connect — the backend drops the sender immediately for
  // unary/server-streaming (`grpc_connect` in `commands/streaming.rs`), so
  // there is nothing meaningful to send afterwards for those modes. Derived
  // from the descriptor, not inferred from events — deterministic and
  // available before the first event ever arrives.
  const supportsSend = method?.streamingType === "client-streaming" || method?.streamingType === "bidi";
  const canSend = status === "open" && supportsSend;

  function handleMethodSelected(m: GrpcMethodDescriptor) {
    setMethod(m);
    setBuildError(null);
  }

  function entryPointKey(): string {
    return protoFileName.trim() !== "" ? protoFileName.trim() : DEFAULT_ENTRY_POINT;
  }

  function parseBuilderJson(requestJson: string): unknown | undefined {
    try {
      setBuildError(null);
      return JSON.parse(requestJson);
    } catch {
      setBuildError("Invalid JSON from the message builder — not sent.");
      return undefined;
    }
  }

  function handleBuilderSend(requestJson: string) {
    const parsed = parseBuilderJson(requestJson);
    if (parsed === undefined) return;

    if (status !== "open") {
      // Pre-connect: the builder's button is the Connect trigger.
      if (!method) return;
      const entryPoint = entryPointKey();
      void connect(workspaceId, {
        url,
        methodFullName: method.fullName,
        request: parsed,
        protoFiles: { [entryPoint]: protoSource },
        entryPoint,
      });
    } else {
      // Post-connect, client-streaming/bidi: send another request message.
      void send(parsed);
    }
  }

  function handleDisconnect() {
    disconnect();
  }

  function streamConfig(): GrpcStreamConfig {
    return { url, methodFullName: method?.fullName ?? null, protoSource, protoFileName };
  }

  async function handleSave(name: string, collectionId: string) {
    const saved = await createRequest.mutateAsync({
      collectionId,
      input: {
        name,
        ...defaultRequest(),
        method: "GRPC",
        auth: defaultRequestAuth(),
        preRequestScript: "",
        postResponseScript: "",
        kind: "grpc",
        streamConfig: streamConfig(),
      },
    });
    setLinkedRequest(saved);
    setSaveDialogOpen(false);
  }

  function handleUpdate() {
    if (!linkedRequest) return;
    updateRequest.mutate({
      id: linkedRequest.id,
      input: {
        name: linkedRequest.name,
        method: linkedRequest.method,
        url: linkedRequest.url,
        headers: linkedRequest.headers,
        query: linkedRequest.query,
        body: linkedRequest.body,
        options: linkedRequest.options,
        auth: linkedRequest.auth,
        preRequestScript: linkedRequest.preRequestScript,
        postResponseScript: linkedRequest.postResponseScript,
        kind: "grpc",
        streamConfig: streamConfig(),
      },
    });
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/30" onClick={onClose}>
      <div
        onClick={(e) => e.stopPropagation()}
        className="flex max-h-[85vh] w-[42rem] flex-col rounded-lg border border-slate-200 bg-white p-4 shadow-xl dark:border-slate-700 dark:bg-slate-800"
      >
        <div className="mb-3 flex items-center justify-between">
          <h2 className="flex items-center gap-1.5 text-sm font-semibold text-slate-800 dark:text-slate-100">
            <Network size={14} /> gRPC{linkedRequest && ` — ${linkedRequest.name}`}
          </h2>
          <div className="flex items-center gap-2">
            <button
              type="button"
              title={linkedRequest ? "Save changes to this saved request" : "Save to a collection"}
              onClick={() => (linkedRequest ? handleUpdate() : setSaveDialogOpen(true))}
              className="flex items-center gap-1 rounded-md px-2 py-1 text-xs text-slate-500 hover:bg-slate-100 dark:hover:bg-slate-700"
            >
              <Save size={12} /> Save
            </button>
            <StatusBadge status={status} />
          </div>
        </div>

        <div className="flex items-center gap-2">
          <input
            value={url}
            onChange={(e) => setUrl(e.target.value)}
            disabled={busy}
            placeholder="grpc://localhost:50051"
            spellCheck={false}
            className="flex-1 rounded-md border border-slate-200 bg-transparent px-2.5 py-1.5 text-sm focus:outline-none focus:ring-2 focus:ring-accent/40 disabled:opacity-60 dark:border-slate-700"
          />
          {busy && (
            <button
              type="button"
              onClick={handleDisconnect}
              className="rounded-md bg-red-500 px-3 py-1.5 text-sm font-medium text-white hover:bg-red-600"
            >
              Disconnect
            </button>
          )}
        </div>

        <details className="mt-2 text-xs" open={method == null}>
          <summary className="cursor-pointer text-slate-500 dark:text-slate-400">
            Schema {method && `— ${method.fullName}`}
          </summary>
          <div className="mt-2">
            <GrpcSchemaPicker onMethodSelected={handleMethodSelected} selectedMethodFullName={method?.fullName} />
          </div>
        </details>

        {/* Panel-owned proto source — see module doc comment for why this
            duplicates the picker's own proto-upload textarea instead of
            reusing its state. Required for Connect regardless of which
            discovery mode the picker used above. */}
        <details className="mt-2 text-xs">
          <summary className="cursor-pointer text-slate-500 dark:text-slate-400">
            .proto source for connect {protoOk && "(set)"}
          </summary>
          <div className="mt-2 flex flex-col gap-2">
            <p className="text-[11px] text-slate-400">
              Connect compiles a descriptor pool from inline .proto source — there is no
              reflection-to-connect handoff yet, so paste the same source here even if you
              discovered this method via reflection above.
            </p>
            <textarea
              value={protoSource}
              onChange={(e) => setProtoSource(e.target.value)}
              disabled={busy}
              rows={4}
              placeholder="Paste the .proto source to connect with…"
              spellCheck={false}
              className="resize-none rounded-md border border-slate-200 bg-transparent px-2.5 py-1.5 font-mono text-xs focus:outline-none focus:ring-2 focus:ring-accent/40 disabled:opacity-60 dark:border-slate-700"
            />
            <input
              value={protoFileName}
              onChange={(e) => setProtoFileName(e.target.value)}
              disabled={busy}
              placeholder={`Entry point filename (defaults to "${DEFAULT_ENTRY_POINT}")`}
              spellCheck={false}
              className="rounded-md border border-slate-200 bg-transparent px-2.5 py-1.5 text-xs focus:outline-none focus:ring-2 focus:ring-accent/40 disabled:opacity-60 dark:border-slate-700"
            />
          </div>
        </details>

        {(errorMessage || buildError) && (
          <p className="mt-2 rounded-md bg-red-50 px-2 py-1 text-xs text-red-600 dark:bg-red-900/30 dark:text-red-400">
            {errorMessage ?? buildError}
          </p>
        )}

        <div className="mt-3 min-h-0 flex-1 overflow-auto rounded-md border border-slate-100 dark:border-slate-700">
          {log.length === 0 ? (
            <div className="flex flex-col items-center justify-center gap-1 p-6 text-center text-sm text-slate-400">
              <p>No events yet.</p>
              <p className="text-xs">Pick a method, paste its .proto source, and connect.</p>
            </div>
          ) : (
            log.map((entry) => <LogRow key={entry.id} entry={entry} />)
          )}
        </div>

        {method && (
          <div className="mt-3">
            <GrpcMessageBuilder
              key={method.fullName}
              method={method}
              onSend={handleBuilderSend}
              sendDisabled={status === "open" ? !canSend : !canConnect}
              sendLabel={status === "open" ? "Send" : "Connect"}
            />
            {canSend && (
              <div className="mt-2 flex justify-end">
                <button
                  type="button"
                  onClick={() => void finishSending()}
                  className="rounded-md border border-slate-200 px-3 py-1.5 text-xs font-medium text-slate-600 hover:bg-slate-100 dark:border-slate-700 dark:text-slate-300 dark:hover:bg-slate-700"
                >
                  Finish sending
                </button>
              </div>
            )}
          </div>
        )}

        <div className="mt-3 flex justify-end text-sm">
          <button
            type="button"
            onClick={onClose}
            className="rounded-md px-3 py-1.5 text-slate-500 hover:bg-slate-100 dark:hover:bg-slate-700"
          >
            Close
          </button>
        </div>
      </div>

      {saveDialogOpen && (
        <div onClick={(e) => e.stopPropagation()}>
          <SaveRequestDialog
            defaultName="gRPC request"
            collections={collections ?? []}
            saving={createRequest.isPending}
            onSave={(name, collectionId) => void handleSave(name, collectionId)}
            onClose={() => setSaveDialogOpen(false)}
          />
        </div>
      )}
    </div>
  );
}

function StatusBadge({ status }: { status: GrpcStatus }) {
  const labels: Record<GrpcStatus, string> = {
    idle: "Idle",
    connecting: "Connecting…",
    open: "Open",
    closed: "Closed",
    error: "Error",
  };
  const classes: Record<GrpcStatus, string> = {
    idle: "text-slate-400",
    connecting: "text-amber-500",
    open: "text-green-600 dark:text-green-400",
    closed: "text-slate-400",
    error: "text-red-500",
  };
  return <span className={"flex items-center gap-1 text-xs font-medium " + classes[status]}>{labels[status]}</span>;
}

function statusCodeLabel(code: number): string {
  return code === 0 ? "OK" : String(code);
}

function LogRow({ entry }: { entry: GrpcLogEntry }) {
  const time = new Date(entry.receivedAt).toLocaleTimeString();
  const { event, direction } = entry;

  if (event.type === "open") return <SystemRow time={time} text="Connection opened" />;
  if (event.type === "closed") return <SystemRow time={time} text="Connection closed" />;
  if (event.type === "error") return <SystemRow time={time} text={`Error: ${event.message}`} isError />;
  if (event.type === "status") {
    const detail = event.message ? `: ${event.message}` : "";
    return (
      <SystemRow
        time={time}
        text={`Status ${statusCodeLabel(event.code)}${detail}`}
        isError={event.code !== 0}
      />
    );
  }

  // "response" — fires once for unary/client-streaming, once per message
  // for server-streaming/bidi. Outbound sends are echoed as the same
  // "response" shape tagged with direction "out" (see `useGrpcConnection`).
  const sent = direction === "out";
  return (
    <div className="flex items-start gap-2 border-b border-slate-100 px-2 py-1.5 text-xs last:border-0 dark:border-slate-800">
      <span className="shrink-0 text-slate-400">{time}</span>
      <span className={"mt-0.5 shrink-0 " + (sent ? "text-accent" : "text-green-500")}>{sent ? "↑" : "↓"}</span>
      <pre className="min-w-0 flex-1 whitespace-pre-wrap break-all font-mono text-slate-700 dark:text-slate-300">
        {JSON.stringify(event.message, null, 2)}
      </pre>
    </div>
  );
}

function SystemRow({ time, text, isError }: { time: string; text: string; isError?: boolean }) {
  return (
    <div className="flex items-center gap-2 border-b border-slate-100 px-2 py-1.5 text-xs last:border-0 dark:border-slate-800">
      <span className="shrink-0 text-slate-400">{time}</span>
      <span className={isError ? "text-red-500" : "text-slate-400 italic"}>{text}</span>
    </div>
  );
}
