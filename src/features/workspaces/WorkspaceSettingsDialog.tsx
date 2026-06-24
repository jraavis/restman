//! Workspace-level transport settings — outbound proxy, default headers
//! applied to every request in the workspace, and an optional mTLS client
//! certificate. Same modal shell as `CollectionAuthDialog`. The backend
//! (`get_workspace_settings`/`set_workspace_settings`) already existed;
//! this is its first frontend surface.

import { useState } from "react";
import {
  emptyClientCertConfig,
  SECRET_MASK,
  type ClientCertConfig,
  type ClientCertMode,
  type WorkspaceSettings,
} from "../../lib/types";
import type { HeaderEntry } from "../../lib/http";
import { Field, inputClass, SecretInput } from "../request/AuthConfigFields";
import { KeyValueEditor, type Pair } from "../request/KeyValueEditor";
import { useSetWorkspaceSettings, useWorkspaceSettings } from "./hooks";

export function WorkspaceSettingsDialog({
  workspaceId,
  workspaceName,
  onClose,
}: {
  workspaceId: string;
  workspaceName: string;
  onClose: () => void;
}) {
  const { data, isLoading, isError } = useWorkspaceSettings(workspaceId);

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/30" onClick={onClose}>
      <div
        onClick={(e) => e.stopPropagation()}
        className="flex max-h-[85vh] w-[34rem] flex-col rounded-lg border border-slate-200 bg-white p-4 shadow-xl dark:border-slate-700 dark:bg-slate-800"
      >
        <h2 className="mb-3 text-sm font-semibold text-slate-800 dark:text-slate-100">
          Workspace settings — {workspaceName}
        </h2>

        {isLoading && <p className="text-sm text-slate-400">Loading…</p>}
        {isError && <p className="text-sm text-red-500">Couldn't load workspace settings.</p>}
        {data && <WorkspaceSettingsForm workspaceId={workspaceId} initial={data} onClose={onClose} />}
      </div>
    </div>
  );
}

function WorkspaceSettingsForm({
  workspaceId,
  initial,
  onClose,
}: {
  workspaceId: string;
  initial: WorkspaceSettings;
  onClose: () => void;
}) {
  const [draft, setDraft] = useState(initial);
  const setSettings = useSetWorkspaceSettings(workspaceId);

  const headerRows: Pair[] = draft.defaultHeaders.map((h) => ({ key: h.name, value: h.value, enabled: h.enabled }));
  const setHeaderRows = (rows: Pair[]) =>
    setDraft({
      ...draft,
      defaultHeaders: rows.map<HeaderEntry>((r) => ({ name: r.key, value: r.value, enabled: r.enabled })),
    });

  function save() {
    setSettings.mutate(draft, { onSuccess: onClose });
  }

  return (
    <>
      <div className="flex max-h-[60vh] flex-col gap-4 overflow-y-auto pr-1">
        <Field label="Proxy URL">
          <input
            value={draft.proxyUrl ?? ""}
            onChange={(e) => setDraft({ ...draft, proxyUrl: e.target.value || null })}
            placeholder="http://proxy.corp:8080"
            className={inputClass}
          />
        </Field>

        <Field label="Proxy bypass (comma-separated hosts)">
          <input
            value={draft.proxyBypass ?? ""}
            onChange={(e) => setDraft({ ...draft, proxyBypass: e.target.value || null })}
            placeholder="localhost,*.corp"
            className={inputClass}
          />
        </Field>

        <Field label="Default headers">
          <KeyValueEditor
            rows={headerRows}
            onChange={setHeaderRows}
            keyPlaceholder="Header name"
            valuePlaceholder="Value"
          />
        </Field>

        <ClientCertFields value={draft.clientCert} onChange={(clientCert) => setDraft({ ...draft, clientCert })} />
      </div>

      <div className="mt-4 flex justify-end gap-2 text-sm">
        <button
          type="button"
          onClick={onClose}
          className="rounded-md px-3 py-1.5 text-slate-500 hover:bg-slate-100 dark:hover:bg-slate-700"
        >
          Cancel
        </button>
        <button
          type="button"
          disabled={setSettings.isPending}
          onClick={save}
          className="rounded-md bg-accent px-3 py-1.5 font-medium text-white hover:bg-accent-hover disabled:opacity-50"
        >
          {setSettings.isPending ? "Saving…" : "Save"}
        </button>
      </div>
    </>
  );
}

function ClientCertFields({
  value,
  onChange,
}: {
  value: ClientCertConfig;
  onChange: (value: ClientCertConfig) => void;
}) {
  return (
    <div className="flex flex-col gap-3 border-t border-slate-100 pt-3 dark:border-slate-700">
      <Field label="Client certificate (mTLS)">
        <select
          className={inputClass}
          value={value.mode}
          onChange={(e) => {
            const next = e.target.value as ClientCertMode;
            if (next !== value.mode) onChange(emptyClientCertConfig(next));
          }}
        >
          <option value="none">None</option>
          <option value="paste">Paste PEM</option>
          <option value="path">File path</option>
        </select>
      </Field>

      {value.mode === "paste" && (
        <>
          <Field label="Certificate (PEM)">
            <textarea
              rows={4}
              spellCheck={false}
              value={value.data.certPem}
              onChange={(e) => onChange({ mode: "paste", data: { ...value.data, certPem: e.target.value } })}
              placeholder="-----BEGIN CERTIFICATE-----"
              className={inputClass + " font-mono text-xs"}
            />
            {value.data.certPem === SECRET_MASK && (
              <span className="text-xs text-slate-400">Certificate already saved — paste to replace.</span>
            )}
          </Field>
          <Field label="Private key (PEM)">
            <textarea
              rows={4}
              spellCheck={false}
              value={value.data.keyPem}
              onChange={(e) => onChange({ mode: "paste", data: { ...value.data, keyPem: e.target.value } })}
              placeholder="-----BEGIN PRIVATE KEY-----"
              className={inputClass + " font-mono text-xs"}
            />
            {value.data.keyPem === SECRET_MASK && (
              <span className="text-xs text-slate-400">Private key already saved — paste to replace.</span>
            )}
          </Field>
          <Field label="Passphrase (optional)">
            <SecretInput
              value={value.data.passphrase ?? ""}
              onChange={(p) => onChange({ mode: "paste", data: { ...value.data, passphrase: p === "" ? null : p } })}
            />
          </Field>
        </>
      )}

      {value.mode === "path" && (
        <>
          <Field label="Certificate path">
            <input
              value={value.data.certPath}
              onChange={(e) => onChange({ mode: "path", data: { ...value.data, certPath: e.target.value } })}
              placeholder="/path/to/cert.pem"
              className={inputClass}
            />
          </Field>
          <Field label="Private key path">
            <input
              value={value.data.keyPath}
              onChange={(e) => onChange({ mode: "path", data: { ...value.data, keyPath: e.target.value } })}
              placeholder="/path/to/key.pem"
              className={inputClass}
            />
          </Field>
          <Field label="Passphrase (optional)">
            <SecretInput
              value={value.data.passphrase ?? ""}
              onChange={(p) => onChange({ mode: "path", data: { ...value.data, passphrase: p === "" ? null : p } })}
            />
          </Field>
        </>
      )}
    </div>
  );
}
