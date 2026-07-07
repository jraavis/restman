# Resume notes (2026-07-07, updated same day)

## What just landed

1. `108120e` — mock server request matchers (query/header/body), path-capture
   response templating, config import/export.
2. `7c1b5dd` — plugin import/export (mirrors #1's export/import shape).
3. `44dc2a0` — CLI runner (`restman-cli`). Full writeup in `PLAN.md` under
   "CLI runner — `restman-cli` (2026-07-07)".
4. **In-editor lint feedback for scripts** — not yet committed. Writeup in
   `PLAN.md` under "In-editor lint feedback for scripts (2026-07-07)".
   One file changed: `src/lib/monaco.ts` (turns on real semantic JS
   diagnostics + adds a `pm.*` ambient typing, shared by the Scripts tab and
   the plugin-source editor since Monaco's JS language service has no
   per-model diagnostics toggle).

#1–#3 committed; #4 is not — ask the user before committing it.

## Verified this session

- CLI runner: `cargo test --lib` 425/0/4, `cargo test --bin restman-cli`
  4/4 new, `cargo clippy` 12 warnings (baseline, zero new), live-verified
  against the user's real DB and a real network call (see PLAN.md for
  detail).
- In-editor lint: `npx tsc --noEmit` clean, `npx vitest run` 255/255
  unchanged. **Live-verified in a real browser** (not just reasoned about)
  by RPC-calling the actual bundled `monaco-editor`'s TS worker directly —
  confirmed a `pm.tets(1)` typo and a genuinely undeclared identifier both
  produce real diagnostics, while correct `pm.*`/`$guid` usage and a
  realistic plugin script produce zero. Caught and fixed a real gap
  mid-verification: `noSemanticValidation: false` alone did nothing for
  plain `.js` models until `compilerOptions.checkJs` was also set — found
  via the live RPC probe, not assumed from docs.

## Heads up

Nothing uncommitted besides item #4 above. No other pending/unrelated
changes.

## Still open (from PLAN.md's follow-up backlog)

1. ~~CLI runner~~ — closed (`44dc2a0`).
2. ~~In-editor lint feedback~~ — closed this session (not yet committed).
3. User-suppliable test-run samples (CollectionRunner's data-driven-run
   field — let users grab/insert a starter CSV/JSON sample instead of
   guessing the shape from a placeholder string). Still open, no blocker.
4. GraphQL docs explorer: cursor-position-aware insert (currently always
   appends to the end of the query instead of the editor's caret). Still
   open, no blocker.
5. Cross-platform release follow-ups — durable keypair + v0.1.0 publish
   already closed; likely just non-macOS `cargo tauri build` verification
   remains, which this sandbox can't do (needs non-macOS hardware).

Items 3 and 4 are genuinely actionable next steps with no external blocker.

## Process reminders (carried forward, still true)

- Don't commit until explicitly asked.
- When an advisor flags an "untested but probably fine" edge case, write
  the test/check and verify — don't reason it away. This session: the pm.*
  typings looked obviously right from the placeholder doc-comment text
  alone, but a live RPC probe against the real Rust `build_assertion_chain`
  caught that `.true`/`.false`/etc. are callable functions, not properties
  — would have shipped a subtly-wrong typing (silent false negatives) on
  inspection alone. Same pattern for the `checkJs` gap — `tsc --noEmit`
  passing on the extracted `.d.ts` string proved the *syntax* was valid,
  not that Monaco's runtime would actually use it; only the live browser
  RPC caught that gap.
- `grep`/`awk`/`sed`/`cat`/`find` are blocked via Bash in this repo
  sometimes (a pre-tool-use hook intercepts them inconsistently) — default
  to `mcp__codedb__codedb_search`/`codedb_symbol`/`codedb_outline`/`Read`,
  and don't assume a blocked command will stay blocked or a working one
  will keep working across calls in the same session.
- When a component/dialog is unreachable in the plain-`vite`
  `Claude_Preview` sandbox (no Tauri IPC, no active workspace), a targeted
  `preview_eval` probe against the underlying library directly (e.g. import
  the exact module Vite resolves via `/@id/<pkg>`, drive its real API,
  inspect real return values) can still give genuine live verification even
  when the app's own UI can't be reached — used this session to verify
  Monaco/TS-worker diagnostics behavior end-to-end without ever seeing the
  Scripts tab render.
- This sandbox DOES have real crates.io network access for `cargo add`/
  `cargo build` (confirmed in the CLI-runner session) — see
  [network-sandbox-limitation](../../.claude/projects/-Users-raavi-dev-restman/memory/network_sandbox_limitation.md)
  memory, which was corrected after a live network call from a compiled
  Rust binary succeeded this session too, contradicting its older claim.
