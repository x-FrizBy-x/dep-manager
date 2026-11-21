// Fake Tauri backend for browser-only UI tests. Nothing here touches a real package manager:
// the page's `window.__TAURI_INTERNALS__` is replaced before the app loads, every `invoke` is
// recorded in `window.__calls`, and answers come from the `responses` table below.
// These tests check the React UI (and the preview -> confirm -> run contract); the Rust adapters
// are covered by `cargo test`.
import type { Page } from '@playwright/test'
import type { ManagerInfo, OperationPlan, OperationRecord, Pkg } from '../src/lib/types'

export const pkg = (over: Partial<Pkg> & Pick<Pkg, 'id' | 'name' | 'packageManager'>): Pkg => ({
  displayName: null,
  version: null,
  latestVersion: null,
  description: null,
  category: null,
  kind: 'application',
  installationSource: null,
  installationPath: null,
  architecture: null,
  installedDate: null,
  updateAvailable: null,
  updateStatus: 'unknown',
  managementStatus: 'unknown',
  homepage: null,
  license: null,
  dependencies: [],
  metadata: {},
  alsoFoundIn: [],
  firstSeenAt: null,
  ...over,
})

export const PACKAGES: Pkg[] = [
  pkg({ id: 'npm:typescript', name: 'typescript', packageManager: 'npm', version: '5.4.0', latestVersion: '5.5.0', updateAvailable: true, updateStatus: 'available', description: 'TypeScript compiler' }),
  pkg({ id: 'npm:left-pad', name: 'left-pad', packageManager: 'npm', version: '1.3.0', latestVersion: '1.3.0', updateAvailable: false, updateStatus: 'up_to_date' }),
  pkg({ id: 'mise:node@20', name: 'node', packageManager: 'mise', version: '20.10.0', kind: 'runtime' }),
]

const manager = (id: string, displayName: string, n: number): ManagerInfo => ({
  id, displayName, detected: true, path: `/usr/bin/${id}`, version: '1.0', supportsUpdates: true, packageCount: n, lastError: null,
})

export const MANAGERS: ManagerInfo[] = [manager('npm', 'npm', 2), manager('mise', 'mise', 1)]

export const plan = (kind: OperationPlan['kind'], name: string): OperationPlan => ({
  id: `plan-${kind}-${name}`,
  managerId: 'npm',
  kind,
  package: name,
  version: null,
  previousVersion: '1.3.0',
  expectedVersion: null,
  program: 'npm',
  args: [kind, '-g', name],
  command: `npm ${kind} -g ${name}`,
  requiresElevation: false,
  cancellable: true,
  effects: [],
  warnings: [],
})

export const record = (kind: OperationRecord['operationType'], name: string): OperationRecord => ({
  id: 1, startedAt: 1_700_000_000, finishedAt: 1_700_000_005, operationType: kind, packageName: name, managerId: 'npm',
  previousVersion: '1.3.0', newVersion: null, command: `npm ${kind} -g ${name}`, result: 'success', error: null,
})

/** Answers per Tauri command. `{ __error: 'msg' }` makes the command reject. */
export const defaultResponses: Record<string, unknown> = {
  get_packages: PACKAGES,
  get_managers: MANAGERS,
  get_scan_history: [],
  start_scan: { id: 1, startedAt: 1, finishedAt: 2, status: 'completed', packageCount: 3, updateCount: 1, details: null },
  get_operations: [],
  get_search_managers: [{ id: 'npm', displayName: 'npm', canInstall: true }],
  get_dependency_managers: [{ id: 'npm', displayName: 'npm' }],
  list_manifests: [],
  get_projects: [],
  get_settings: { rescanEnabled: false, rescanIntervalMinutes: 60, rescanCheckUpdates: true },
  save_settings: { rescanEnabled: false, rescanIntervalMinutes: 60, rescanCheckUpdates: true },
  preview_operation: plan('uninstall', 'left-pad'),
  run_operation: record('uninstall', 'left-pad'),
  cancel_operation: null,
}

export async function installBackend(page: Page, overrides: Record<string, unknown> = {}) {
  const responses = { ...defaultResponses, ...overrides }
  await page.addInitScript((responses) => {
    const w = window as unknown as Record<string, unknown>
    const calls: { cmd: string; args: unknown }[] = []
    const callbacks = new Map<number, (e: unknown) => void>()
    let next = 1
    w.__calls = calls
    w.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} }
    w.__TAURI_INTERNALS__ = {
      transformCallback(cb: (e: unknown) => void) {
        const id = next++
        callbacks.set(id, cb)
        return id
      },
      unregisterCallback(id: number) {
        callbacks.delete(id)
      },
      convertFileSrc: (p: string) => p,
      async invoke(cmd: string, args: unknown) {
        if (cmd === 'plugin:event|listen') return next++
        if (cmd === 'plugin:event|unlisten') return null
        calls.push({ cmd, args })
        if (!(cmd in responses)) throw new Error(`unmocked command: ${cmd}`)
        const r = (responses as Record<string, unknown>)[cmd]
        if (r && typeof r === 'object' && '__error' in r) throw (r as { __error: string }).__error
        return r
      },
    }
  }, responses)
}

/** Every command the page has invoked so far (event plumbing excluded). */
export const calls = (page: Page, cmd?: string) =>
  page
    .evaluate(() => (window as unknown as { __calls: { cmd: string; args: unknown }[] }).__calls)
    .then((all) => (cmd ? all.filter((c) => c.cmd === cmd) : all))
