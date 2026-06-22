//! The request editor: method + URL + send, over Params/Headers/Body/Options.

import { useEffect, useRef, useState, type ReactNode } from "react";
import { ChevronDown, Save, Send } from "lucide-react";
import { COMMON_HEADERS, type HeaderEntry } from "../../lib/http";
import { HTTP_METHODS, isValidUrl, methodBadgeClasses, protocolOf } from "../../lib/methods";
import { Switch } from "../../components/Switch";
import { VariableSuggestInput } from "../../components/VariableSuggestInput";
import { useRequestStore } from "../../stores/requestStore";
import { useCollections } from "../collections/hooks";
import { useActiveWorkspace } from "../workspaces/hooks";
import { useResolvedVariableKeys } from "../environments/hooks";
import { BodyEditor } from "./BodyEditor";
import { KeyValueEditor, type Pair } from "./KeyValueEditor";
import { RequestAuthTab } from "./RequestAuthTab";
import { SaveRequestDialog } from "./SaveRequestDialog";
import { useSaveRequest } from "./useSaveRequest";
import { useSend } from "./useSend";

type Tab = "params" | "headers" | "body" | "auth" | "options";

export function RequestBuilder() {
  const request = useRequestStore((s) => s.request);
  const title = useRequestStore((s) => s.title);
  const setTitle = useRequestStore((s) => s.setTitle);
  const setMethod = useRequestStore((s) => s.setMethod);
  const setUrl = useRequestStore((s) => s.setUrl);
  const setQuery = useRequestStore((s) => s.setQuery);
  const setHeaders = useRequestStore((s) => s.setHeaders);
  const setBody = useRequestStore((s) => s.setBody);
  const setOptions = useRequestStore((s) => s.setOptions);
  const auth = useRequestStore((s) => s.auth);
  const setAuth = useRequestStore((s) => s.setAuth);
  const { send, sending } = useSend();

  const { data: workspace } = useActiveWorkspace();
  const workspaceId = workspace?.id;
  const { data: collections } = useCollections(workspaceId);
  const { save, isLinked, saving } = useSaveRequest(workspaceId);
  const [showSaveDialog, setShowSaveDialog] = useState(false);
  const collectionId = useRequestStore((s) => s.collectionId);
  const requestId = useRequestStore((s) => s.requestId);
  const variableKeys = useResolvedVariableKeys(workspaceId, collectionId);

  // Cmd/Ctrl+S: overwrite directly if already linked, otherwise prompt for a
  // name + collection. Mounted once; reads the latest save/isLinked through
  // refs so the listener doesn't re-attach on every keystroke in the builder.
  const isLinkedRef = useRef(isLinked);
  isLinkedRef.current = isLinked;
  const saveRef = useRef(save);
  saveRef.current = save;

  useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      if (!(e.metaKey || e.ctrlKey) || e.key.toLowerCase() !== "s") return;
      e.preventDefault();
      if (isLinkedRef.current) void saveRef.current();
      else setShowSaveDialog(true);
    }
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, []);

  const [tab, setTab] = useState<Tab>("params");

  const protocol = protocolOf(request.url);
  const urlOk = request.url.trim() === "" || isValidUrl(request.url);

  const headerRows: Pair[] = request.headers.map((h) => ({
    key: h.name,
    value: h.value,
    enabled: h.enabled,
  }));
  const setHeaderRows = (rows: Pair[]) =>
    setHeaders(
      rows.map<HeaderEntry>((r) => ({ name: r.key, value: r.value, enabled: r.enabled })),
    );

  return (
    <section className="flex h-full flex-col bg-white dark:bg-slate-900">
      <div className="flex items-center gap-2 p-3">
        <div className="relative shrink-0">
          <select
            value={request.method}
            onChange={(e) => setMethod(e.target.value)}
            className={
              "appearance-none rounded-lg border bg-transparent py-1.5 pl-3 pr-7 text-sm font-bold focus:outline-none focus:ring-2 focus:ring-accent/40 " +
              methodBadgeClasses(request.method)
            }
          >
            {HTTP_METHODS.map((m) => (
              <option key={m} value={m} className="text-slate-900">
                {m}
              </option>
            ))}
          </select>
          <ChevronDown
            size={13}
            className="pointer-events-none absolute right-2 top-1/2 -translate-y-1/2 opacity-60"
          />
        </div>

        <VariableSuggestInput
          value={request.url}
          onChange={setUrl}
          onKeyDown={(e) => {
            if (e.key === "Enter" && (e.metaKey || e.ctrlKey || !e.shiftKey)) {
              e.preventDefault();
              void send();
            }
          }}
          variableKeys={variableKeys}
          placeholder="https://api.example.com/v1/users"
          spellCheck={false}
          className={
            "rounded-lg border bg-slate-50 px-3 py-1.5 text-sm focus:bg-white focus:outline-none focus:ring-2 focus:ring-accent/40 dark:bg-slate-800 dark:focus:bg-slate-800 " +
            (urlOk
              ? "border-slate-200 dark:border-slate-700"
              : "border-red-400 dark:border-red-500")
          }
        />

        <button
          type="button"
          disabled={saving}
          title={isLinked ? "Save (Cmd+S)" : "Save as… (Cmd+S)"}
          onClick={() => (isLinked ? void save() : setShowSaveDialog(true))}
          className="flex shrink-0 items-center gap-1.5 rounded-lg border border-slate-200 px-3 py-1.5 text-sm font-medium text-slate-600 hover:bg-slate-100 disabled:opacity-50 dark:border-slate-700 dark:text-slate-300 dark:hover:bg-slate-800"
        >
          <Save size={14} />
        </button>

        <button
          type="button"
          disabled={sending}
          onClick={() => void send()}
          className="flex shrink-0 items-center gap-1.5 rounded-lg bg-accent px-4 py-1.5 text-sm font-medium text-white hover:bg-accent-hover disabled:opacity-50"
        >
          <Send size={14} />
          {sending ? "Sending…" : "Send"}
        </button>
      </div>

      {request.url.trim() !== "" && (
        <div className="-mt-1 px-3 pb-1 text-xs text-slate-400">
          {protocol ? `${protocol.toUpperCase()} · ` : ""}
          {urlOk ? "valid" : "invalid URL"}
        </div>
      )}

      <nav className="flex gap-1 border-t border-b border-slate-100 px-3 dark:border-slate-800">
        {(["params", "headers", "body", "auth", "options"] as Tab[]).map((t) => (
          <button
            key={t}
            type="button"
            onClick={() => setTab(t)}
            className={
              "flex items-center gap-1.5 px-3 py-2 text-xs font-medium capitalize transition-colors " +
              (tab === t
                ? "border-b-2 border-accent text-accent"
                : "border-b-2 border-transparent text-slate-500 hover:text-slate-800 dark:hover:text-slate-200")
            }
          >
            {t}
            {t === "params" && request.query.length > 0 && (
              <Badge active={tab === t}>{request.query.length}</Badge>
            )}
            {t === "headers" && request.headers.length > 0 && (
              <Badge active={tab === t}>{request.headers.length}</Badge>
            )}
          </button>
        ))}
      </nav>

      <div className="min-h-0 flex-1 overflow-auto p-3">
        {tab === "params" && (
          <KeyValueEditor
            rows={request.query}
            onChange={setQuery}
            keyPlaceholder="Parameter"
            variableKeys={variableKeys}
          />
        )}
        {tab === "headers" && (
          <KeyValueEditor
            rows={headerRows}
            onChange={setHeaderRows}
            keyPlaceholder="Header"
            keySuggestions={COMMON_HEADERS}
            variableKeys={variableKeys}
          />
        )}
        {tab === "body" && <BodyEditor body={request.body} onChange={setBody} variableKeys={variableKeys} />}
        {tab === "auth" && (
          <RequestAuthTab auth={auth} onChange={setAuth} collectionId={collectionId} requestId={requestId} />
        )}
        {tab === "options" && (
          <div className="flex flex-col gap-4 text-sm">
            <label className="flex items-center gap-2">
              <span className="w-40 text-slate-500 dark:text-slate-400">Timeout (seconds)</span>
              <input
                type="number"
                min={1}
                max={300}
                value={request.options.timeoutSecs}
                onChange={(e) => setOptions({ timeoutSecs: Number(e.target.value) })}
                className="w-24 rounded-lg border border-slate-200 bg-transparent px-2 py-1 focus:outline-none focus:ring-2 focus:ring-accent/40 dark:border-slate-700"
              />
            </label>
            <Switch
              checked={request.options.followRedirects}
              onChange={(v) => setOptions({ followRedirects: v })}
              label="Follow redirects"
            />
            <Switch
              checked={request.options.verifySsl}
              onChange={(v) => setOptions({ verifySsl: v })}
              label="Verify SSL certificate"
            />
          </div>
        )}
      </div>

      {showSaveDialog && (
        <SaveRequestDialog
          defaultName={title}
          collections={collections ?? []}
          saving={saving}
          onClose={() => setShowSaveDialog(false)}
          onSave={(name, collectionId) => {
            setTitle(name);
            void save(collectionId, name).then(() => setShowSaveDialog(false));
          }}
        />
      )}
    </section>
  );
}

function Badge({ children, active }: { children: ReactNode; active: boolean }) {
  return (
    <span
      className={
        "rounded-full px-1.5 py-0.5 text-[10px] font-semibold leading-none " +
        (active ? "bg-accent/15 text-accent" : "bg-slate-100 text-slate-500 dark:bg-slate-800")
      }
    >
      {children}
    </span>
  );
}
