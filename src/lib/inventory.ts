import { updateSeverity } from './severity'
import type { Pkg } from './types'

export type SortKey = 'name' | 'version' | 'update'

export interface Filters {
  query: string
  manager: string // '' = all
  update: '' | 'available' | 'up_to_date' | 'unknown'
  kind: '' | Pkg['kind']
  management: '' | Pkg['managementStatus']
  /** apt only: how the package got installed (`installReason` metadata). */
  reason: '' | 'manual' | 'automatic'
}

export const emptyFilters: Filters = { query: '', manager: '', update: '', kind: '', management: '', reason: '' }

export function filterPackages(pkgs: Pkg[], f: Filters): Pkg[] {
  const q = f.query.trim().toLowerCase()
  return pkgs.filter(
    (p) =>
      (!q ||
        p.name.toLowerCase().includes(q) ||
        (p.displayName ?? '').toLowerCase().includes(q) ||
        (p.description ?? '').toLowerCase().includes(q)) &&
      (!f.manager || p.packageManager === f.manager) &&
      (!f.update || p.updateStatus === f.update) &&
      (!f.kind || p.kind === f.kind) &&
      (!f.management || p.managementStatus === f.management) &&
      (!f.reason || p.metadata.installReason === f.reason),
  )
}

// Updates first, then unknown, then up to date.
const updateRank = { available: 0, unknown: 1, up_to_date: 2 } as const

export function sortPackages(pkgs: Pkg[], key: SortKey, dir: 'asc' | 'desc' = 'asc'): Pkg[] {
  const sign = dir === 'asc' ? 1 : -1
  const cmp = (a: Pkg, b: Pkg): number => {
    switch (key) {
      case 'version':
        // Versions are not comparable across managers; sort as natural strings and push unknowns last.
        if (a.version === b.version) return 0
        if (a.version === null) return 1
        if (b.version === null) return -1
        return a.version.localeCompare(b.version, undefined, { numeric: true })
      case 'update':
        return updateRank[a.updateStatus] - updateRank[b.updateStatus]
      default:
        return a.name.localeCompare(b.name, undefined, { sensitivity: 'base' })
    }
  }
  return [...pkgs].sort((a, b) => sign * cmp(a, b) || a.name.localeCompare(b.name))
}

export function formatTimestamp(secs: number | null): string {
  return secs === null ? '—' : new Date(secs * 1000).toLocaleString()
}

const SEVERITY_RANK = { major: 0, minor: 1, patch: 2, unknown: 3 } as const

/** Packages with an update available, biggest version jump first (from version numbers only). */
export function packagesNeedingAttention(pkgs: Pkg[], limit = 5): Pkg[] {
  return pkgs
    .filter((p) => p.updateStatus === 'available')
    .sort(
      (a, b) =>
        SEVERITY_RANK[updateSeverity(a.version, a.latestVersion)] - SEVERITY_RANK[updateSeverity(b.version, b.latestVersion)] ||
        a.name.localeCompare(b.name),
    )
    .slice(0, limit)
}

/**
 * Packages that first appeared after the initial inventory, newest first. This is
 * "first seen by UniManage", not an install date: packages from the very first
 * scan are never reported (they would all look new).
 */
export function recentlySeen(pkgs: Pkg[], limit = 5): Pkg[] {
  const times = pkgs.map((p) => p.firstSeenAt).filter((t): t is number => t !== null)
  if (times.length === 0) return []
  const baseline = Math.min(...times) + 300 // the first scan finishes within minutes
  return pkgs
    .filter((p) => p.firstSeenAt !== null && p.firstSeenAt > baseline)
    .sort((a, b) => (b.firstSeenAt ?? 0) - (a.firstSeenAt ?? 0))
    .slice(0, limit)
}
