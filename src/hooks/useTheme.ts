//! Applies the selected theme to <html> by toggling the `.dark` class,
//! reacting to OS preference changes when in "system" mode.

import { useEffect } from "react";
import { useUiStore } from "../stores/uiStore";

export function useApplyTheme(): void {
  const theme = useUiStore((s) => s.theme);

  useEffect(() => {
    const root = document.documentElement;
    const mq = window.matchMedia("(prefers-color-scheme: dark)");

    const apply = () => {
      const dark = theme === "dark" || (theme === "system" && mq.matches);
      root.classList.toggle("dark", dark);
    };

    apply();
    if (theme === "system") {
      mq.addEventListener("change", apply);
      return () => mq.removeEventListener("change", apply);
    }
  }, [theme]);
}

/** Applies the selected accent as a `data-accent` attribute on <html>; global.css
 * redefines `--color-accent` per value so `bg-accent`/`text-accent`/etc. just work. */
export function useApplyAccent(): void {
  const accent = useUiStore((s) => s.accent);

  useEffect(() => {
    document.documentElement.dataset.accent = accent;
  }, [accent]);
}

/** True if the effective (system-resolved) theme is dark. */
export function useIsDark(): boolean {
  const theme = useUiStore((s) => s.theme);
  return (
    theme === "dark" ||
    (theme === "system" &&
      typeof window !== "undefined" &&
      window.matchMedia("(prefers-color-scheme: dark)").matches)
  );
}
