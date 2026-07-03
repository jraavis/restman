//! Native app menu so Cmd/Ctrl+W closes the active tab instead of the
//! window. macOS routes that shortcut through the menu accelerator — a
//! document-level keydown listener alone cannot intercept it. Deliberately
//! omits PredefinedMenuItem CloseWindow so the window close button still quits.

import { useEffect } from "react";
import { Menu, MenuItem, PredefinedMenuItem, Submenu } from "@tauri-apps/api/menu";
import { runCommand } from "../lib/commands";

export function useAppMenu() {
  useEffect(() => {
    let cancelled = false;

    void (async () => {
      const quit = await PredefinedMenuItem.new({ item: "Quit" });
      const appMenu = await Submenu.new({
        text: "restman",
        items: [quit],
      });

      const closeTab = await MenuItem.new({
        id: "tab.close",
        text: "Close Tab",
        accelerator: "CmdOrCtrl+W",
        action: () => runCommand("tab.close"),
      });
      const fileMenu = await Submenu.new({
        text: "File",
        items: [closeTab],
      });

      const undo = await PredefinedMenuItem.new({ item: "Undo" });
      const redo = await PredefinedMenuItem.new({ item: "Redo" });
      const cut = await PredefinedMenuItem.new({ item: "Cut" });
      const copy = await PredefinedMenuItem.new({ item: "Copy" });
      const paste = await PredefinedMenuItem.new({ item: "Paste" });
      const selectAll = await PredefinedMenuItem.new({ item: "SelectAll" });
      const editMenu = await Submenu.new({
        text: "Edit",
        items: [undo, redo, cut, copy, paste, selectAll],
      });

      const menu = await Menu.new({ items: [appMenu, fileMenu, editMenu] });
      if (!cancelled) await menu.setAsAppMenu();
    })();

    return () => {
      cancelled = true;
    };
  }, []);
}