//! gRPC schema picker — embedded (non-modal) panel for choosing a gRPC schema
//! source (server reflection OR `.proto` upload), running discovery, and
//! selecting a method. Purely presentational + local state; no TanStack
//! Query, no real `invoke`. Modal-shell class strings mirror `WsPanel` so
//! it drops into the same chrome when hosted inside one.

import { useRef, useState } from "react";
import { Loader2, Upload } from "lucide-react";
import type {
  GrpcMethodDescriptor,
  GrpcSchema,
  GrpcSchemaSource,
} from "./grpcSchemaTypes";
import { discoverGrpcSchema } from "./grpcSchemaIpc";

interface GrpcSchemaPickerProps {
  onMethodSelected: (method: GrpcMethodDescriptor) => void;
  selectedMethodFullName?: string;
  onClose?: () => void;
}

const STREAMING_LABELS: Record<GrpcMethodDescriptor["streamingType"], string> = {
  unary: "unary",
  "server-streaming": "server-stream",
  "client-streaming": "client-stream",
  bidi: "bidi",
};

const STREAMING_CLASSES: Record<GrpcMethodDescriptor["streamingType"], string> = {
  unary: "bg-slate-100 text-slate-600 dark:bg-slate-700 dark:text-slate-300",
  "server-streaming": "bg-green-100 text-green-700 dark:bg-green-900/40 dark:text-green-300",
  "client-streaming": "bg-accent/20 text-accent",
  bidi: "bg-amber-100 text-amber-700 dark:bg-amber-900/40 dark:text-amber-300",
};

export function GrpcSchemaPicker({
  onMethodSelected,
  selectedMethodFullName,
  onClose,
}: GrpcSchemaPickerProps) {
  const [mode, setMode] = useState<GrpcSchemaSource>("reflection");
  const [target, setTarget] = useState("");
  const [protoContent, setProtoContent] = useState("");
  const [protoFileName, setProtoFileName] = useState("");
  const [schema, setSchema] = useState<GrpcSchema | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);

  const reflectionDisabled = loading || target.trim() === "";
  const protoDisabled = loading || protoContent.trim() === "";
  const busyLabel = mode === "reflection" ? "Discovering…" : "Compiling…";

  async function runDiscovery() {
    setLoading(true);
    setError(null);
    try {
      const result = await discoverGrpcSchema({
        mode,
        target: mode === "reflection" ? target : undefined,
        protoContent: mode === "proto-upload" ? protoContent : undefined,
        protoFileName: mode === "proto-upload" ? protoFileName : undefined,
      });
      setSchema(result);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }

  function handleFileChange(e: React.ChangeEvent<HTMLInputElement>) {
    const file = e.target.files?.[0];
    if (!file) return;
    if (!protoFileName) setProtoFileName(file.name);
    const reader = new FileReader();
    reader.onload = () => {
      setProtoContent(typeof reader.result === "string" ? reader.result : "");
    };
    reader.readAsText(file);
  }

  return (
    <div className="flex max-h-[85vh] w-[36rem] flex-col rounded-lg border border-slate-200 bg-white p-4 shadow-xl dark:border-slate-700 dark:bg-slate-800">
      <div className="mb-3 flex items-center justify-between">
        <h2 className="flex items-center gap-1.5 text-sm font-semibold text-slate-800 dark:text-slate-100">
          gRPC Schema
        </h2>
        <div className="flex items-center gap-1 rounded-md border border-slate-200 p-0.5 text-xs dark:border-slate-700">
          <ModeButton active={mode === "reflection"} onClick={() => setMode("reflection")}>
            Reflection
          </ModeButton>
          <ModeButton active={mode === "proto-upload"} onClick={() => setMode("proto-upload")}>
            .proto Upload
          </ModeButton>
        </div>
      </div>

      {mode === "reflection" ? (
        <div className="flex items-center gap-2">
          <input
            value={target}
            onChange={(e) => setTarget(e.target.value)}
            disabled={loading}
            placeholder="localhost:50051"
            spellCheck={false}
            className="flex-1 rounded-md border border-slate-200 bg-transparent px-2.5 py-1.5 text-sm focus:outline-none focus:ring-2 focus:ring-accent/40 disabled:opacity-60 dark:border-slate-700"
          />
          <button
            type="button"
            disabled={reflectionDisabled}
            onClick={() => void runDiscovery()}
            className="rounded-md bg-accent px-3 py-1.5 text-sm font-medium text-white disabled:opacity-40"
          >
            Discover
          </button>
        </div>
      ) : (
        <div className="flex flex-col gap-2">
          <textarea
            value={protoContent}
            onChange={(e) => setProtoContent(e.target.value)}
            disabled={loading}
            rows={5}
            placeholder={'syntax = "proto3"; package example; service Greeter { ... }'}
            spellCheck={false}
            className="resize-none rounded-md border border-slate-200 bg-transparent px-2.5 py-1.5 font-mono text-xs focus:outline-none focus:ring-2 focus:ring-accent/40 disabled:opacity-60 dark:border-slate-700"
          />
          <div className="flex items-center gap-2">
            <input
              value={protoFileName}
              onChange={(e) => setProtoFileName(e.target.value)}
              disabled={loading}
              placeholder="greeter.proto (optional filename)"
              spellCheck={false}
              className="flex-1 rounded-md border border-slate-200 bg-transparent px-2.5 py-1.5 text-xs focus:outline-none focus:ring-2 focus:ring-accent/40 disabled:opacity-60 dark:border-slate-700"
            />
            <label
              className="flex items-center gap-1 rounded-md border border-slate-200 px-2.5 py-1.5 text-xs text-slate-600 hover:bg-slate-100 dark:border-slate-700 dark:text-slate-300 dark:hover:bg-slate-700"
              title="Read a .proto file from disk"
            >
              <Upload size={12} />
              file
              <input
                ref={fileInputRef}
                type="file"
                accept=".proto,text/plain"
                onChange={handleFileChange}
                className="hidden"
              />
            </label>
            <button
              type="button"
              disabled={protoDisabled}
              onClick={() => void runDiscovery()}
              className="rounded-md bg-accent px-3 py-1.5 text-sm font-medium text-white disabled:opacity-40"
            >
              Compile
            </button>
          </div>
        </div>
      )}

      {loading && (
        <p className="mt-2 flex items-center gap-1 text-xs text-amber-500">
          <Loader2 size={11} className="animate-spin" />
          {busyLabel}
        </p>
      )}

      {error && (
        <p className="mt-2 rounded-md bg-red-50 px-2 py-1 text-xs text-red-600 dark:bg-red-900/30 dark:text-red-400">
          {error}
        </p>
      )}

      <div className="mt-3 min-h-0 flex-1 overflow-auto rounded-md border border-slate-100 dark:border-slate-700">
        {schema == null && !loading && (
          <div className="flex flex-col items-center justify-center gap-1 p-6 text-center text-sm text-slate-400">
            <p>No schema discovered yet.</p>
            <p className="text-xs">Run discovery to browse services and methods here.</p>
          </div>
        )}
        {schema && (
          <div>
            {schema.services.length === 0 && (
              <p className="p-4 text-center text-xs text-slate-400">
                Discovery returned no services.
              </p>
            )}
            {schema.services.map((service) => (
              <div key={service.name}>
                <div className="border-b border-slate-100 bg-slate-50 px-2 py-1.5 text-xs font-semibold text-slate-700 dark:border-slate-800 dark:bg-slate-900/40 dark:text-slate-200">
                  {service.name}
                </div>
                {service.methods.map((method) => {
                  const selected = method.fullName === selectedMethodFullName;
                  return (
                    <button
                      key={method.fullName}
                      type="button"
                      onClick={() => onMethodSelected(method)}
                      className={
                        "flex w-full items-center gap-2 border-b border-slate-100 px-2 py-1.5 text-left text-xs last:border-0 hover:bg-slate-50 dark:border-slate-800 dark:hover:bg-slate-900/40 " +
                        (selected
                          ? "bg-accent/10 ring-1 ring-inset ring-accent/40"
                          : "")
                      }
                    >
                      <span className="font-mono text-slate-700 dark:text-slate-300">
                        {method.fullName}
                      </span>
                      <span
                        className={
                          "ml-auto rounded px-1 py-0.5 text-[10px] " +
                          STREAMING_CLASSES[method.streamingType]
                        }
                      >
                        {STREAMING_LABELS[method.streamingType]}
                      </span>
                    </button>
                  );
                })}
              </div>
            ))}
          </div>
        )}
      </div>

      {onClose && (
        <div className="mt-3 flex justify-end text-sm">
          <button
            type="button"
            onClick={onClose}
            className="rounded-md px-3 py-1.5 text-slate-500 hover:bg-slate-100 dark:hover:bg-slate-700"
          >
            Close
          </button>
        </div>
      )}
    </div>
  );
}

function ModeButton({
  active,
  onClick,
  children,
}: {
  active: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={
        "rounded px-2 py-1 " +
        (active
          ? "bg-accent text-white"
          : "text-slate-600 hover:bg-slate-100 dark:text-slate-300 dark:hover:bg-slate-700")
      }
    >
      {children}
    </button>
  );
}