//! Portals modal content to `document.body`. Needed for any dialog mounted
//! inside the sidebar tree: the sidebar and `<main>` are sibling stacking
//! contexts (both `relative z-10`), and `<main>` comes later in the DOM, so
//! a `fixed` overlay rendered inline inside the sidebar paints underneath
//! `<main>` regardless of its own z-index. Rendering at `document.body`
//! escapes both contexts.

import { createPortal } from "react-dom";
import type { ReactNode } from "react";

export function ModalPortal({ children }: { children: ReactNode }) {
  return createPortal(children, document.body);
}
