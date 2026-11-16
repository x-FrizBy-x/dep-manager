# UniManage

A unified, **safety-first** management layer over existing package managers (Tauri 2 + Rust core, React/TypeScript/Tailwind UI, SQLite).

## Status

Read-only inventory for eight managers; writes (install / update / uninstall) for **npm global packages, pipx, per-user Flatpak, APT and Snap**. Linux is the target platform; the dev machine is Windows, so only npm has been exercised end to end in the running app. On Linux (Ubuntu in WSL) the whole backend test suite passes and the real adapters have been run against live apt, snap, flatpak and npm via `cargo test --test real_system -- --ignored --nocapture` (the desktop window itself has not been opened on Linux).

| Area | State |
|---|---|
| Adapters: APT, Flatpak, Snap, mise, npm (global), pnpm (global), pipx, uv tools | list + version + native update check (pipx/uv: no update check, status stays `unknown`). Only npm verified in the running app; the others are tested on fixtures / parser tests |
| Scanner (parallel, per-manager failure isolation, progress events, overlap annotation) | done |
| SQLite schema + migrations (all 9 tables) | done; scan_history / installed_packages / package_managers / operations / operation_logs in use |
| Overview + Installed Software pages (search, filters, sort, detail panel) | done |
| Operations (npm only): preview → explicit confirmation → execute, live operation console, cancel (kills the whole process tree), history page (clearable, with confirmation) | done |
| Update Center: multi-select batch update with one review dialog, sequential execution, no "update all" | done (npm only) |
| Package Explorer: search + details | npm, APT, Snap and Flatpak (all but npm are read-only: no install); snap/flatpak parsers tested on real Ubuntu (WSL) output, not yet run in the app |
| Dependencies page | per-manager dependency lists for npm and APT (read-only) |
| Environment Backup | manifests per manager (stored in SQLite), JSON and YAML export, JSON import with validation, diff against the last scan, restore through the normal preview/confirm flow (npm only). No exact-reproduction guarantee; nothing is ever removed |
| Project Dependencies | read-only: declared deps from a folder's package.json (+ package-lock.json, node_modules), requirements.txt, pyproject.toml (PEP 621/735, Poetry) and Cargo.toml (+ Cargo.lock); other manifests are listed as unsupported. Never runs anything in the project; separate from global packages |
| Settings + background rescan | theme (system/light/dark) and an opt-in, off-by-default periodic read-only rescan (15 min to daily) while the app is open; Overview shows needs-attention, recently seen and recent operations; Installed has an apt install-reason filter |
| Install / update / remove for pipx and per-user Flatpak | done; run against real pipx 1.8 and Flatpak 1.16 on Ubuntu (WSL) via `cargo test --test real_ops`. System-wide Flatpak installs are refused (no privilege escalation); Flatpak installs a branch, not a pinned version |
| Install / update / remove for APT and Snap | done via polkit `pkexec`: the system shows its own password dialog, UniManage never sees a password, nothing runs through sudo or a shell. APT previews an unprivileged `apt-get -s` dry run (what is installed/removed), uses `remove` (no autoremove, no purge) and `--only-upgrade`; protected packages (apt, systemd, libc6, sudo, snapd, core...) are refused; elevated operations cannot be cancelled. APT cycle verified as root on Ubuntu (WSL); the real polkit dialog and Snap writes are not yet exercised |
| Install / update / remove for uv, mise, pnpm | **not implemented** |

Design rules in force: no shell (argument arrays only), strict validation of every user-supplied token, nothing modifies the system without a preview and explicit confirmation, one operation at a time and never during a scan, no privilege escalation, no password handling, no automatic dependency removal, nothing is merged across managers by name alone.

## Privileges explained

APT and Snap changes need administrator rights. UniManage itself always runs as your normal user; this is how the elevation works and what the words mean.

| Term | Meaning |
|---|---|
| **root** | The all-powerful administrator account (uid 0). Can change any file on the system. |
| **Normal user** | Your everyday account. Cannot install system software or edit system files. |
| **sudo group** | Membership means "may become root after proving who you are". Being in it does not make you root. |
| **polkit** | The system service that decides "may this user do this privileged action?". It is the OS's own permission authority. |
| **`pkexec`** | A command that asks polkit for permission and, if granted, runs one program as root. UniManage's only escalation path (`/usr/bin/pkexec`). Never `sudo`. |
| **Authentication agent** | A small program in the desktop session that draws the password dialog for polkit and hands the answer back. Without an agent polkit has nobody to ask and `pkexec` waits forever. Every Linux desktop ships one. |
| **Login session** | The "logged-in sitting" tracked by systemd-logind. Agents register per session; a login on a real desktop creates one automatically. |

Flow of one privileged operation:

1. UniManage shows the preview (for APT: an unprivileged `apt-get -s` dry run) and waits for your confirmation.
2. UniManage runs `pkexec apt-get …` as your normal user.
3. polkit sees the request needs admin authentication and asks the agent.
4. The desktop shows its own password dialog. You type your password **into that dialog**; UniManage never sees, asks for or stores it.
5. If polkit approves, only that one command runs as root. Cancel or a wrong password means nothing runs.

Elevated operations cannot be cancelled from UniManage (a root child cannot be killed safely), and dependencies are never auto-removed.

**Testing under WSL.** `wsl -u root` starts the distro as root with no password; it is a WSL shortcut, not how a real system works, and bypasses polkit. WSL has no desktop and no agent, and `wsl.exe` commands do not belong to a logind session. To exercise the real dialog install an agent (e.g. `lxpolkit`) and run everything from a real session: `wsl -u root -- su -l alex -c "<script that starts lxpolkit, then the command>"`. Verified this way: `pkexec` + `lxpolkit` dialog + authentication as a normal user.

## Develop

```
npm install
npm run tauri dev      # desktop app (needs Rust + platform webview deps)
npm test               # Vitest
npm run test:rust      # Rust unit tests (parsers, scanner, DB, operations, exec)
```

Linux is the target platform. On other OSes only the managers actually present (e.g. npm) are detected.
