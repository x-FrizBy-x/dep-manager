# UniManage

A unified, **safety-first** management layer over existing package managers (Tauri 2 + Rust core, React/TypeScript/Tailwind UI, SQLite).

## Status

Read-only inventory for eight managers; writes (install / update / uninstall) for **npm global packages only**. Linux is the target platform; the dev machine is Windows, so only npm has been exercised end to end in the app.

| Area | State |
|---|---|
| Adapters: APT, Flatpak, Snap, mise, npm (global), pnpm (global), pipx, uv tools | list + version + native update check (pipx/uv: no update check, status stays `unknown`). Only npm verified in the running app; the others are tested on fixtures / parser tests |
| Scanner (parallel, per-manager failure isolation, progress events, overlap annotation) | done |
| SQLite schema + migrations (all 9 tables) | done; scan_history / installed_packages / package_managers / operations / operation_logs in use |
| Overview + Installed Software pages (search, filters, sort, detail panel) | done |
| Operations (npm only): preview → explicit confirmation → execute, live operation console, cancel (kills the whole process tree), history page | done |
| Update Center: multi-select batch update with one review dialog, sequential execution, no "update all" | done (npm only) |
| Package Explorer: search + details | npm and APT (apt is read-only: no install) |
| Dependency graph, backup/restore, settings, background rescans | **not implemented** (nav items show "Soon") |
| Install / update / remove for APT, Snap, Flatpak, pipx, uv, mise, pnpm | **not implemented** (no privilege-escalation design yet) |

Design rules in force: no shell (argument arrays only), strict validation of every user-supplied token, nothing modifies the system without a preview and explicit confirmation, one operation at a time and never during a scan, no privilege escalation, no password handling, no automatic dependency removal, nothing is merged across managers by name alone.

## Develop

```
npm install
npm run tauri dev      # desktop app (needs Rust + platform webview deps)
npm test               # Vitest
npm run test:rust      # Rust unit tests (parsers, scanner, DB, operations, exec)
```

Linux is the target platform. On other OSes only the managers actually present (e.g. npm) are detected.
