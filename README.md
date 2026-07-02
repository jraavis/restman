# restman

A privacy-first, offline-capable REST API client for the desktop, built with Tauri v2 (Rust + React). Every request, credential, and byte of history stays on your machine — the frontend never touches the network or disk directly; all networking, storage, and crypto live in the Rust backend behind a typed IPC boundary.

**[Download the latest release](https://github.com/jraavis/restman/releases/latest)** (macOS, universal ARM64 + Intel).

> The macOS build is not Apple-notarized. On first launch, right-click the app and choose **Open** (or run `xattr -d com.apple.quarantine /Applications/restman.app`).

## Features

**Requests & responses**
- All standard HTTP methods plus custom, with per-request timeout / redirect / SSL-verify / cookie options
- Body modes: JSON (Monaco editor with format/validate), raw, form-data with file upload, x-www-form-urlencoded, binary, GraphQL
- Response viewer: pretty (content-type-aware), raw, sandboxed HTML preview, hex; filtering, save-to-file, timing breakdown
- Request diff: compare any two history entries side by side

**Organization**
- Workspaces isolate collections, environments, history, and settings
- Collections with unlimited nesting, drag-drop, tags, and FTS5 full-text search
- Environments (workspace-global or collection-scoped, groupable) with `{{variable}}` interpolation and autocomplete; resolution priority: local → environment → collection → workspace → global
- Secret variables and all auth credentials live in the OS keychain — never plaintext in the database, never exported unmasked
- Auto-saved history with search/filter/replay and configurable retention; multi-tab editing restored across restarts

**Auth**
- Bearer, Basic, API key (header/query), OAuth 2.0 (auth code + PKCE, client credentials, password, refresh; token caching and auto-refresh), AWS Signature V4
- Collection-level auth with per-request override

**Scripting & testing**
- Pre-request / post-response scripts in a sandboxed QuickJS runtime (no filesystem or network access, 8s timeout) with a `pm.*` API
- Collection test runner: iterations, delays, CSV/JSON data-driven runs, JUnit XML / JSON export

**Protocols**
- GraphQL: schema introspection, docs explorer, schema-aware autocomplete
- WebSocket and SSE clients that honor workspace proxy / client-cert / default-header settings
- gRPC: `.proto` upload or server reflection, unary + client/server/bidi streaming

**Interop**
- Import: Postman v2.1, Postman environments, OpenAPI 3.0 / Swagger 2.0 (JSON + YAML), cURL, HAR, Insomnia, Bruno, `.http` files — with preview, conflict resolution, and partial-import reporting
- Export: Postman v2.1 (round-trips), OpenAPI 3.0, HAR, cURL
- Code generation for 9 languages (cURL, JS fetch, Python, Go, Rust, PHP, Java, C#, Ruby)
- JS plugins for custom codegen and import/export formats, run in the same sandbox as scripts

**Local tooling & data**
- Mock servers: rule-based (method + `:param` path patterns, status/headers/body/delay), creatable from a collection, served on localhost
- File-based `.restman/` folder sync (JSON or YAML, manual or live auto-export) for git-friendly sharing — always secret-redacted
- Encrypted ZIP backup/restore (AES-256, password required) covering every workspace *including* keychain secrets, for disaster recovery
- Command palette (Cmd+K), remappable keyboard shortcuts, full settings, cookie jar viewer, auto-updates

## Development

Prerequisites: [Rust](https://rustup.rs), Node.js 22+, and the [Tauri v2 system dependencies](https://v2.tauri.app/start/prerequisites/).

```sh
npm install
npx tauri dev        # run the app
```

Tests and checks:

```sh
cargo test           # Rust suite (run from src-tauri/)
npx vitest run       # frontend suite (run from the repo root)
npx tsc --noEmit     # type-check
cargo clippy --all-targets
```

## Architecture

```
React 19 + TS (Vite, Tailwind 4, Monaco, Zustand, TanStack Query)
        │  Tauri IPC — typed wrappers in src/lib/ipc.ts
Rust backend — owns ALL network / file / crypto work
  engine/ (HTTP, WS, SSE, gRPC, mock)   store/ (SQLite + migrations)
  auth/ (OAuth2, SigV4)   scripting/ (QuickJS)   interop/   codegen/
```

- Storage is a single SQLite database (bundled, WAL mode) with an append-only migration runner; search runs on FTS5.
- Secrets follow a mask-on-write contract: real values go to the OS keychain, the database and every export only ever see a mask.
- `PLAN.md` is the detailed build log: per-phase scope, decisions, verification results, and open follow-ups.

## Releasing

Bump `version` in `src-tauri/tauri.conf.json`, then:

```sh
git tag v<version> && git push origin v<version>
```

GitHub Actions (`.github/workflows/release.yml`) builds a universal macOS bundle, publishes the release, and uploads the signed updater artifacts (`latest.json` + `.app.tar.gz.sig`) that installed apps check via **Settings → About → Check for updates**. Updater artifacts are signed with the Tauri updater key (`TAURI_SIGNING_PRIVATE_KEY` repo secret); Apple code signing is not used.
