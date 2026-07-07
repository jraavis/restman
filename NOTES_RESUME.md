# Resume notes (2026-07-07, updated same day)

## What just landed

1. `108120e` — mock server request matchers, path-capture templating, config import/export.
2. `7c1b5dd` — plugin import/export.
3. `44dc2a0` — CLI runner (`restman-cli`).
4. `05a208a` — in-editor lint feedback for pre/post-request scripts.
5. `8c8da96` — GraphQL docs-explorer caret-aware insert.
6. **User-suppliable test-run sample data** — not yet committed. Writeup in
   `PLAN.md` under "User-suppliable test-run sample data (2026-07-07)". New
   file `src/features/collections/runnerSampleData.ts` (+ its test file),
   `CollectionRunner.tsx` gets two new buttons.

#1–#5 committed; #6 is not — ask the user before committing it.

## Verified this session (item 6)

- `npx tsc --noEmit` clean, `npx vitest run` 267/267 (+10 new, all for the
  pure `runnerSampleData.ts` helpers — var extraction, dedup, sort,
  fallback vs. real-var-name sample output).
- **Not exercised**: the actual buttons through a real Tauri shell —
  `CollectionRunner` needs an active workspace/collection, unreachable in
  this sandbox's plain-`vite` preview, same limitation as every other
  IPC-gated dialog. The underlying logic is fully covered by unit tests
  instead (no Monaco/IPC involved in this particular feature, unlike the
  two before it, so no live-browser probe was needed this time).

## Still open — this may have been the last one

As of this session, every follow-up in PLAN.md's backlog that doesn't need
a Tauri shell or non-macOS hardware to verify is closed:

1. ~~CLI runner~~ — closed (`44dc2a0`).
2. ~~In-editor lint feedback~~ — closed (`05a208a`).
3. ~~GraphQL caret-aware insert~~ — closed (`8c8da96`).
4. ~~User-suppliable test-run samples~~ — closed this session (not yet committed).
5. Cross-platform release follow-ups — durable keypair + v0.1.0 publish
   already closed; likely just non-macOS `cargo tauri build` verification
   remains, needs non-macOS hardware this sandbox doesn't have.

If asked to "complete the next" again with no other user-specified task,
check `PLAN.md`'s "Follow-ups surfaced but not scheduled" sections fresh —
don't assume this list is still accurate, since new follow-ups may have
been added in the meantime (or ask the user what's next, since the backlog
may genuinely be empty of sandbox-actionable items).

## Process reminders (carried forward, still true)

- Don't commit until explicitly asked.
- When an advisor flags an "untested but probably fine" edge case, write
  the test/check and verify — don't reason it away. Recurring theme all
  session: pm.* typings, the Monaco `checkJs` gap, the caret-insert API
  semantics — each time a live/executed check caught something static
  reasoning alone would have missed.
- `grep`/`awk`/`sed`/`cat`/`find` are blocked via Bash in this repo
  sometimes (a pre-tool-use hook intercepts them inconsistently) — default
  to `mcp__codedb__codedb_search`/`codedb_symbol`/`codedb_outline`/`Read`.
- When a component/dialog is unreachable in the plain-`vite`
  `Claude_Preview` sandbox (no Tauri IPC, no active workspace), a targeted
  `preview_eval` probe against the underlying library directly (e.g.
  `import('/@id/<pkg>')`) can still give genuine live verification when the
  feature touches something like Monaco — but when a feature is pure logic
  with no such library, straightforward unit tests are the right (and
  sufficient) verification, no need to force a browser probe.
- No eslint/lint script configured in this repo — don't add
  `eslint-disable` comments, they're inert.
