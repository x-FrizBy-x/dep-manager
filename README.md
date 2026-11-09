# UniManage

A unified, **safety-first** management layer over existing package managers (Tauri 2 + Rust core, React/TypeScript/Tailwind UI, SQLite).

## Status: Milestone 1 — read-only core

| Area | State |
|---|---|
| Adapters: APT, Flatpak, Snap, mise, npm (global), pnpm (global), pipx, uv tools | list + version + native update check (pipx/uv: no update check, status stays `unknown`) |
| Scanner (parallel, per-manager failure isolation, progress events, overlap annotation) | done |
| SQLite schema + migrations (all 9 tables) | done; scan_history / installed_packages / package_managers in use |
| Overview + Installed Software pages (search, filters, sort, detail panel) | done |
| Install / update / remove, operation console, history, dependency graph, backup/restore, package explorer, settings, background rescans | **not implemented** (nav items show "Soon") |

Design rules in force: no shell (argument arrays only), fixed read-only commands, per-command timeout, no privilege escalation, no password handling, nothing is merged across managers by name alone.

## Develop

```
npm install
npm run tauri dev      # desktop app (needs Rust + platform webview deps)
npm test               # Vitest
npm run test:rust      # Rust unit tests (parsers, scanner, DB)
```

Linux is the target platform. On other OSes only the managers actually present (e.g. npm) are detected.
