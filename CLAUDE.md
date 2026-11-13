# CLAUDE.md

UniManage: safety-first unified manager over existing package managers (Tauri 2 + Rust core, React 19/TypeScript/Tailwind 4 UI, SQLite). Linux is the target platform; the dev machine is Windows 11, so only npm is live-testable in the app. See `README.md` for status and `HANDOFF.md` (local, gitignored) for session notes and the TODO list.

## Commands
- `npm install`; `npm run tauri dev` (desktop app); `npm run build` (tsc -b + vite)
- `npm test` (Vitest); `npm run test:rust` (or `cargo test --lib` in `src-tauri`); `npm run typecheck`; `npm run lint` (oxlint; one known warning in `hooks/useInventory.ts`)
- Real-system smoke test (read-only, ignored by default): `cargo test --manifest-path src-tauri/Cargo.toml --test real_system -- --ignored --nocapture`; used on Linux/WSL to run the real adapters
- Real write test (installs/uninstalls a throwaway pipx and Flatpak package, per-user): `cargo test --manifest-path src-tauri/Cargo.toml --test real_ops -- --ignored --nocapture --test-threads=1`
- UI driving: `node scripts/ui-check.mjs <shot.png> [text to click ...]` (Playwright over CDP; start app with `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222`)

## Layout
- `src-tauri/src`: `adapters/` (one per manager: apt, flatpak, snap, mise, npm, pnpm, pipx, uv; Adapter trait with list/update check/search/details/dependencies, defaults = Unsupported), `scanner.rs`, `ops.rs` (plan/validate/execute operations), `exec.rs` (streaming runner, process-tree kill), `commands.rs` (Tauri commands), `db.rs` (SQLite + migrations), `model.rs`.
- `src`: `pages/` (Overview, Installed, Updates, Explorer, Dependencies, History), `components/OperationUI.tsx`, `hooks/`, `lib/` (api, types, severity, operations).

## Rules (do not break)
- Never pretend support; use null rather than invented values.
- No shell: argv arrays only; strictly validate every user-supplied token (matters on Windows because `npm` is `npm.cmd`).
- Nothing modifies the system without preview -> explicit confirmation -> execute; one operation at a time, never during a scan; log everything.
- No privilege escalation/password handling, no auto-removal of dependencies, no "update all", nothing merged across managers by name alone.
- Tests must not alter the user's global npm packages beyond throwaway ones that are uninstalled afterwards.

## Gotchas
- Disk space: Windows target dir (~8 GB) plus a Linux copy in WSL (~6 GB) plus Flatpak runtimes can fill C:. In session 3 C: hit 0 bytes free, the WSL ext4 went read-only (emergency_ro) and Ubuntu would not start until space was freed and `wsl --shutdown` was run. Check `Get-PSDrive C` before big builds; `cargo clean` is always safe.
- Running the real GUI on Linux: WSLg works (`/mnt/wslg`); build with `cargo build --features tauri/custom-protocol` in a copy under ~ (not /mnt/c), run with DISPLAY=:0, capture with `xwd -id <window id> | convert`, click with `xdotool` using absolute coords from `xwininfo`.
- Use PowerShell for `wsl` calls (Git Bash rewrites `/mnt` paths). Keep scratch scripts outside the repo.
- After adding/removing React hooks, HMR errors -> reload the page.
