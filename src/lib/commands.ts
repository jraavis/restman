//! Central registry of global app commands: static metadata (id/label/
//! category/default shortcut) for the command palette, plus a live handler
//! registry so components can attach the actual callback — which usually
//! needs fresh component state (e.g. RequestBuilder's `isLinked`) — without
//! each one owning its own `document.addEventListener`. One global listener
//! (`useGlobalCommandShortcuts`, mounted once in `AppShell`) resolves the
//! pressed key combo to a command id (checking `uiStore`'s user overrides
//! before a command's `defaultShortcut`) and invokes whatever handler is
//! currently registered for it.
//!
//! Deliberately not for component-local dismissal (Escape/click-outside) —
//! those aren't global app actions and don't belong in a remappable/
//! palette-searchable registry.

import { useEffect, useRef } from "react";
import { useUiStore } from "../stores/uiStore";

export interface CommandDef {
  id: string;
  label: string;
  category: string;
  /** e.g. "mod+s" — "mod" is Cmd on Mac, Ctrl elsewhere. Omitted for
   * palette-only commands with no default shortcut. */
  defaultShortcut?: string;
}

export const COMMANDS: CommandDef[] = [
  { id: "app.commandPalette", label: "Command palette", category: "App", defaultShortcut: "mod+k" },
  { id: "app.openSettings", label: "Open settings", category: "App" },
  { id: "app.openTools", label: "Developer tools", category: "App" },
  { id: "request.save", label: "Save request", category: "Request", defaultShortcut: "mod+s" },
  { id: "request.send", label: "Send request", category: "Request" },
  { id: "environment.switch", label: "Switch environment", category: "Environment", defaultShortcut: "mod+e" },
  { id: "tab.new", label: "New tab", category: "Tabs" },
  ...Array.from({ length: 9 }, (_, i) => ({
    id: `tab.switchTo.${i + 1}`,
    label: `Switch to tab ${i + 1}`,
    category: "Tabs",
    defaultShortcut: `mod+${i + 1}`,
  })),
];

type Handler = () => void;
const handlers = new Map<string, Handler>();

/** Registers the live callback for a command id, returning an unregister
 * function. Last registration wins if the same id is registered twice
 * (shouldn't happen in practice — each command id has exactly one owning
 * component). */
function registerCommandHandler(id: string, handler: Handler): () => void {
  handlers.set(id, handler);
  return () => {
    if (handlers.get(id) === handler) handlers.delete(id);
  };
}

/** Runs the currently-registered handler for a command id. Returns whether
 * a handler was found and ran — callers use this to decide whether to
 * `preventDefault()` the triggering keydown. */
export function runCommand(id: string): boolean {
  const handler = handlers.get(id);
  if (!handler) return false;
  handler();
  return true;
}

/** Attaches live handlers for one or more command ids for as long as the
 * calling component is mounted. Handlers are read through a ref each call
 * so the effect doesn't need to re-run (and the listener doesn't need to
 * re-attach) just because a closure's captured values changed. */
export function useRegisterCommands(bindings: Record<string, Handler>) {
  const bindingsRef = useRef(bindings);
  bindingsRef.current = bindings;
  const ids = Object.keys(bindings).join(",");
  useEffect(() => {
    const currentIds = ids ? ids.split(",") : [];
    const unregisters = currentIds.map((id) => registerCommandHandler(id, () => bindingsRef.current[id]?.()));
    return () => unregisters.forEach((u) => u());
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ids]);
}

export function useRegisterCommand(id: string, handler: Handler) {
  useRegisterCommands({ [id]: handler });
}

/** Normalizes a keydown event into a shortcut string like "mod+s", or null
 * if it's not a mod-chord (every registrable shortcut in this app requires
 * Cmd/Ctrl — plain-key shortcuts risk colliding with normal typing). */
export function normalizeShortcut(e: KeyboardEvent): string | null {
  if (!(e.metaKey || e.ctrlKey)) return null;
  const key = e.key.toLowerCase();
  if (["meta", "control", "shift", "alt"].includes(key)) return null;
  return `mod+${key}`;
}

/** Resolves a pressed shortcut string to whichever command currently owns
 * it — an override wins over a command's own `defaultShortcut` (and, since
 * overrides are 1:1 by command id, a remap can't silently orphan the
 * command it moved away from `defaultShortcut`). */
export function commandForShortcut(shortcut: string, overrides: Record<string, string>): CommandDef | undefined {
  return COMMANDS.find((c) => (overrides[c.id] ?? c.defaultShortcut) === shortcut);
}

/** Mount once (in `AppShell`) to make every registered command's shortcut
 * live app-wide. */
export function useGlobalCommandShortcuts() {
  const overrides = useUiStore((s) => s.keybindingOverrides);
  useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      const shortcut = normalizeShortcut(e);
      if (!shortcut) return;
      const cmd = commandForShortcut(shortcut, overrides);
      if (!cmd) return;
      if (runCommand(cmd.id)) e.preventDefault();
    }
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [overrides]);
}
