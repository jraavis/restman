//! Full settings dialog: General / Editor / Network / Data / Keybindings /
//! About. Replaces the old appearance-only popover (`SettingsPanel`) —
//! theme/accent/font-size/history-retention all moved here, redistributed
//! into the tab each belongs under. Same overlay shell as `CookieJarDialog`.
//!
//! Network's "defaults for new requests" (timeout/redirects/verify-ssl) are
//! distinct from the per-workspace proxy/client-cert settings in
//! `WorkspaceSettingsDialog` — this tab only seeds a freshly created
//! request/tab, it isn't a transport override. Data also carries full-app
//! ZIP backup/restore (Phase 8) — unlike everything else in this dialog,
//! that's app-wide (every workspace), not scoped to the active one; see
//! `crate::backup` module doc for why it bundles real secrets under a
//! required password instead of following this app's usual mask-on-write
//! export contract. About carries the manual update check, backed by the
//! shared `useUpdater` store (also driving the launch-time `UpdateBanner`);
//! the signing keypair and GitHub Releases endpoint are live — see PLAN.md.

import { useEffect, useRef, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { save } from "@tauri-apps/plugin-dialog";
import { Minus, Monitor, Moon, Plus, RotateCcw, Sun } from "lucide-react";
import { COMMANDS, commandForShortcut, normalizeShortcut } from "../../lib/commands";
import { ipc } from "../../lib/ipc";
import { Switch } from "../../components/Switch";
import { UpdateProgressBar } from "../updates/UpdateProgressBar";
import { useUpdaterStore } from "../updates/useUpdater";
import { useUiStore, type Accent, type Theme } from "../../stores/uiStore";
import { useClearHistory, useHistoryRetention, useSetHistoryRetention } from "../history/hooks";

const ACCENTS: { id: Accent; label: string; swatch: string }[] = [
  { id: "blue", label: "Blue", swatch: "#3b82f6" },
  { id: "green", label: "Green", swatch: "#16a34a" },
  { id: "purple", label: "Purple", swatch: "#8b5cf6" },
  { id: "orange", label: "Orange", swatch: "#f97316" },
  { id: "pink", label: "Pink", swatch: "#ec4899" },
  { id: "red", label: "Red", swatch: "#ef4444" },
];

const THEMES: { id: Theme; label: string; icon: typeof Sun }[] = [
  { id: "light", label: "Light", icon: Sun },
  { id: "dark", label: "Dark", icon: Moon },
  { id: "system", label: "System", icon: Monitor },
];

type SettingsTab = "general" | "editor" | "network" | "data" | "keybindings" | "about";
const TABS: { id: SettingsTab; label: string }[] = [
  { id: "general", label: "General" },
  { id: "editor", label: "Editor" },
  { id: "network", label: "Network" },
  { id: "data", label: "Data" },
  { id: "keybindings", label: "Keybindings" },
  { id: "about", label: "About" },
];

function SectionLabel({ children }: { children: string }) {
  return (
    <p className="mb-1.5 text-xs font-semibold tracking-wide text-slate-400 uppercase dark:text-slate-500">
      {children}
    </p>
  );
}

export function SettingsDialog({ onClose, workspaceId }: { onClose: () => void; workspaceId?: string }) {
  const [tab, setTab] = useState<SettingsTab>("general");

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/30" onClick={onClose}>
      <div
        onClick={(e) => e.stopPropagation()}
        className="flex h-[32rem] w-[42rem] overflow-hidden rounded-lg border border-slate-200 bg-white shadow-xl dark:border-slate-700 dark:bg-slate-800"
      >
        <nav className="w-40 shrink-0 border-r border-slate-100 p-2 dark:border-slate-700">
          {TABS.map((t) => (
            <button
              key={t.id}
              type="button"
              onClick={() => setTab(t.id)}
              className={
                "block w-full rounded-md px-2.5 py-1.5 text-left text-sm " +
                (tab === t.id
                  ? "bg-accent/10 font-medium text-accent"
                  : "text-slate-600 hover:bg-slate-100 dark:text-slate-300 dark:hover:bg-slate-700")
              }
            >
              {t.label}
            </button>
          ))}
        </nav>
        <div className="min-h-0 flex-1 overflow-y-auto p-4">
          {tab === "general" && <GeneralTab />}
          {tab === "editor" && <EditorTab />}
          {tab === "network" && <NetworkTab />}
          {tab === "data" && <DataTab workspaceId={workspaceId} />}
          {tab === "keybindings" && <KeybindingsTab />}
          {tab === "about" && <AboutTab />}
        </div>
      </div>
    </div>
  );
}

function GeneralTab() {
  const theme = useUiStore((s) => s.theme);
  const setTheme = useUiStore((s) => s.setTheme);
  const accent = useUiStore((s) => s.accent);
  const setAccent = useUiStore((s) => s.setAccent);
  const confirmBeforeDelete = useUiStore((s) => s.confirmBeforeDelete);
  const setConfirmBeforeDelete = useUiStore((s) => s.setConfirmBeforeDelete);
  const autoCheckUpdates = useUiStore((s) => s.autoCheckUpdates);
  const setAutoCheckUpdates = useUiStore((s) => s.setAutoCheckUpdates);

  return (
    <div className="flex flex-col gap-5">
      <div>
        <SectionLabel>Theme</SectionLabel>
        <div className="grid w-56 grid-cols-3 gap-1">
          {THEMES.map(({ id, label, icon: Icon }) => (
            <button
              key={id}
              type="button"
              onClick={() => setTheme(id)}
              className={
                "flex flex-col items-center gap-1 rounded-lg border px-2 py-1.5 text-xs transition-colors " +
                (theme === id
                  ? "border-accent/40 bg-accent/10 text-accent"
                  : "border-transparent text-slate-500 hover:bg-slate-100 dark:text-slate-400 dark:hover:bg-slate-800")
              }
            >
              <Icon size={15} />
              {label}
            </button>
          ))}
        </div>
      </div>

      <div>
        <SectionLabel>Accent</SectionLabel>
        <div className="flex flex-wrap gap-2">
          {ACCENTS.map((a) => (
            <button
              key={a.id}
              type="button"
              title={a.label}
              onClick={() => setAccent(a.id)}
              className={
                "h-6 w-6 rounded-full ring-offset-2 ring-offset-white transition-shadow dark:ring-offset-slate-800 " +
                (accent === a.id ? "ring-2 ring-slate-900 dark:ring-white" : "")
              }
              style={{ backgroundColor: a.swatch }}
            />
          ))}
        </div>
      </div>

      <div>
        <SectionLabel>Deletions</SectionLabel>
        <Switch
          checked={confirmBeforeDelete}
          onChange={setConfirmBeforeDelete}
          label="Confirm before deleting"
        />
        <p className="mt-1 text-xs text-slate-400">
          Applies to workspaces, collections, requests, environments, tags, plugins, and mock
          servers. Bulk actions (clear history, restore) always confirm.
        </p>
      </div>

      <div>
        <SectionLabel>Updates</SectionLabel>
        <Switch
          checked={autoCheckUpdates}
          onChange={setAutoCheckUpdates}
          label="Check for updates automatically"
        />
        <p className="mt-1 text-xs text-slate-400">
          Checks GitHub for a new version shortly after launch and shows a prompt when one is
          available. Nothing installs without your confirmation.
        </p>
      </div>
    </div>
  );
}

function EditorTab() {
  const fontSize = useUiStore((s) => s.editorFontSize);
  const setEditorFontSize = useUiStore((s) => s.setEditorFontSize);
  const wordWrap = useUiStore((s) => s.editorWordWrap);
  const setEditorWordWrap = useUiStore((s) => s.setEditorWordWrap);
  const tabSize = useUiStore((s) => s.editorTabSize);
  const setEditorTabSize = useUiStore((s) => s.setEditorTabSize);

  return (
    <div className="flex flex-col gap-5">
      <div>
        <SectionLabel>Font size</SectionLabel>
        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={() => setEditorFontSize(fontSize - 1)}
            className="flex h-6 w-6 items-center justify-center rounded-md text-slate-500 hover:bg-slate-100 dark:text-slate-400 dark:hover:bg-slate-700"
          >
            <Minus size={13} />
          </button>
          <span className="w-6 text-center text-sm tabular-nums">{fontSize}</span>
          <button
            type="button"
            onClick={() => setEditorFontSize(fontSize + 1)}
            className="flex h-6 w-6 items-center justify-center rounded-md text-slate-500 hover:bg-slate-100 dark:text-slate-400 dark:hover:bg-slate-700"
          >
            <Plus size={13} />
          </button>
        </div>
      </div>

      <div>
        <SectionLabel>Tab size</SectionLabel>
        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={() => setEditorTabSize(tabSize - 1)}
            className="flex h-6 w-6 items-center justify-center rounded-md text-slate-500 hover:bg-slate-100 dark:text-slate-400 dark:hover:bg-slate-700"
          >
            <Minus size={13} />
          </button>
          <span className="w-6 text-center text-sm tabular-nums">{tabSize}</span>
          <button
            type="button"
            onClick={() => setEditorTabSize(tabSize + 1)}
            className="flex h-6 w-6 items-center justify-center rounded-md text-slate-500 hover:bg-slate-100 dark:text-slate-400 dark:hover:bg-slate-700"
          >
            <Plus size={13} />
          </button>
        </div>
      </div>

      <Switch checked={wordWrap} onChange={setEditorWordWrap} label="Word wrap" />
    </div>
  );
}

function NetworkTab() {
  const defaults = useUiStore((s) => s.defaultRequestOptions);
  const setDefaults = useUiStore((s) => s.setDefaultRequestOptions);

  return (
    <div className="flex flex-col gap-5">
      <div>
        <SectionLabel>Defaults for new requests</SectionLabel>
        <p className="mb-3 text-xs text-slate-400">
          Applied when a new request tab is created — not retroactive to existing requests, and separate from a
          workspace's proxy/client-cert settings.
        </p>
        <label className="flex items-center gap-2 text-sm">
          <span className="w-32 text-slate-500 dark:text-slate-400">Timeout (seconds)</span>
          <input
            type="number"
            min={1}
            max={300}
            value={defaults.timeoutSecs}
            onChange={(e) => setDefaults({ ...defaults, timeoutSecs: Number(e.target.value) })}
            className="w-20 rounded-lg border border-slate-200 bg-transparent px-2 py-1 focus:outline-none focus:ring-2 focus:ring-accent/40 dark:border-slate-700"
          />
        </label>
      </div>
      <Switch
        checked={defaults.followRedirects}
        onChange={(v) => setDefaults({ ...defaults, followRedirects: v })}
        label="Follow redirects"
      />
      <Switch
        checked={defaults.verifySsl}
        onChange={(v) => setDefaults({ ...defaults, verifySsl: v })}
        label="Verify SSL certificate"
      />
    </div>
  );
}

function DataTab({ workspaceId }: { workspaceId?: string }) {
  const { data: retention } = useHistoryRetention();
  const setRetention = useSetHistoryRetention();
  const clearHistory = useClearHistory(workspaceId);
  const [draftRetention, setDraftRetention] = useState(retention != null ? String(retention) : "");

  function commitRetention() {
    const n = parseInt(draftRetention, 10);
    if (Number.isFinite(n) && n > 0) {
      if (n !== retention) setRetention.mutate(n);
    } else if (retention != null) {
      setDraftRetention(String(retention));
    }
  }

  return (
    <div className="flex flex-col gap-5">
      <div>
        <SectionLabel>History retention</SectionLabel>
        <label className="flex items-center gap-2 text-sm">
          <span className="text-slate-500 dark:text-slate-400">Keep last</span>
          <input
            type="number"
            min={1}
            step={50}
            value={draftRetention || (retention != null ? String(retention) : "")}
            onChange={(e) => setDraftRetention(e.target.value)}
            onBlur={commitRetention}
            onKeyDown={(e) => {
              if (e.key === "Enter") commitRetention();
            }}
            className="w-20 rounded-lg border border-slate-200 bg-transparent px-2 py-1 tabular-nums focus:outline-none focus:ring-2 focus:ring-accent/40 dark:border-slate-700"
          />
          <span className="text-slate-500 dark:text-slate-400">requests, per workspace</span>
        </label>
      </div>

      <div>
        <SectionLabel>Clear data</SectionLabel>
        <button
          type="button"
          disabled={!workspaceId || clearHistory.isPending}
          onClick={() => {
            if (window.confirm("Clear all history for the active workspace? This can't be undone.")) {
              clearHistory.mutate();
            }
          }}
          className="rounded-lg border border-slate-200 px-3 py-1.5 text-sm text-slate-600 hover:bg-slate-100 disabled:opacity-40 dark:border-slate-700 dark:text-slate-300 dark:hover:bg-slate-700"
        >
          Clear history for this workspace
        </button>
      </div>

      <BackupSection />
    </div>
  );
}

/** App-wide (every workspace, not just the active one) ZIP backup/restore.
 * A required password AES-256 encrypts the archive, since — unlike every
 * other export in this app — it bundles real, unmasked secrets for a true
 * one-shot local disaster-recovery restore. See `crate::backup`. */
function BackupSection() {
  const fileInputRef = useRef<HTMLInputElement>(null);
  const [backupPassword, setBackupPassword] = useState("");
  const [restorePassword, setRestorePassword] = useState("");
  const [status, setStatus] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function createBackup() {
    if (!backupPassword) {
      setStatus("Enter a password to protect the backup first.");
      return;
    }
    setBusy(true);
    setStatus(null);
    try {
      const contentBase64 = await ipc.createBackup(backupPassword);
      const path = await save({
        defaultPath: `restman-backup-${new Date().toISOString().slice(0, 10)}.zip`,
      });
      if (!path) return;
      await ipc.writeFileBytes(path, contentBase64);
      setStatus("Backup saved.");
      setBackupPassword("");
    } catch (e) {
      setStatus(`Backup failed: ${e}`);
    } finally {
      setBusy(false);
    }
  }

  async function restoreBackup(file: File) {
    if (!restorePassword) {
      setStatus("Enter the backup's password first.");
      return;
    }
    if (
      !window.confirm(
        "Restore this backup? It replaces every workspace, collection, environment, and secret currently in the app. This can't be undone.",
      )
    ) {
      return;
    }
    setBusy(true);
    setStatus(null);
    try {
      const dataUrl = await new Promise<string>((resolve, reject) => {
        const reader = new FileReader();
        reader.onload = () => resolve(reader.result as string);
        reader.onerror = () => reject(reader.error);
        reader.readAsDataURL(file);
      });
      const contentBase64 = dataUrl.slice(dataUrl.indexOf(",") + 1);
      const report = await ipc.restoreBackup(contentBase64, restorePassword);
      setStatus(
        `Restored ${report.workspaces} workspace(s), ${report.collections} collection(s), ` +
          `${report.requests} request(s), ${report.environments} environment(s), ` +
          `${report.historyEntries} history entr(y/ies), ${report.secretsRestored} secret(s). Reloading…`,
      );
      setRestorePassword("");
      window.location.reload();
    } catch (e) {
      setStatus(`Restore failed: ${e}`);
      setBusy(false);
    }
  }

  return (
    <div>
      <SectionLabel>Backup &amp; restore (every workspace)</SectionLabel>
      <div className="flex flex-col gap-3">
        <div className="flex items-center gap-2">
          <input
            type="password"
            value={backupPassword}
            onChange={(e) => setBackupPassword(e.target.value)}
            placeholder="Backup password"
            className="w-48 rounded-lg border border-slate-200 bg-transparent px-2 py-1 text-sm focus:outline-none focus:ring-2 focus:ring-accent/40 dark:border-slate-700"
          />
          <button
            type="button"
            disabled={busy}
            onClick={createBackup}
            className="rounded-lg border border-slate-200 px-3 py-1.5 text-sm text-slate-600 hover:bg-slate-100 disabled:opacity-40 dark:border-slate-700 dark:text-slate-300 dark:hover:bg-slate-700"
          >
            Create backup…
          </button>
        </div>

        <div className="flex items-center gap-2">
          <input
            type="password"
            value={restorePassword}
            onChange={(e) => setRestorePassword(e.target.value)}
            placeholder="Backup password"
            className="w-48 rounded-lg border border-slate-200 bg-transparent px-2 py-1 text-sm focus:outline-none focus:ring-2 focus:ring-accent/40 dark:border-slate-700"
          />
          <input
            ref={fileInputRef}
            type="file"
            accept=".zip"
            className="hidden"
            onChange={(e) => {
              const file = e.target.files?.[0];
              e.target.value = "";
              if (file) void restoreBackup(file);
            }}
          />
          <button
            type="button"
            disabled={busy}
            onClick={() => fileInputRef.current?.click()}
            className="rounded-lg border border-slate-200 px-3 py-1.5 text-sm text-slate-600 hover:bg-slate-100 disabled:opacity-40 dark:border-slate-700 dark:text-slate-300 dark:hover:bg-slate-700"
          >
            Restore from file…
          </button>
        </div>

        {status && <p className="text-xs text-slate-500 dark:text-slate-400">{status}</p>}
      </div>
    </div>
  );
}

function KeybindingsTab() {
  const overrides = useUiStore((s) => s.keybindingOverrides);
  const setOverride = useUiStore((s) => s.setKeybindingOverride);
  const clearOverride = useUiStore((s) => s.clearKeybindingOverride);
  const [capturingId, setCapturingId] = useState<string | null>(null);
  const [collision, setCollision] = useState<string | null>(null);

  function handleCaptureKeyDown(e: React.KeyboardEvent, commandId: string) {
    e.preventDefault();
    // Without this, the keydown still bubbles to the document-level global
    // listener (`useGlobalCommandShortcuts`) — caught live: capturing a new
    // binding that happens to match another command's *current* shortcut
    // fired that command for real (e.g. remapping into Cmd+D while Cmd+D
    // already owned "Save request" popped the actual save dialog).
    e.stopPropagation();
    const shortcut = normalizeShortcut(e.nativeEvent);
    if (!shortcut) return;
    const existing = commandForShortcut(shortcut, overrides);
    if (existing && existing.id !== commandId) {
      setCollision(`Already used by "${existing.label}"`);
      return;
    }
    setOverride(commandId, shortcut);
    setCapturingId(null);
    setCollision(null);
  }

  return (
    <div className="flex flex-col gap-0.5">
      {COMMANDS.filter((c) => c.id !== "app.commandPalette").map((c) => {
        const shortcut = overrides[c.id] ?? c.defaultShortcut;
        const isCapturing = capturingId === c.id;
        return (
          <div
            key={c.id}
            className="flex items-center justify-between rounded-md px-1 py-1.5 hover:bg-slate-50 dark:hover:bg-slate-700/40"
          >
            <span className="text-sm text-slate-700 dark:text-slate-200">{c.label}</span>
            <div className="flex items-center gap-1.5">
              {isCapturing ? (
                <input
                  autoFocus
                  readOnly
                  value={collision ?? "Press a key…"}
                  onKeyDown={(e) => handleCaptureKeyDown(e, c.id)}
                  onBlur={() => {
                    setCapturingId(null);
                    setCollision(null);
                  }}
                  className={
                    "w-40 rounded-md border px-2 py-1 text-xs focus:outline-none " +
                    (collision ? "border-red-400 text-red-500" : "border-accent/40 text-accent")
                  }
                />
              ) : (
                <button
                  type="button"
                  onClick={() => {
                    setCapturingId(c.id);
                    setCollision(null);
                  }}
                  className="w-40 rounded-md border border-slate-200 px-2 py-1 text-left font-mono text-xs text-slate-500 hover:bg-slate-100 dark:border-slate-600 dark:text-slate-400 dark:hover:bg-slate-700"
                >
                  {shortcut ?? "No shortcut"}
                </button>
              )}
              {overrides[c.id] && (
                <button
                  type="button"
                  title="Reset to default"
                  onClick={() => clearOverride(c.id)}
                  className="flex h-6 w-6 items-center justify-center rounded-md text-slate-400 hover:bg-slate-100 dark:hover:bg-slate-700"
                >
                  <RotateCcw size={12} />
                </button>
              )}
            </div>
          </div>
        );
      })}
    </div>
  );
}

function AboutTab() {
  // Real bundle version from Tauri (`tauri.conf.json`'s `version`), replacing
  // an earlier hardcoded literal that had already drifted. Null outside a
  // Tauri shell (plain-vite dev/preview, jsdom tests).
  const [version, setVersion] = useState<string | null>(null);
  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    getVersion().then(setVersion, () => {});
  }, []);

  return (
    <div className="flex flex-col gap-3 text-sm">
      <div>
        <p className="font-semibold text-slate-800 dark:text-slate-100">Restman</p>
        <p className="text-xs text-slate-400">Version {version ?? "—"}</p>
      </div>
      <p className="text-slate-600 dark:text-slate-300">
        A privacy-first, offline-capable REST API client. All networking, storage, and
        credential handling happen in the Rust backend — the frontend never touches the
        network or disk directly.
      </p>
      <a
        href="https://github.com/jraavis/restman"
        target="_blank"
        rel="noreferrer"
        className="text-accent hover:underline"
      >
        github.com/jraavis/restman
      </a>
      <UpdateCheck />
    </div>
  );
}

/** Manual check, driven by the shared `useUpdater` store — same flow (and
 * progress state) as the launch-time `UpdateBanner`, just surfaced inline. */
function UpdateCheck() {
  const phase = useUpdaterStore((s) => s.phase);
  const update = useUpdaterStore((s) => s.update);
  const progress = useUpdaterStore((s) => s.progress);
  const error = useUpdaterStore((s) => s.error);
  const checkForUpdate = useUpdaterStore((s) => s.checkForUpdate);
  const installAndRestart = useUpdaterStore((s) => s.installAndRestart);

  const busy = phase === "checking" || phase === "downloading" || phase === "installing";
  const status =
    phase === "checking"
      ? "Checking…"
      : phase === "available"
        ? `Update available: v${update?.version}`
        : phase === "downloading"
          ? "Downloading…"
          : phase === "installing"
            ? "Installing…"
            : phase === "upToDate"
              ? "You're up to date."
              : phase === "error"
                ? `Update failed: ${error}`
                : null;

  return (
    <div className="mt-1 border-t border-slate-100 pt-3 dark:border-slate-700">
      <div className="flex items-center gap-2">
        <button
          type="button"
          disabled={busy}
          onClick={() => void checkForUpdate()}
          className="rounded-lg border border-slate-200 px-3 py-1.5 text-xs text-slate-600 hover:bg-slate-100 disabled:opacity-40 dark:border-slate-700 dark:text-slate-300 dark:hover:bg-slate-700"
        >
          Check for updates
        </button>
        {update && (
          <button
            type="button"
            disabled={busy}
            onClick={() => void installAndRestart()}
            className="rounded-lg bg-accent px-3 py-1.5 text-xs font-medium text-white hover:bg-accent-hover disabled:opacity-40"
          >
            Install &amp; restart
          </button>
        )}
      </div>
      {(phase === "downloading" || phase === "installing") && (
        <div className="mt-2 w-64">
          <UpdateProgressBar progress={progress} />
        </div>
      )}
      {status && <p className="mt-1.5 text-xs text-slate-400">{status}</p>}
    </div>
  );
}
