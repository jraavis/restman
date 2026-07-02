import { beforeEach, describe, expect, it } from "vitest";
import { useUiStore } from "./uiStore";

describe("uiStore", () => {
  beforeEach(() => {
    useUiStore.setState({ theme: "system", sidebarOpen: true, activePanel: "collections" });
  });

  it("toggles the sidebar", () => {
    expect(useUiStore.getState().sidebarOpen).toBe(true);
    useUiStore.getState().toggleSidebar();
    expect(useUiStore.getState().sidebarOpen).toBe(false);
  });

  it("sets the theme", () => {
    useUiStore.getState().setTheme("dark");
    expect(useUiStore.getState().theme).toBe("dark");
  });

  it("switches the active panel", () => {
    useUiStore.getState().setActivePanel("history");
    expect(useUiStore.getState().activePanel).toBe("history");
  });

  it("sets and clears a keybinding override", () => {
    useUiStore.getState().setKeybindingOverride("request.save", "mod+shift+s");
    expect(useUiStore.getState().keybindingOverrides["request.save"]).toBe("mod+shift+s");
    useUiStore.getState().clearKeybindingOverride("request.save");
    expect(useUiStore.getState().keybindingOverrides["request.save"]).toBeUndefined();
  });

  it("clamps editor tab size to [1, 8]", () => {
    useUiStore.getState().setEditorTabSize(0);
    expect(useUiStore.getState().editorTabSize).toBe(1);
    useUiStore.getState().setEditorTabSize(99);
    expect(useUiStore.getState().editorTabSize).toBe(8);
  });

  it("toggles editor word wrap", () => {
    useUiStore.getState().setEditorWordWrap(true);
    expect(useUiStore.getState().editorWordWrap).toBe(true);
  });

  it("sets autoCheckUpdates", () => {
    expect(useUiStore.getState().autoCheckUpdates).toBe(true);
    useUiStore.getState().setAutoCheckUpdates(false);
    expect(useUiStore.getState().autoCheckUpdates).toBe(false);
  });

  it("sets confirmBeforeDelete", () => {
    useUiStore.getState().setConfirmBeforeDelete(false);
    expect(useUiStore.getState().confirmBeforeDelete).toBe(false);
  });

  it("sets default request options for new tabs", () => {
    useUiStore.getState().setDefaultRequestOptions({ timeoutSecs: 60, followRedirects: false, verifySsl: false });
    expect(useUiStore.getState().defaultRequestOptions).toEqual({
      timeoutSecs: 60,
      followRedirects: false,
      verifySsl: false,
    });
  });
});
