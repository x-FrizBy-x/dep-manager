import { describe, expect, it } from 'vitest'
import { emptyFilters, filterPackages, packagesNeedingAttention, recentlySeen, sortPackages } from './inventory'
import type { Pkg } from './types'

const mk = (over: Partial<Pkg>): Pkg => ({
  id: over.id ?? over.name ?? 'x',
  name: 'x',
  displayName: null,
  version: null,
  latestVersion: null,
  description: null,
  packageManager: 'apt',
  category: null,
  kind: 'system',
  installationSource: null,
  installationPath: null,
  architecture: null,
  installedDate: null,
  updateAvailable: null,
  updateStatus: 'unknown',
  managementStatus: 'managed',
  homepage: null,
  license: null,
  dependencies: [],
  metadata: {},
  alsoFoundIn: [],
  firstSeenAt: null,
  ...over,
})

const data = [
  mk({ name: 'git', packageManager: 'apt', description: 'Distributed VCS', updateStatus: 'up_to_date' }),
  mk({ name: 'typescript', packageManager: 'npm', kind: 'library', updateStatus: 'available', version: '5.4.0' }),
  mk({ name: 'node', packageManager: 'mise', kind: 'runtime', version: '22.10.0' }),
  mk({ name: 'ruff', packageManager: 'uv', version: '0.4.1' }),
]

describe('filterPackages', () => {
  it('matches name and description case-insensitively', () => {
    expect(filterPackages(data, { ...emptyFilters, query: 'DISTRIBUTED' }).map((p) => p.name)).toEqual(['git'])
  })
  it('combines filters', () => {
    expect(filterPackages(data, { ...emptyFilters, manager: 'npm', update: 'available' })).toHaveLength(1)
    expect(filterPackages(data, { ...emptyFilters, manager: 'npm', update: 'up_to_date' })).toHaveLength(0)
  })
  it('filters by kind', () => {
    expect(filterPackages(data, { ...emptyFilters, kind: 'runtime' }).map((p) => p.name)).toEqual(['node'])
  })
})

describe('sortPackages', () => {
  it('sorts by name', () => {
    expect(sortPackages(data, 'name').map((p) => p.name)).toEqual(['git', 'node', 'ruff', 'typescript'])
  })
  it('puts updates first and does not mutate input', () => {
    const before = data.map((p) => p.name)
    expect(sortPackages(data, 'update')[0].name).toBe('typescript')
    expect(data.map((p) => p.name)).toEqual(before)
  })
  it('sorts versions naturally with unknown last', () => {
    const v = sortPackages(
      [mk({ name: 'a', version: '10.0' }), mk({ name: 'b', version: '9.0' }), mk({ name: 'c', version: null })],
      'version',
    )
    expect(v.map((p) => p.name)).toEqual(['b', 'a', 'c'])
  })
})

describe('install reason filter', () => {
  const pk = [
    mk({ name: 'curl', metadata: { installReason: 'manual' } }),
    mk({ name: 'libcurl', metadata: { installReason: 'automatic' } }),
    mk({ name: 'ruff' }),
  ]
  it('keeps only the requested reason', () => {
    expect(filterPackages(pk, { ...emptyFilters, reason: 'manual' }).map((p) => p.name)).toEqual(['curl'])
    expect(filterPackages(pk, { ...emptyFilters, reason: 'automatic' }).map((p) => p.name)).toEqual(['libcurl'])
    expect(filterPackages(pk, emptyFilters)).toHaveLength(3)
  })
})

describe('overview helpers', () => {
  it('ranks updates by size of the version jump and respects the limit', () => {
    const pk = [
      mk({ name: 'a', updateStatus: 'available', version: '1.0.0', latestVersion: '1.0.1' }),
      mk({ name: 'b', updateStatus: 'available', version: '1.0.0', latestVersion: '2.0.0' }),
      mk({ name: 'c', updateStatus: 'available', version: '1.0.0', latestVersion: '1.2.0' }),
      mk({ name: 'd', updateStatus: 'up_to_date' }),
    ]
    expect(packagesNeedingAttention(pk).map((p) => p.name)).toEqual(['b', 'c', 'a'])
    expect(packagesNeedingAttention(pk, 1)).toHaveLength(1)
  })

  it('never reports the initial inventory as recently seen', () => {
    const t = 1_000_000
    const pk = [
      mk({ name: 'old1', firstSeenAt: t }),
      mk({ name: 'old2', firstSeenAt: t + 20 }),
      mk({ name: 'new1', firstSeenAt: t + 86_400 }),
      mk({ name: 'new2', firstSeenAt: t + 2 * 86_400 }),
      mk({ name: 'unknown', firstSeenAt: null }),
    ]
    expect(recentlySeen(pk).map((p) => p.name)).toEqual(['new2', 'new1'])
    expect(recentlySeen([])).toEqual([])
  })
})
