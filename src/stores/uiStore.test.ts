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
});
