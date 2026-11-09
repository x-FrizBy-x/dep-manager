import { describe, expect, it } from 'vitest'
import { emptyFilters, filterPackages, sortPackages } from './inventory'
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
