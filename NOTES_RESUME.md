# Resume notes (2026-07-07, updated same day)

## What just landed

1. `108120e` — mock server request matchers (query/header/body), path-capture
   response templating, config import/export.
2. `7c1b5dd` — plugin import/export (mirrors #1's export/import shape).
3. **CLI runner (`restman-cli`)** — not yet committed. Full writeup in
   `PLAN.md` under "CLI runner — `restman-cli` (2026-07-07)". New files:
   `src-tauri/src/runner.rs` (shared execution core, extracted from
   `commands/scripting.rs`), `src-tauri/src/bin/restman-cli.rs` (the CLI
   itself, auto-discovered by Cargo — no `[[bin]]` section added). `lib.rs`
   flipped `store`/`model`/`error` from `mod` to `pub mod` (plus new
   `pub mod runner`) so the CLI, as a separate bin target linking
   `restman_lib`, can reach them. New deps: `dirs` (zero-cost — already
   transitive via `tauri`), `clap` (derive, real new small tree).

Both #1/#2 committed; #3 (CLI runner) is not — ask the user before
committing it, per this repo's "don't commit until explicitly asked" rule.

## Verified this session

- `cargo test --lib` 425/0/4 (unchanged — no tests moved/lost).
- `cargo test --bin restman-cli` 4/4 new (id/name resolver logic).
- `cargo clippy --lib --all-targets --quiet` 12 warnings, byte-identical
  baseline, zero new.
- `cargo check --all-targets` clean, `cargo build --release --bin
  restman-cli` clean, `npx tsc --noEmit` clean.
- **Live-verified for real** (not just compiled): ran `restman-cli` against
  the user's actual on-disk DB (`~/Library/Application Support/
  com.restman.app/restman.db`) and a real network call — "Seeya Workspace" →
  "Seeya Collection" produced a real 403 + a real post-response-script
  error, correct exit code 1, correct JUnit/JSON export content
  (spot-checked). Also checked error paths: bad workspace name, bad
  collection name, missing `--db` path — all clean, actionable messages.

## Heads up: unrelated pending change

`src/components/Watermark.tsx`'s repositioning was committed in `dbf80dd`
since the last resume note was written — no longer a pending/uncommitted
item. Nothing else uncommitted besides the CLI runner work above.

## Still open (from PLAN.md's "Still open from this list")

1. ~~CLI runner~~ — closed this session (not yet committed — see above).
2. Cross-platform release follow-ups — durable keypair + v0.1.0 publish
   already closed; likely just non-macOS `cargo tauri build` verification
   remains, which this sandbox can't do either (needs non-macOS hardware).

No other open items from the roadmap — this may be the last "Still open"
entry to close out of the original plan's follow-up backlog.

## Process reminders (carried forward, still true)

- Don't commit until explicitly asked.
- When an advisor flags an "untested but probably fine" edge case, write the
  test and check — don't reason it away. This session: the CLI's id/name
  resolver got 4 real unit tests (exact-id-wins-over-name-collision,
  case-insensitive name, not-found, ambiguous) rather than being trusted
  on inspection alone.
- `grep`/`awk`/`sed` are blocked via Bash in this repo sometimes (a
  pre-tool-use hook intercepts them inconsistently — occasionally lets a
  `grep` through, most often blocks it) — default to `mcp__codedb__codedb_
  search`/`codedb_symbol`/`codedb_outline` or `Read` instead, and don't
  rely on `grep` working just because it worked once earlier in a session.
- Before assuming a Tauri-internal behavior (like `app_data_dir()`'s exact
  path formula), read the vendored crate source in
  `~/.cargo/registry/src/*/tauri-<version>/` rather than guessing from
  memory/docs — confirmed this session for `app.path().app_data_dir()` ==
  `dirs::data_dir().join(identifier)`.
- This sandbox DOES have real crates.io network access for `cargo add`/
  `cargo build` (confirmed this session) — the previously-recorded
  "network_sandbox_limitation" memory is narrower than it sounds: it's
  specifically about test binaries reaching the network *at runtime*
  (e.g. live HTTP requests inside `cargo test`), not Cargo's own registry
  index fetching. Don't avoid `cargo add` for genuinely new deps out of
  caution — dry-run it first to confirm resolution, then add for real.
- `form_urlencoded` added as a direct Cargo dep — already transitive via
  `reqwest`→`url`, confirmed with `cargo tree -i form_urlencoded` before
  adding, so zero new crates in the build. Same pattern used again this
  session for `dirs` (`cargo add --dry-run dirs` before adding, confirmed
  already-resolved at the exact same version via `tauri`'s own dep tree).
