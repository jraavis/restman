//! Settings → Data: restman-native full export/import (`.restman.json`).
//! Selective (checkbox per workspace) and unencrypted, unlike the
//! password-protected full-app backup above it — secrets stay masked unless
//! the user explicitly opts into plaintext. Import runs the shared
//! preview → conflict-mode → apply flow (`ipc.previewRestmanImport` /
//! `ipc.applyRestmanImport`), mirroring `ImportDialog`'s wording.

import { useRef, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { AlertTriangle } from "lucide-react";
import { useQueryClient } from "@tanstack/react-query";
import { ipc } from "../../lib/ipc";
import { textToBase64 } from "../../lib/encoding";
import { useRequestStore } from "../../stores/requestStore";
import { useWorkspaces } from "../workspaces/hooks";
import type { ConflictMode, FullImportPreview, FullImportReport } from "../../lib/types";

/** Human status line covering every outcome the report can carry — an
 * Overwrite import into an existing workspace legitimately *creates*
 * nothing (everything lands in `overwritten`), and showing only created
 * counts made a successful import read as "Imported 0 …". */
export function formatImportStatus(report: FullImportReport): string {
  const parts: string[] = [];
  if (report.workspacesCreated > 0) parts.push(`${report.workspacesCreated} workspace(s) created`);
  if (report.createdCollections > 0) parts.push(`${report.createdCollections} collection(s) created`);
  if (report.createdRequests > 0) parts.push(`${report.createdRequests} request(s) created`);
  if (report.overwritten > 0) parts.push(`${report.overwritten} request(s) overwritten`);
  if (report.environmentsCreated > 0) parts.push(`${report.environmentsCreated} environment(s) created`);
  if (report.variablesCreated > 0) parts.push(`${report.variablesCreated} variable(s) created`);
  if (report.variablesOverwritten > 0) parts.push(`${report.variablesOverwritten} variable(s) overwritten`);
  if (report.skipped > 0) parts.push(`${report.skipped} request(s) skipped`);
  if (report.variablesSkipped > 0) parts.push(`${report.variablesSkipped} variable(s) skipped`);

  const summary =
    parts.length > 0
      ? `Import finished: ${parts.join(", ")}.`
      : "Import finished: everything in the file already matches this app — nothing changed.";
  return summary + (report.warnings.length > 0 ? ` ${report.warnings.length} warning(s).` : "");
}

function SectionLabel({ children }: { children: string }) {
  return (
    <p className="mb-1.5 text-xs font-semibold tracking-wide text-slate-400 uppercase dark:text-slate-500">
      {children}
    </p>
  );
}

const BUTTON_CLASS =
  "rounded-lg border border-slate-200 px-3 py-1.5 text-sm text-slate-600 hover:bg-slate-100 disabled:opacity-40 dark:border-slate-700 dark:text-slate-300 dark:hover:bg-slate-700";

export function ExportImportSection() {
  return (
    <div>
      <SectionLabel>Restman export &amp; import</SectionLabel>
      <div className="flex flex-col gap-4">
        <ExportControls />
        <ImportControls />
      </div>
    </div>
  );
}

function ExportControls() {
  const { data: workspaces } = useWorkspaces();
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [includeSecrets, setIncludeSecrets] = useState(false);
  const [status, setStatus] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  function toggle(id: string) {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }

  async function exportRestman() {
    setBusy(true);
    setStatus(null);
    try {
      const json = await ipc.exportRestman([...selected], includeSecrets, true);
      const path = await save({
        defaultPath: `restman-export-${new Date().toISOString().slice(0, 10)}.restman.json`,
        filters: [{ name: "Restman export", extensions: ["json"] }],
      });
      if (!path) return;
      await ipc.writeFileBytes(path, textToBase64(json));
      setStatus("Export saved.");
    } catch (e) {
      setStatus(`Export failed: ${e}`);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="flex flex-col gap-2">
      <p className="text-xs text-slate-400">
        Export selected workspaces — collections, requests (including scripts), environments, and
        variables — to a single .restman.json file.
      </p>
      <div className="flex max-h-32 flex-col gap-1 overflow-y-auto rounded border border-slate-200 p-2 dark:border-slate-700">
        {(workspaces ?? []).map((w) => (
          <label key={w.id} className="flex items-center gap-2 text-sm">
            <input
              type="checkbox"
              checked={selected.has(w.id)}
              onChange={() => toggle(w.id)}
              className="accent-accent"
            />
            {w.name}
          </label>
        ))}
        {(workspaces ?? []).length === 0 && (
          <p className="text-xs text-slate-400">No workspaces.</p>
        )}
      </div>
      <label className="flex items-center gap-2 text-sm">
        <input
          type="checkbox"
          checked={includeSecrets}
          onChange={(e) => setIncludeSecrets(e.target.checked)}
          className="accent-accent"
        />
        Include secrets
      </label>
      {includeSecrets && (
        <p className="flex items-center gap-1 text-xs text-amber-600 dark:text-amber-400">
          <AlertTriangle size={12} /> Tokens, passwords, and secret variables will be written in
          plaintext. Share this file carefully.
        </p>
      )}
      <div>
        <button type="button" disabled={busy || selected.size === 0} onClick={() => void exportRestman()} className={BUTTON_CLASS}>
          Export…
        </button>
      </div>
      {status && <p className="text-xs text-slate-500 dark:text-slate-400">{status}</p>}
    </div>
  );
}

function ImportControls() {
  const qc = useQueryClient();
  const fileInputRef = useRef<HTMLInputElement>(null);
  const [content, setContent] = useState<string | null>(null);
  const [preview, setPreview] = useState<FullImportPreview | null>(null);
  const [mode, setMode] = useState<ConflictMode>("skip");
  const [status, setStatus] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function loadFile(file: File) {
    setBusy(true);
    setStatus(null);
    setPreview(null);
    try {
      const text = await file.text();
      const p = await ipc.previewRestmanImport(text);
      setContent(text);
      setPreview(p);
    } catch (e) {
      setStatus(`Import failed: ${e}`);
    } finally {
      setBusy(false);
    }
  }

  async function applyImport() {
    if (!content) return;
    setBusy(true);
    setStatus(null);
    try {
      const report = await ipc.applyRestmanImport(content, mode);
      setStatus(formatImportStatus(report));
      setContent(null);
      setPreview(null);
      // Names/trees may have changed anywhere — refetch everything, then
      // force the active tab to reload from its (possibly import-refreshed)
      // DB draft: an Overwrite import rewrites tab drafts in place, and the
      // on-screen editor would otherwise keep showing the pre-import content.
      await qc.invalidateQueries();
      useRequestStore.setState({ activeTabId: null });
    } catch (e) {
      setStatus(`Import failed: ${e}`);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="flex flex-col gap-2">
      <input
        ref={fileInputRef}
        type="file"
        accept=".json"
        className="hidden"
        onChange={(e) => {
          const file = e.target.files?.[0];
          e.target.value = "";
          if (file) void loadFile(file);
        }}
      />
      {!preview && (
        <div>
          <button type="button" disabled={busy} onClick={() => fileInputRef.current?.click()} className={BUTTON_CLASS}>
            Import from file…
          </button>
        </div>
      )}

      {preview && (
        <div className="flex flex-col gap-2 rounded border border-slate-200 p-2 dark:border-slate-700">
          <div className="text-xs text-slate-600 dark:text-slate-300">
            {preview.workspaces.map((w) => (
              <div key={w.name} className="flex items-center gap-2 py-0.5">
                <span className="font-medium">{w.name}</span>
                {w.exists && (
                  <span className="rounded bg-amber-100 px-1 text-[10px] text-amber-700 dark:bg-amber-950/40 dark:text-amber-400">
                    exists — will merge
                  </span>
                )}
                <span className="text-slate-400">
                  {w.collections} collections · {w.requests} requests · {w.environments}{" "}
                  environments · {w.variables} variables
                </span>
              </div>
            ))}
            {preview.globalVariables > 0 && (
              <div className="py-0.5 text-slate-400">{preview.globalVariables} global variable(s)</div>
            )}
          </div>

          {preview.warnings.length > 0 && (
            <div className="rounded border border-amber-200 bg-amber-50 p-2 text-xs text-amber-700 dark:border-amber-900 dark:bg-amber-950/30 dark:text-amber-400">
              {preview.warnings.map((w, i) => (
                <div key={i}>{w}</div>
              ))}
            </div>
          )}

          <label className="flex items-center gap-2 text-xs text-slate-600 dark:text-slate-300">
            On name conflict
            <select
              value={mode}
              onChange={(e) => setMode(e.target.value as ConflictMode)}
              className="rounded border border-slate-200 px-2 py-1 text-xs dark:border-slate-700 dark:bg-slate-800"
            >
              <option value="skip">Skip existing</option>
              <option value="overwrite">Overwrite existing</option>
              <option value="merge">Keep both</option>
            </select>
          </label>

          <div className="flex items-center gap-2">
            <button type="button" disabled={busy} onClick={() => void applyImport()} className={BUTTON_CLASS}>
              Import
            </button>
            <button
              type="button"
              disabled={busy}
              onClick={() => {
                setContent(null);
                setPreview(null);
                setStatus(null);
              }}
              className={BUTTON_CLASS}
            >
              Cancel
            </button>
          </div>
        </div>
      )}

      {status && <p className="text-xs text-slate-500 dark:text-slate-400">{status}</p>}
    </div>
  );
}
