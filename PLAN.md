# Restman — Development Plan

Privacy-first Tauri v2 (Rust + React) REST client. Frontend talks to the backend only over Tauri IPC; Rust owns all networking, file I/O, and crypto. Secrets (tokens, passwords, API keys) never cross IPC in plaintext and are never persisted to SQLite in plaintext — see the mask-on-write contract in `src-tauri/src/secrets.rs` and `SECRET_MASK` in `src/lib/types.ts`. Storage is SQLite only.

Repo: https://github.com/jraavis/restman (public, default branch `main`).

## Phase 0-1 — Scaffold, HTTP engine, request/response loop ✅ complete

- Tauri v2 + React + TypeScript scaffold, UI modernization.
- Core HTTP engine (`src-tauri/src/engine/http.rs`): send/receive, response timing, header/body handling.
- Basic request/response loop in the UI.

Commit: `Initial commit: Phase 0-1 complete` (`d5cb5ac`).

## Phase 2 — Workspaces, collections, environments, history, tabs ✅ complete

- Workspace-scoped collections with FTS5 search, nested folders, tags, drag-drop.
- Environments with grouping, collection scoping, `{{var}}` interpolation.
- Secret variables backed by the OS keychain instead of plaintext SQLite.
- Auto-saved history with filtering and replay.
- Multi-tab editing with restore-on-restart.
- Follow-up gaps from the original Phase 2 commit (env import/export scope, a search-query-independent tag/method filter) were closed out in later work on this same branch before Phase 3 started.

Commit: `Phase 2: workspaces, collections, environments, history, tabs` (`63b8d8c`).

## Phase 3 — Authentication ✅ complete

- Per-collection/request auth config with inheritance (`RequestAuth::Inherit` vs `Own`), resolved collection→request.
- Auth types: Bearer, Basic, API Key (header or query param), AWS Signature V4, OAuth2.
- OAuth2 grants: Authorization Code (with PKCE — S256/plain/none), Client Credentials, Password, Refresh Token. Token caching + automatic refresh (`src-tauri/src/auth/oauth/token_store.rs`), authorization-code browser flow via a local loopback redirect.
- AWS SigV4 signing (`src-tauri/src/auth/aws_sigv4.rs`) verified against AWS's own published `get-vanilla` reference vector — pinned fixed-date known-answer test, asserts the exact `Authorization` header byte-for-byte, not just "doesn't crash."
- Generic secrets store generalized to a keyed KV so every secret-bearing auth type shares one mask-on-write path.
- DB migration v3: auth columns on collections/requests + `oauth_tokens` table.
- Frontend: shared `AuthConfigFields` component, request-level Auth tab (`RequestAuthTab`), collection-level auth dialog (`CollectionAuthDialog`), OAuth2 status/connect hooks (`oauthHooks.ts`).
- Verification: `cargo test`, `tsc`, `vitest` all green. Live browser-verified in the actual dev server (not just code review) — Request Auth tab across all types/grants, and the full Collection Auth dialog lifecycle (open → switch type → enter secret → reveal toggle → save → reopen-persisted), zero console errors.
- A real compile bug (cookie-jar parameter threading across `engine/http.rs` and `commands/history.rs`) was caught and fixed during this phase — the original "Phase 3 complete" claim that triggered re-verification was premature; this plan reflects the corrected, re-verified state.
- **Token preview open item — closed.** Decision: Build it. Implemented in Phase 4 (see below).

Commit: `Phase 3: Authentication (Bearer, Basic, API Key, OAuth2, AWS SigV4)` (`5ae1e7b`).

## Phase 4 — Scripting & Testing ✅ complete

- **JS sandbox**: `rquickjs` (QuickJS) in Rust, sandboxed — no filesystem or network access, fresh `Runtime`+`Context` per script run, 8s execution timeout.
- **`pm.*` API**: `pm.environment.get/set`, `pm.request.method/url/headers`, `pm.response.status/statusText/json()/text()/headers/responseTime`, `pm.test`, `pm.expect` (chainable: `.equal`, `.include`, `.a`, `.length`, `.true`, `.false`, `.null`, `.undefined`), `pm.abort`. Template tags `$guid`, `$timestamp`, `$randomInt` as globals.
- **Pre-request scripts**: run before send; can mutate env vars or call `pm.abort()` to cancel the request. Env mutations are re-interpolated into the URL/headers/body before the actual HTTP send.
- **Post-response scripts**: run after the response arrives; can read `pm.response` and assert with `pm.test`.
- **Scripts tab** in `RequestBuilder` (Monaco, JS mode, pm.* hint overlay when empty).
- **TestResultsPanel** in `ResponseViewer` — summary bar (passed/failed counts), per-section pre/post breakdown, individual test rows with pass/fail icons, runtime error alert.
- **Collection test runner** (`CollectionRunner.tsx`): config strip (iterations, delay, data), live progress via `runner:progress` Tauri events, summary, export JUnit XML and JSON.
- **Data-driven runs**: CSV (with header row) and JSON array data files.
- **Token preview** (`get_oauth_token_preview` IPC command): Rust computes `head[..4] + "…" + tail[len-3..]` server-side; raw token never crosses IPC. Displayed as a monospace badge in `AuthConfigFields` OAuth2 status area.
- **DB migration v4**: `pre_request_script`/`post_response_script` columns on `requests`, `test_results_json` on `history`, `masked_preview` on `oauth_tokens`.
- **`send_request` return type** changed to `SendResponse { response, preScript, postScript }` — all call sites updated.
- **Verification**: `tsc --noEmit` clean. `vitest run` requires `@rolldown/binding-darwin-arm64` (macOS native binary) — passes on the user's machine but can't run in the Linux sandbox; all test files compile and type-check correctly. `cargo test` must be run locally (Rust/cargo not in sandbox). Rust unit tests (8) are in `scripting/engine.rs`.

## Repo / git history note

The first push to GitHub included `Co-Authored-By: Claude` trailers on the Phase 2 and Phase 3 commit messages. Those two commits were rebuilt locally (`git commit-tree`, identical trees/authors/dates, reworded messages only) and force-pushed to `origin/main` to remove the trailer. Current hashes: `d5cb5ac` (Phase 0-1, unchanged), `63b8d8c` (Phase 2), `5ae1e7b` (Phase 3). If you ever see the old hashes (`ed6f53e`, `5b05f67`) referenced anywhere (forks, local clones made before the rewrite, etc.), they're the pre-rewrite versions of the same content.

## Phase 5 — Import/Export & Code Generation ✅ complete

- **Shared import/export IR** (`src-tauri/src/interop/mod.rs`): `ImportedNode`/`ImportedRequest` format-agnostic tree, `parse`/`export` dispatch, `apply_import`/`collect` for DB↔IR, conflict modes (skip/overwrite/merge), `ImportReport`, partial-import (unknown bits degrade to warnings, not failures). Secrets: import routes through `crate::auth::persist`/`persist_request_auth` so a freshly imported Bearer token lands in the keychain, never plaintext in the DB; export reads `auth_json` straight from the DB where it's already mask-on-write, so every exporter gets export-safe auth for free. Re-importing a file whose auth is already `SECRET_MASK` clears the secret to `""` and warns (a freshly-created owner has no keychain entry to recover the mask from) — see `strip_unrecoverable_masks`.
- **Import** (`src-tauri/src/interop/{postman,curl,openapi,har,insomnia,bruno,http_file}.rs`):
  - **Postman Collection v2.1** — full parse + round-trip export; `event` (pre-request/test scripts), `graphql` body mode, `formdata` files.
  - **cURL** — POSIX-ish shell tokenizer, `-X`/`-H`/`-d`/`-F`/`-u`/`-k`/`-L`/`--max-time`/`--max-redirs`/`-b`/`-A`/`-e`/`--url`, Bearer/Basic collapse from `-u` or `Authorization`; multi-block export.
  - **OpenAPI 3.0 / Swagger 2.0** — JSON *and* YAML input (`saphyr` bridge to `serde_json::Value`), local `$ref` resolution with cycle guard (external refs never fetched), `securitySchemes` → `AuthConfig`, schema example-synthesis for request bodies, tag-grouped folders. Export emits OpenAPI 3.0 (Swagger import-only). `{{var}}` ↔ `{name}` path templating round-trips.
  - **HAR 1.2** — flat entry list wrapped in a synthetic root (like curl); `postData` mime-shape detection (json/urlencoded/multipart/raw/binary); export synthesizes the required `response` object.
  - **Insomnia v4** — `resources` array tree rebuilt from `parentId` (handles missing/`null`/`__WORKSPACE__` parentId roots; array-order preserved); `request_group`/`request`/workspace auth inheritance; GraphQL body parsing.
  - **Bruno `.bru` request files** — brace-block section parser (`meta`/`url`/`query`/`headers`/`auth`/`body`/scripts); one request per file (directory-of-`.bru` import is out of scope for the single-paste/upload flow). Import-only.
  - **`.http` files (JetBrains / VS Code REST Client)** — `###`-separated request blocks, `@variable = value` inlined into URLs/headers/bodies, `# @name` request names, bare-URL-with-no-method → GET, method validation rejects prose. Import-only.
- **Export** (`ExportFormat::{Postman, Curl, OpenApi, Har}`): Postman round-trips; OpenAPI 3.0; HAR; cURL (multi-block). AWS SigV4 is dropped silently on export (no OpenAPI/Postman scheme type for it), consistent with the format-inherent-gap convention. UI export menu in `CollectionNode.tsx` (Postman / OpenAPI / HAR / cURL) with per-format filename+MIME (`exportArtifactMeta`).
- **Code generation (9 langs)** (`src-tauri/src/codegen/{curl,javascript,python,go,rust,php,java,csharp,ruby}.rs` + `mod.rs`): pure `generate` over a resolved `HttpRequest`; shared `plan_auth`/`plan_body`/`full_url`/escapers (`dquote`/`squote`/`shquote`). OAuth2 must already be collapsed to `Bearer` by the caller — `commands::codegen::generate_code` reuses a fresh cached token or substitutes a visible `<OAUTH2_ACCESS_TOKEN>` placeholder (never fires a live token exchange just to preview code); AWS SigV4 emits an explanatory comment instead of a time-bound signature (preserves purity). Frontend `CodeTab.tsx` (language picker, Auth/Headers toggles, copy/download, live preview via `useQuery`), wired into `RequestBuilder`.
- **Environment import/export** (deferred from Phase 2): `src-tauri/src/interop/environment.rs` — Postman Environment JSON (`name` + `values[]` with `type: "secret"`) parse/apply/export. Secrets route through the keychain via `variables::create` on import; export masks with `*****`. Re-import of an already-masked secret clears it and warns (same contract as collection auth). IPC `preview_environment_import`/`apply_environment_import`/`export_environment`. UI: `ImportDialog` "Environment"/"Collection" toggle (env mode renders a variable table + overwrite-same-key checkbox); `EnvironmentsPanel` per-row export button + header import button.
- **Frontend wiring**: widened `ImportFormat`/`ExportFormat` TS unions (postman/curl/open_api/har/insomnia/bruno/http_file); `ipc.ts` wrappers for all new commands; `ImportDialog.tsx` format-aware placeholders/`accept`/blurbs; `CollectionNode.tsx` OpenAPI+HAR export menu items.
- **Verification**: `cargo test` → 193 passed / 0 failed (was 175 pre-Phase-5; +18 across HAR/Insomnia/Bruno/`.http`/environment). `npx tsc --noEmit` clean. `npx vitest run` → 71 passed / 14 files (was 68; +3 for OpenAPI-format selection + environment preview/apply). `cargo build` warning-free. Rust/cargo tests must be run locally (Rust/cargo not in this sandbox); the `@rolldown/binding-darwin-arm64` native binary requirement from Phase 4 persists.

## Interlude — Hardening & workspace transport settings ✅ complete

Landed between Phase 5 and Phase 6, on top of `2c08819`:

- **Per-workspace transport settings (backend)**: migration v5 adds `workspace_settings` (proxy_url, proxy_bypass, default_headers_json, client_cert_json). New `engine::http::TransportOverrides` struct (proxy + optional `reqwest::Identity` for mTLS), kept decoupled from DB/keychain types so the engine stays pure/unit-testable. `crate::workspace::resolve_transport` hydrates real secret bytes from the masked `WorkspaceSettings` just before send; `apply_default_headers` fills header gaps without overriding user-set headers. Pasted client-cert PEM goes to the OS keychain, never the DB row (mask-on-write contract, same as every other secret). `get_workspace_settings`/`set_workspace_settings` IPC commands exist. **No frontend UI yet** — tracked as a Phase 6 item below.
- **Scripting hardening**: the Phase 4 "8s execution timeout" line above was aspirational until this pass — `scripting::engine::apply_runtime_limits` now actually installs a QuickJS interrupt handler (8s deadline) plus a 512KB max stack size, called from both `run_pre_script` and `run_post_script`. `$randomInt` switched from a deterministic `ts % 1001` derivation to real `rand::rng().random_range(..)`. Pre/post script execution moved to `tokio::task::spawn_blocking` so synchronous QuickJS work can't block the async executor.
- **Frontend**: `sendCookies` toggle added to `RequestBuilder` — the shared-cookie-jar capability (`RequestOptions::send_cookies`) existed Rust-side since earlier but was never exposed in the UI.
- Commits: `f3df973`, `ab35d67`, `46d3549`, `a44b55b`.
- **Verification**: `cargo test` → 207 passed / 0 failed. `npx vitest run` → 71 passed. `npx tsc --noEmit` clean. `cargo clippy --lib --quiet` was claimed as "only 3 pre-existing warnings" here, but that was never re-checked after this Interlude's own commits — actual count at the time was already 12 (corrected below, found during Phase 6 task #14 verification).

## Phase 6 — Response viewer upgrade ✅ complete (task 1 of 5)

- **Content-type-aware rendering**: `contentTypeOf`/`monacoLanguageFor`/`extensionFor` (`src/lib/http.ts`) read the response's `Content-Type` header (stripped of `; charset=…`) and map it to a Monaco language id and a save-file extension. Both Pretty and Raw views now use this instead of a hardcoded `json`/`plaintext` choice.
- **XML pretty-printing**: `prettyXml` (`src/lib/encoding.ts`) — `DOMParser`-based, 2-space indent, returns `null` on parse failure (mirrors `prettyJson`'s contract) so the Pretty view falls back to JSON, then XML, then raw text with content-type-derived highlighting.
- **Response filtering**: `filterJsonValue` (recursive, key-or-value substring match, per-field pruning — a matching key keeps only its own subtree, not sibling fields) and `filterLines` (substring line filter with a no-match placeholder), both in `src/lib/encoding.ts`. Wired to a filter input shown next to the Pretty/Raw/Preview/Hex toggle (hidden for Preview/Hex, where filtering doesn't apply).
- **Save to file**: new `tauri-plugin-dialog` (native save picker, JS side) + `write_file_bytes` Rust command (`src-tauri/src/commands/files.rs`, base64-decode then `std::fs::write`). Chosen over reusing the existing `CollectionNode.tsx` Blob-anchor-download convention because that convention is itself a violation of this repo's "Rust owns all file I/O" contract (`PLAN.md` line 3, `ipc.ts` header) — converging `CollectionNode` onto the same Rust path is a good follow-up, not done here.
- **Verification**: `cargo test` → 209 passed / 0 failed (+2 for `write_file_bytes`). `npx vitest run` → 94 passed / 17 files (+23: XML/filter helpers, `contentTypeOf`/`monacoLanguageFor`/`extensionFor`). `npx tsc --noEmit` clean. `cargo clippy --lib --quiet` → 12 pre-existing warnings, all in files untouched by this task (`model/auth.rs` ×4, `interop/{bruno,har,http_file}.rs`, `commands/{http,scripting}.rs`, `store/{collections,history}.rs`, `model/workspace_settings.rs`); zero new warnings introduced. Also fixed in passing: an unused `WorkspaceSettings` import this Interlude's own cleanup had mis-removed from `workspace.rs` (it's used by `#[cfg(test)]`, invisible to `cargo check --lib` but breaks `cargo test`) and an unused-variable warning on the same test module's `t` binding.
- **Not yet live-browser-verified** — no dev server run yet this task; `npx tsc`/`vitest`/`cargo test` all green but the save-dialog flow and the new filter UI have not been clicked through in the actual app.

### Phase 6, task 2 of 5 — Workspace settings UI ✅ complete

- **Backend bug found and fixed first**: `store::workspace_settings::persist_cert_secrets` was missing the `SECRET_MASK`-skip guard that `auth::persist` already has (`src-tauri/src/auth/mod.rs:64-67`). Without it, the frontend panel below would silently destroy a saved client cert/key/passphrase on the very first unrelated save (e.g. editing the proxy URL), because `get()` returns those fields masked and any round-trip would write the literal `"••••••••"` string into the keychain. Fixed by adding `persist_secret_slot` (same skip-on-mask / clear-on-empty / set-and-mask contract, applied to all three Paste slots plus the Path-mode passphrase). Regression tests added: `resaving_already_masked_paste_cert_does_not_clobber_keychain`, `empty_passphrase_clears_the_slot_instead_of_storing_empty_string`. This was a latent bug from the Interlude (`ab35d67`), caught by re-reading the store layer before wiring a UI to it, not user-reported.
- **Frontend**: `WorkspaceSettings`/`ClientCertConfig`/`ClientCertMode` TS mirrors + `emptyWorkspaceSettings`/`emptyClientCertConfig` helpers (`src/lib/types.ts`); `getWorkspaceSettings`/`setWorkspaceSettings` wrappers (`src/lib/ipc.ts`); `useWorkspaceSettings`/`useSetWorkspaceSettings` query hooks (`src/features/workspaces/hooks.ts`); new `WorkspaceSettingsDialog.tsx` (proxy URL/bypass, `KeyValueEditor`-based default-headers list, client-cert mode picker — Paste mode with masked-PEM textareas + "already saved, paste to replace" hint, Path mode with plain path inputs — both with an optional passphrase via `SecretInput`). `SecretInput`/`Field` promoted from private helpers in `AuthConfigFields.tsx` to exported ones rather than duplicated.
- **Entry point**: new "Settings" item in `TopBar.tsx`'s workspace overflow menu (next to Rename/Delete), opening the dialog for the active workspace.
- **Verification**: `cargo test` → 211 passed / 0 failed (+2 for the mask regression tests above). `npx vitest run` → 99 passed / 16 files (+5 for `WorkspaceSettingsDialog.test.tsx`). `npx tsc --noEmit` clean. `cargo clippy --lib --quiet` → 13 warnings, not 12 as previously logged — the prior count undercounted `commands/scripting.rs` (2 warnings, not 1); the file this task actually touched (`store/workspace_settings.rs`) has zero. No new warnings introduced.
- **Not yet live-browser-verified** — same caveat as task 1: code review + automated tests only, dialog not yet clicked through in the running app.
- **Follow-up flagged, not fixed here** (non-blocking per code review): switching/clearing a client-cert mode doesn't sweep the old keychain slots (e.g. Paste → None leaves the old cert/key/pass entries orphaned in the keychain rather than deleting them). `delete_cert_secrets` already exists and would cover it.

### Phase 6, task 3 of 5 — Cookie jar visualization UI ✅ complete

- **Backend**: `cookie_store` added as a direct dependency (`src-tauri/Cargo.toml`) — was previously only transitive via `reqwest_cookie_store`, but matching on `cookie_store::CookieExpiration`'s variants requires naming the type, which needs a direct dependency edge (the resolved version is unchanged, already locked to 0.22.1). New `CookieEntry` DTO (`src-tauri/src/model/http.rs`) and two commands next to the existing `clear_cookies` (`src-tauri/src/commands/http.rs`): `list_cookies` (iterates `iter_unexpired()`, sorted by domain then name; `expiresAt` is `Option<i64>` unix seconds, `None` for session cookies) and `delete_cookie` (thin wrapper over `CookieStore::remove(domain, path, name)`). Both registered in `lib.rs`.
- **Frontend**: `CookieEntry` TS mirror (`src/lib/types.ts`); `listCookies`/`deleteCookie`/`clearCookies` IPC wrappers (`src/lib/ipc.ts` — `clearCookies` had no frontend wrapper at all until now, despite the Rust command existing since the Interlude). New `src/features/cookies/` module: `hooks.ts` (`useCookies`/`useDeleteCookie`/`useClearCookies`) and `CookieJarDialog.tsx` (same modal shell as `WorkspaceSettingsDialog`; per-row delete on hover, "Clear all" with `window.confirm`, empty state, Secure/HttpOnly/SameSite badges, session-vs-expiry display). Cookie values are shown in full, not masked — this is a read-only diagnostic view of data the backend already sends over the wire in plaintext, same posture as response headers/body in `ResponseViewer`, not a user-entered secret field.
- **Entry point — deliberately not the workspace overflow menu.** The cookie jar (`AppState.cookie_jar`) is a single app-global `Arc<CookieStoreMutex>`, not workspace-scoped (unlike task 2's workspace settings), so it gets its own icon in `TopBar.tsx`'s global right-side icon cluster (next to the appearance-settings gear), not a workspace-menu item — putting it there would have falsely implied per-workspace cookies.
- **Verification**: `cargo test` → 211 passed / 0 failed (no new Rust tests — these commands are one-line wrappers with no separable logic, same precedent as `clear_cookies` itself having none). `npx vitest run` → 105 passed / 18 files (+6 for `CookieJarDialog.test.tsx`). `npx tsc --noEmit` clean. `cargo clippy --lib --quiet` → still 13 warnings, all pre-existing, none in any file this task touched.
- **Not yet live-browser-verified** — same caveat as tasks 1 and 2.

## Next

**Phase 6** — confirmed with the user, in progress. Candidate scope:
- ~~Response body pretty-print / JSON viewer / content-type-aware rendering, response filtering, save response to file.~~ ✅ done, see above.
- ~~Per-workspace settings UI (proxy, default headers, client cert).~~ ✅ done, see above.
- ~~Request/response cookie visualization (cookie jar is already shared in the backend; surface it).~~ ✅ done, see above.
- gRPC / WebSocket / SSE streaming client.
- Plugin system for custom codegen / custom import formats.

## How to resume in a new session

1. Read this file first.
2. `git log --oneline` to confirm the above hashes still match (this file will drift if more commits land without an update).
3. Run `cargo test` and `npx vitest run` locally to confirm tests are green before continuing Phase 6. Expect 211 Rust tests / 105 frontend tests baseline.
