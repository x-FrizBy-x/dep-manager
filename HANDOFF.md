# UniManage — Session Handoff

Spec: Universal Software & Dependency Manager (Tauri 2 + Rust + React/TS/Tailwind/shadcn + SQLite). Linux first (Ubuntu/Debian). Decisions made with the user: install Rust and scaffold; **start read-only**; keep project dependencies separate from global packages; Linux is the first platform.

## Environment notes
- Dev machine: Windows 11, Node 24, npm 12 (no pnpm). Rust 1.99.1 installed via winget/rustup this session (add `%USERPROFILE%\.cargo\bin` to PATH in new shells). MSVC (VS 2022 Community) present.
- WSL2 Ubuntu (26.04) is available: has `snap`, `npm`, `apt`; no flatpak/mise/pipx/uv. Rust is NOT installed in WSL.
- Shell gotcha: complex quoting/heredocs with `$`/unicode broke in Bash tool; use the Write tool for files, and copy scripts to `C:\Users\Alexa\AppData\Local\Temp` then run via `wsl -d Ubuntu -- bash /mnt/c/...`.
- TypeScript is v7 (native), Vite 8, Vitest 5, React 19, Tailwind 4 (CSS-first, no tailwind.config).

## DONE (Milestone 1: read-only core)
**Rust (`src-tauri/`)** — `cargo test --lib`: 26 tests pass; `cargo check` of full app passes.
- `model.rs`: `Package` (all spec fields; unknown = `None`), `ManagementStatus`, `UpdateStatus`, `PackageKind`, `UpdateInfo`, `ManagerInfo`.
- `exec.rs`: `Runner` trait, `SystemRunner` (no shell, arg arrays, 60s timeout, LC_ALL=C, CREATE_NO_WINDOW), `FakeRunner` for tests.
- `adapters/`: `Adapter` trait (id, display_name, binary, supports_updates, detect, version, list_installed, check_updates). Implemented: apt, flatpak, snap, mise, npm(global), pnpm(global), pipx, uv. pipx/uv have no update check (status stays unknown). Install/update/uninstall deliberately NOT in the trait yet.
- `scanner.rs`: parallel per-adapter scan, panic/failure isolation, `apply_updates`, `scan-progress` events, status completed/partial/failed.
- `db.rs` + `migrations/001_init.sql`: all 9 spec tables; migrations via `PRAGMA user_version`; stale rows kept when a manager fails; `also_found_in` overlap annotation (rows never merged).
- `commands.rs`/`lib.rs`: `start_scan` (guarded against concurrent scans), `get_packages`, `get_managers`, `get_scan_history`. DB at app data dir `unimanage.sqlite`.
- Capabilities: only `core:default` (no shell/fs plugins).

**Frontend (`src/`)** — `npm test` 6 pass, `tsc -b` clean, `vite build` OK, 1 cosmetic oxlint warning (set-state-in-effect in `hooks/useInventory.ts`).
- `lib/` (types mirroring Rust, api wrappers, filter/sort + tests), `hooks/useInventory.ts`, `components/ui/{button,badge}.tsx` (hand-written shadcn-style), `pages/Overview.tsx`, `pages/Installed.tsx` (search, filters, sort, detail panel), `App.tsx` (sidebar with 8 items, 6 disabled "Soon", theme toggle).
- Config: `vite.config.ts` (alias `@`, tailwind, vitest jsdom), `components.json`, `app-icon.svg` → generated icons.
- `README.md` has the status table.

**Verified:** APT `dpkg-query` format matches real Ubuntu output (WSL). Other parsers are tested only on hand-written fixtures.

## NOT VERIFIED
- The desktop window has never been launched (`npm run tauri dev`); UI is unseen.
- No real-output check yet for flatpak, snap, mise, npm, pnpm, pipx, uv parsers.
- Playwright not set up. shadcn CLI not used (components written by hand).

## TODO (suggested order)
1. **Smoke test**: run `npm run tauri dev` on Windows (npm adapter) and, ideally, in WSL (install Rust + Tauri Linux deps: webkit2gtk-4.1, libayatana-appindicator, etc.) for real apt/snap/npm data. Fix parser mismatches; save real outputs as fixtures. Install flatpak/mise/pipx/uv in WSL to validate those.
2. **Operation pipeline** (first write path), start with one low-risk manager (npm or pipx):
   - Strict package-name/identifier validation; arg arrays only; never run text from descriptions.
   - Preview (command + args + manager + package + version) → explicit confirmation → execute → live stdout/stderr events → record in `operations`/`operation_logs` → refresh that manager's inventory.
   - Global lock to prevent concurrent conflicting operations; cancellation where safe; handle interrupted ops (mark on startup like scans).
   - Privilege: native mechanisms only (pkexec/polkit for APT etc.); never store passwords; flag "requires elevation" in UI.
   - No universal update-all.
3. **Operation Console** (terminal-style panel) + **Operation History** page (filter/search).
4. **Update Center**: multi-select updates, preview, severity/changelog where known, elevation flag, failed-update reporting; use native manager info for versions.
5. **Package Explorer**: `search()` and `getPackageDetails()` on the adapter trait (apt-cache/`apt search`, `flatpak search`, `snap find`, `npm search`, etc.); installed-state column; install action via pipeline.
6. **Dependency Explorer**: per-manager graphs only (never one global graph); direct/transitive/reverse; mark confirmed (manager-reported) vs inferred; orphan hints without auto-removal; fill `package_dependencies`. APT: `apt-cache depends/rdepends`, `apt-mark showauto`.
7. **Environment Backup/Restore**: JSON + YAML export, manager-specific manifests, import with diff preview; no exact-reproducibility promise; store in `environment_manifests`.
8. **Settings + background rescan** (`application_settings`, interval timer, theme), Overview extras (recently installed, recent operations, packages needing attention), missing filters (category, managed vs unmanaged, runtime vs app vs dev dependency).
9. **Later**: project-level dependencies (package.json etc.) shown separately from global ones; extra adapters (dnf, pacman, brew, winget, cargo, docker) only when implemented AND tested; Playwright e2e; installation date/`installedDate` (currently always null); icons per package in the table.

## Principles to keep
Never pretend support; null not invented; never modify without explicit authorization; no shell string concatenation; log everything; don't auto-remove dependencies; no bypass of OS permission model.
