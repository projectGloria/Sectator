# AGENTS.md — Sectator

Local-first Windows desktop torrent meta-search engine: Tauri v2 + Rust (`tokio`) backend, React/TypeScript/Vite/Tailwind frontend, fanning out to curated, DHT-index, and Torznab sources, ranking deduplicated results, with a built-in libtorrent downloader.

## Commands
- Native deps (once, from `src-tauri/`): `vcpkg install --triplet x64-windows-static-md --x-install-root=vcpkg_installed` — libtorrent/Boost/OpenSSL from `src-tauri/vcpkg.json`; `build.rs` panics without them (`SECTATOR_VCPKG_INSTALLED` overrides the root)
- Run / Dev: `npm run tauri dev`
- Test (all): `npm run test:rust` (= `cargo test --manifest-path src-tauri/Cargo.toml`)
- Test (single): `cargo test --manifest-path src-tauri/Cargo.toml <test_name>`
- Typecheck: `npm run typecheck`
- Build frontend: `npm run build`; app binary: `npx tauri build --no-bundle` (installer is packed by GloriaSetup in CI)
- Version: `npm run set:version -- <x.y.z-alpha.N>` / `npm run check:version`

## Architecture
- `src-tauri/src/lib.rs` — builds `Arc<Store>` + `Arc<Engine>` from stored settings, prunes cache (30 days), wires the offline FTS source, registers IPC handlers.
- `src-tauri/src/engine/runner.rs` — concurrent fan-out, cancellation, circuit breaker, cache replay (`Origin::Cache`) and offline fallback (`Origin::Offline`).
- `src-tauri/src/commands/` — thin IPC wrappers only; logic lives in `engine/`, `storage/`, `scraper/`, `clients/`.
- `src-tauri/src/clients/` — qBittorrent WebUI and Transmission RPC push/test; `clients/download/` is the built-in downloader (`Downloader`: lazy session, fast-resume files in `<appdata>/downloads`, seed-ratio stop) over the `cxx` bridge `ffi.rs` ↔ `src-tauri/cpp/lt_bridge.{h,cpp}`.
- `src-tauri/src/storage/` — SQLite: FTS5 torrent cache (`cache.rs`), history/presets/bookmarks/indexer health (`library.rs`), settings (`settings.rs`).
- `src/App.tsx` + `src/hooks/useSearch.ts` — UI shell and streaming search state; typed IPC wrappers in `src/lib/api.ts`, payload types in `src/lib/types.ts`.
- `.github/workflows/release.yml` — from `GloriaSetup/templates/release.yml`; runs only via `gh workflow run release.yml` (/monjita-github-release). Shared `check` and `release` jobs come from GloriaSetup; only the `build` job (npm ci, vcpkg cache, tests, `tauri build --no-bundle`, stage `sectator.exe`) is Sectator's own.
- **Version Manifests:** `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, `gloria.toml` (synced by `scripts/version.mjs`).

## Personal Rules (Brain)
- **Git Branching:** Never create new branches or git worktrees. Always work directly on `main` until explicitly told otherwise.
- **Versioning (Minimal Changes):** When runtime code or packaging changes, bump the alpha number by `+1` (e.g., `0.1.0-alpha.1` -> `0.1.0-alpha.2`) across all version manifests in the repo **at most once per uncommitted change set** (check `git diff HEAD` first—if the working tree version is already ahead of `HEAD`, do not bump again until committed). Skip version bumps on docs-only or read-only tasks.
- **Versioning (Major/Minor Changes):** Before changing minor/major versions or dropping `-alpha` on breaking or major feature work, stop and ask for the exact target version.
- **Commit Format:** Write one plain sentence stating what changed and why; do not use `feat:`/`fix:`/`chore:` conventional-commit prefixes.
- **Commit & Push Policy:** Never run `git commit`, `git tag`, or `git push` unless explicitly asked; never force-push to `main`. When the user says "commit and push", verify the build and tests pass, confirm version manifests are bumped and synced for code/packaging changes, commit on `main`, and push to `origin main` (no manual release tags—pushing to `main` triggers `.github/workflows/release.yml` to build with `gloria-setup` and publish to GitHub Releases).
- **Path Privacy:** Always use `C:\Users\you` in documentation, comments, and examples—never hardcode the local machine's real username.
- **Release Workflow Check:** Before pushing to `main`, check `.github/workflows/release.yml` against `.brain/github-actions.md` (runs on `push: branches: [main]`, builds and releases only when the version changed). If it's missing or set up differently, bring it in line in the same push and tell the user what changed.
- **Window Chrome:** Disable native OS titlebars; build an integrated frameless titlebar with drag region, double-click maximize, and custom min/max/close controls.
- **Desktop Shell Lock:** Lock the app shell to `100vw x 100vh` (`overflow: hidden`) with internal scroll panes and `user-select: none` on UI chrome.
- **Scrollbars:** Ban raw OS scrollbars; use thin (4–6px) themed scrollbars with transparent tracks and zero layout shift.
- **Interactive Controls:** Custom-style all form controls; define explicit `hover`, `active`, `focus-visible`, and `disabled` states with snappy 120–180ms transitions.

## Playbooks (Read Before Task)
- **Building & Packaging:** Read `.brain/building-the-project.md` before packaging, compiling release binaries, or building an installer.
- **GitHub Actions & Releases:** Read `.brain/github-actions.md` before creating/modifying `.github/workflows/*.yml`, setting up release automation, or preparing a first release push.

## Repo Guardrails & Gotchas
- **Phased Build:** Follow the spec's phases (scaffold -> engine/parser -> runner/indexers/scraper -> UI -> persistence/release). After Phase 1 (layout, Cargo deps, core models, FTS5 schema, frameless window config) stop for approval before writing implementation code; never dump the whole codebase in one pass.
- **No Fake Implementations:** Never mock or hardcode search results; every indexer, parser, deduplicator, and scraper must be genuinely functional and backed by Rust unit tests.
- **No DHT Crawler:** Never build a local BEP-51 crawler daemon; query external DHT indexes (e.g., BTDig, BT4G) instead.
- **No Headless Browser:** HTML indexers use `reqwest` + `scraper`/`quick-xml` with configurable mirror fallback lists.
- **Local-First:** Zero telemetry, analytics, or cloud accounts; cache, history, bookmarks, and config live only in local SQLite/disk.
- **Rust Module Boundaries:** Keep backend modules strictly split into `indexers/`, `engine/`, `parser/`, `scraper/`, `storage/`, `clients/`, `commands/`; keep IPC payloads lean and strongly typed.
- **Streaming Fan-Out:** Run indexers concurrently via `tokio::JoinSet`, stream each batch over a Tauri `Channel` as it arrives, cancel superseded searches, enforce per-indexer timeouts (clearnet 4–6s, Tor 10–12s) and a circuit breaker; never block the UI on the slowest source.
- **Proxy Routing:** Proxies are per-indexer (`socks5h://` for remote DNS, e.g., Tor at `127.0.0.1:9050`); never force clearnet indexers through Tor.
- **InfoHash Normalization:** Normalize BTIH to lowercase 40-char hex (Base32 -> hex; handle v2/hybrid) before dedup, then merge trackers, file lists, and reporting indexers per hash.
- **Results Table:** Virtualize with `@tanstack/react-virtual`; dark dense UI with monospace numerals for sizes, peers, latency, and infohashes.

## Done Criteria
- Rust unit tests pass for every touched `engine`/`parser`/`indexers`/`scraper`/`storage` module; no mocked results anywhere.
- `npm run test:rust` and `npm run build` pass; `npm run check:version` shows all four Version Manifests in sync, bumped once per change set.

<!-- brain-sync: fd4fe4022083 -->
