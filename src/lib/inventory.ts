import type { Pkg } from './types'

export type SortKey = 'name' | 'version' | 'update'

export interface Filters {
  query: string
  manager: string // '' = all
  update: '' | 'available' | 'up_to_date' | 'unknown'
  kind: '' | Pkg['kind']
  management: '' | Pkg['managementStatus']
}

export const emptyFilters: Filters = { query: '', manager: '', update: '', kind: '', management: '' }

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
      (!f.management || p.managementStatus === f.management),
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
