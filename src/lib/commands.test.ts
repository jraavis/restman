//! Tests for the command registry's pure logic (shortcut normalization,
//! override resolution, tab-slot defaults). `runCommand`/registration are
//! covered indirectly through `useRegisterCommands`'s consumers.
//!
//! NOTE: `npx vitest run` cannot start in this sandbox (`ERR_REQUIRE_ESM`
//! from `html-encoding-sniffer`/`@exodus/bytes`, pre-existing in
//! `node_modules`, unrelated to this task); this file is hand-traced, not
//! run — but these exact assertions were run standalone via `npx tsx`
//! before this file was committed.

import { describe, expect, it } from "vitest";
import { COMMANDS, commandForShortcut, normalizeShortcut, runCommand } from "./commands";

function fakeEvent(opts: Partial<KeyboardEvent>): KeyboardEvent {
  return { metaKey: false, ctrlKey: false, key: "", ...opts } as KeyboardEvent;
}

describe("normalizeShortcut", () => {
  it("returns null without a mod key", () => {
    expect(normalizeShortcut(fakeEvent({ key: "s" }))).toBeNull();
  });

  it("normalizes Cmd/Meta and Ctrl the same way, lowercased", () => {
    expect(normalizeShortcut(fakeEvent({ metaKey: true, key: "s" }))).toBe("mod+s");
    expect(normalizeShortcut(fakeEvent({ ctrlKey: true, key: "S" }))).toBe("mod+s");
  });

  it("returns null for a bare modifier keydown", () => {
    expect(normalizeShortcut(fakeEvent({ metaKey: true, key: "Meta" }))).toBeNull();
  });
});

describe("commandForShortcut", () => {
  it("resolves a command's default shortcut with no overrides", () => {
    expect(commandForShortcut("mod+s", {})?.id).toBe("request.save");
    expect(commandForShortcut("mod+w", {})?.id).toBe("tab.close");
  });

  it("an override takes precedence, and the old default no longer resolves", () => {
    const overrides = { "request.save": "mod+shift+s" };
    expect(commandForShortcut("mod+shift+s", overrides)?.id).toBe("request.save");
    expect(commandForShortcut("mod+s", overrides)?.id).not.toBe("request.save");
  });
});

describe("COMMANDS", () => {
  it("registers all 9 tab-switch slots with mod+1..mod+9", () => {
    for (let i = 1; i <= 9; i++) {
      const cmd = COMMANDS.find((c) => c.id === `tab.switchTo.${i}`);
      expect(cmd?.defaultShortcut).toBe(`mod+${i}`);
    }
  });
});

describe("runCommand", () => {
  it("returns false for an id with no registered handler", () => {
    expect(runCommand("nonexistent")).toBe(false);
  });
});
