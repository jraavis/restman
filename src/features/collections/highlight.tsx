//! Renders FTS5 `highlight()` output. The backend wraps matches in
//! `char(1)`/`char(2)` (\x01/\x02) instead of HTML tags — see
//! `store/requests.rs::search` — so they survive untrusted text safely and
//! get turned into `<mark>` here instead of being injected as markup.

import type { ReactNode } from "react";

export function renderHighlight(text: string): ReactNode[] {
  const nodes: ReactNode[] = [];
  const re = /\x01(.*?)\x02/g;
  let last = 0;
  let match: RegExpExecArray | null;
  let key = 0;

  while ((match = re.exec(text))) {
    if (match.index > last) nodes.push(text.slice(last, match.index));
    nodes.push(
      <mark key={key++} className="rounded-sm bg-accent/25 text-inherit">
        {match[1]}
      </mark>,
    );
    last = re.lastIndex;
  }
  if (last < text.length) nodes.push(text.slice(last));
  return nodes;
}
