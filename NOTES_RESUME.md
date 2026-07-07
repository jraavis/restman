# Resume notes (2026-07-07, updated same day)

## What just landed

1. `108120e` — mock server request matchers, path-capture templating, config import/export.
2. `7c1b5dd` — plugin import/export.
3. `44dc2a0` — CLI runner (`restman-cli`).
4. `05a208a` — in-editor lint feedback for pre/post-request scripts.
5. **GraphQL docs-explorer caret-aware insert** — not yet committed. Writeup
   in `PLAN.md` under "GraphQL docs-explorer caret-aware insert
   (2026-07-07)". One file changed for the feature
   (`src/features/request/BodyEditor.tsx`), one test file extended
   (`BodyEditor.test.tsx`, +2 tests).

#1–#4 committed; #5 is not — ask the user before committing it.

## Verified this session (item 5)

- `npx tsc --noEmit` clean, `npx vitest run` 257/257 (+2 new tests using a
  fake-editor-injection harness — Monaco itself is always mocked in this
  codebase's vitest suite, same established precedent as `ScriptsTab.test.tsx`).
- **Live-verified the real Monaco API semantics** in a browser (same
  `/@id/monaco-editor` direct-driving technique from the lint-feedback
  session): a real editor with the cursor mid-query produced the field name
  spliced in at exactly that position (not appended), cursor landing right
  after it; a real active selection got replaced in place rather than
  appended past it.

## Still open (from PLAN.md's follow-up backlog)

1. ~~CLI runner~~ — closed (`44dc2a0`).
2. ~~In-editor lint feedback~~ — closed (`05a208a`).
3. ~~GraphQL caret-aware insert~~ — closed this session (not yet committed).
4. User-suppliable test-run samples (CollectionRunner's data-driven-run
   field — let users grab/insert a starter CSV/JSON sample instead of
   guessing the shape from a placeholder string). Still open, no blocker.
5. Cross-platform release follow-ups — durable keypair + v0.1.0 publish
   already closed; likely just non-macOS `cargo tauri build` verification
   remains, which this sandbox can't do (needs non-macOS hardware).

Item 4 is the last genuinely actionable, unblocked follow-up left in the
backlog as of this session.

## Process reminders (carried forward, still true)

- Don't commit until explicitly asked.
- When an advisor flags an "untested but probably fine" edge case, write
  the test/check and verify — don't reason it away. Recurring theme this
  session (pm.* typings, checkJs gap, now the caret-insert Monaco API
  semantics): a live browser probe against the real library caught things
  static reasoning alone would have missed or gotten subtly wrong.
- `grep`/`awk`/`sed`/`cat`/`find` are blocked via Bash in this repo
  sometimes (a pre-tool-use hook intercepts them inconsistently) — default
  to `mcp__codedb__codedb_search`/`codedb_symbol`/`codedb_outline`/`Read`.
- When a component/dialog is unreachable in the plain-`vite`
  `Claude_Preview` sandbox (no Tauri IPC, no active workspace), a targeted
  `preview_eval` probe against the underlying library directly (e.g.
  `import('/@id/<pkg>')`, drive its real API, inspect real return values)
  still gives genuine live verification even when the app's own UI can't
  be reached.
- No eslint/lint script configured in this repo (`package.json` has no
  `lint` script) — don't add `eslint-disable` comments, they're inert.
