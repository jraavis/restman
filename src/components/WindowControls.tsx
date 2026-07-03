//! Frameless-window chrome: minimize, maximize, close.

import { Minus, Square, X } from "lucide-react";
import { getCurrentWindow } from "@tauri-apps/api/window";

const btn =
  "flex h-7 w-7 items-center justify-center rounded-md text-slate-500 hover:bg-slate-100 dark:text-slate-400 dark:hover:bg-slate-800";

const isMac = /Mac|iPhone|iPod|iPad/.test(navigator.userAgent);

export function WindowControls() {
  const appWindow = getCurrentWindow();

  return (
    <div className={`flex items-center gap-0.5 ${isMac ? "mr-1" : "ml-1"}`}>
      <button
        type="button"
        title="Minimize"
        className={btn}
        onClick={() => void appWindow.minimize()}
      >
        <Minus size={14} />
      </button>
      <button
        type="button"
        title="Maximize"
        className={btn}
        onClick={() => void appWindow.toggleMaximize()}
      >
        <Square size={12} />
      </button>
      <button
        type="button"
        title="Close"
        className={`${btn} hover:bg-red-100 hover:text-red-600 dark:hover:bg-red-900/40 dark:hover:text-red-400`}
        onClick={() => void appWindow.close()}
      >
        <X size={14} />
      </button>
    </div>
  );
}