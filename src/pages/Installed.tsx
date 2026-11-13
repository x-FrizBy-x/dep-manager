import { ArrowDown, ArrowUp, Plus, Search, X } from 'lucide-react'
import { useMemo, useState } from 'react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import type { Inventory } from '@/hooks/useInventory'
import { OPERABLE_MANAGERS, type Operations } from '@/hooks/useOperations'
import { emptyFilters, filterPackages, sortPackages, type Filters, type SortKey } from '@/lib/inventory'
import type { Pkg } from '@/lib/types'

const selectCls = 'h-9 rounded-md border bg-background px-2 text-sm focus-visible:outline-2'

function UpdateBadge({ p }: { p: Pkg }) {
  if (p.updateStatus === 'available') return <Badge tone="primary">→ {p.latestVersion}</Badge>
  if (p.updateStatus === 'up_to_date') return <Badge tone="success">Up to date</Badge>
  return <Badge>Unknown</Badge>
}

function Detail({ p, onClose, ops }: { p: Pkg; onClose: () => void; ops: Operations }) {
  const operable = OPERABLE_MANAGERS.has(p.packageManager)
  const rows: [string, string | null][] = [
    ['Version', p.version],
    ['Latest', p.latestVersion],
    ['Manager', p.packageManager],
    ['Type', p.kind],
    ['Source', p.installationSource],
    ['Path', p.installationPath],
    ['Architecture', p.architecture],
    ['Homepage', p.homepage],
    ['License', p.license],
    ...Object.entries(p.metadata).map(([k, v]): [string, string] => [k, v]),
  ]
  return (
    <aside className="w-80 shrink-0 overflow-y-auto border-l bg-card p-4" aria-label="Package details">
      <div className="mb-3 flex items-start justify-between gap-2">
        <div className="min-w-0">
          <h2 className="truncate font-semibold">{p.displayName ?? p.name}</h2>
          {p.displayName && <div className="truncate text-xs text-muted-foreground">{p.name}</div>}
        </div>
        <Button variant="ghost" size="icon" className="size-7" onClick={onClose} aria-label="Close details">
          <X className="size-4" />
        </Button>
      </div>
      {p.description && <p className="mb-3 text-sm text-muted-foreground">{p.description}</p>}
      {p.alsoFoundIn.length > 0 && (
        <p className="mb-3 rounded-md bg-warning/10 p-2 text-xs">
          A package with this name also appears in: <b>{p.alsoFoundIn.join(', ')}</b>. These are listed separately
          because the same name does not prove they are the same software.
        </p>
      )}
      <dl className="space-y-2 text-sm">
        {rows.map(([k, v]) => (
          <div key={k}>
            <dt className="text-xs text-muted-foreground">{k}</dt>
            <dd className="break-all">{v ?? <span className="text-muted-foreground">—</span>}</dd>
          </div>
        ))}
      </dl>
      {operable ? (
        <div className="mt-4 flex flex-wrap gap-2 border-t pt-4">
          {p.updateStatus === 'available' && (
            <Button size="sm" disabled={ops.busy} onClick={() => void ops.preview(p.packageManager, 'update', p.name)}>
              Update to {p.latestVersion}
            </Button>
          )}
          {p.name !== 'npm' && (
            <Button size="sm" variant="outline" disabled={ops.busy} onClick={() => void ops.preview(p.packageManager, 'uninstall', p.name)}>
              Uninstall
            </Button>
          )}
          <p className="w-full text-xs text-muted-foreground">You will see the exact command and confirm before anything runs.</p>
        </div>
      ) : (
        <p className="mt-4 text-xs text-muted-foreground">Changes are not supported for {p.packageManager} yet (read-only).</p>
      )}
    </aside>
  )
}

function InstallForm({ ops }: { ops: Operations }) {
  const [name, setName] = useState('')
  const [version, setVersion] = useState('')
  const submit = (e: React.FormEvent) => {
    e.preventDefault()
    const n = name.trim()
    if (n) void ops.preview('npm', 'install', n, version.trim() || undefined)
  }
  return (
    <form onSubmit={submit} className="flex items-center gap-2 border-b bg-card/50 px-3 py-2" aria-label="Install npm package">
      <Plus className="size-4 text-muted-foreground" aria-hidden />
      <input
        value={name}
        onChange={(e) => setName(e.target.value)}
        placeholder="Install global npm package (e.g. typescript)"
        aria-label="npm package name"
        className="h-8 min-w-48 flex-1 rounded-md border bg-background px-2 font-mono text-xs focus-visible:outline-2"
      />
      <input
        value={version}
        onChange={(e) => setVersion(e.target.value)}
        placeholder="version (optional)"
        aria-label="Version or tag"
        className="h-8 w-36 rounded-md border bg-background px-2 font-mono text-xs focus-visible:outline-2"
      />
      <Button type="submit" size="sm" disabled={ops.busy || !name.trim()}>
        Preview install
      </Button>
    </form>
  )
}

function Th({ k, sort, onSort, children }: { k: SortKey; sort: { key: SortKey; dir: 'asc' | 'desc' }; onSort: (k: SortKey) => void; children: string }) {
  return (
    <th className="px-3 py-2 text-left font-medium" aria-sort={sort.key === k ? (sort.dir === 'asc' ? 'ascending' : 'descending') : 'none'}>
      <button className="inline-flex cursor-pointer items-center gap-1 hover:text-foreground" onClick={() => onSort(k)}>
        {children}
        {sort.key === k && (sort.dir === 'asc' ? <ArrowUp className="size-3" /> : <ArrowDown className="size-3" />)}
      </button>
    </th>
  )
}

export function Installed({ inv, ops }: { inv: Inventory; ops: Operations }) {
  const [filters, setFilters] = useState<Filters>(emptyFilters)
  const [sort, setSort] = useState<{ key: SortKey; dir: 'asc' | 'desc' }>({ key: 'name', dir: 'asc' })
  const [selectedId, setSelectedId] = useState<string | null>(null)

  const rows = useMemo(
    () => sortPackages(filterPackages(inv.packages, filters), sort.key, sort.dir),
    [inv.packages, filters, sort],
  )
  const hasReason = useMemo(() => inv.packages.some((p) => p.metadata.installReason), [inv.packages])
  const selected = inv.packages.find((p) => p.id === selectedId) ?? null
  const set = <K extends keyof Filters>(k: K, v: Filters[K]) => setFilters((f) => ({ ...f, [k]: v }))
  const toggle = (key: SortKey) =>
    setSort((s) => (s.key === key ? { key, dir: s.dir === 'asc' ? 'desc' : 'asc' } : { key, dir: 'asc' }))

  if (inv.packages.length === 0) {
    return (
      <div className="flex flex-col items-center gap-3 p-16 text-center">
        <InstallForm ops={ops} />
        <div className="font-medium">No packages yet</div>
        <p className="max-w-sm text-sm text-muted-foreground">
          Go to Overview and run a scan to build your software inventory.
        </p>
      </div>
    )
  }

  return (
    <div className="flex h-full min-h-0">
      <div className="flex min-w-0 flex-1 flex-col">
        <InstallForm ops={ops} />
        <div className="flex flex-wrap items-center gap-2 border-b p-3">
          <div className="relative min-w-48 flex-1">
            <Search className="pointer-events-none absolute top-2.5 left-2.5 size-4 text-muted-foreground" aria-hidden />
            <input
              type="search"
              value={filters.query}
              onChange={(e) => set('query', e.target.value)}
              placeholder="Search installed software"
              aria-label="Search installed software"
              className="h-9 w-full rounded-md border bg-background pr-3 pl-8 text-sm focus-visible:outline-2"
            />
          </div>
          <select aria-label="Package manager" className={selectCls} value={filters.manager} onChange={(e) => set('manager', e.target.value)}>
            <option value="">All managers</option>
            {inv.managers.filter((m) => m.packageCount > 0).map((m) => (
              <option key={m.id} value={m.id}>{m.displayName}</option>
            ))}
          </select>
          <select aria-label="Update status" className={selectCls} value={filters.update} onChange={(e) => set('update', e.target.value as Filters['update'])}>
            <option value="">Any update status</option>
            <option value="available">Update available</option>
            <option value="up_to_date">Up to date</option>
            <option value="unknown">Unknown</option>
          </select>
          <select aria-label="Type" className={selectCls} value={filters.kind} onChange={(e) => set('kind', e.target.value as Filters['kind'])}>
            <option value="">All types</option>
            <option value="application">Application</option>
            <option value="runtime">Runtime</option>
            <option value="library">Library / dev tool</option>
            <option value="system">System package</option>
          </select>
          {hasReason && (
            <select aria-label="Install reason" className={selectCls} value={filters.reason} onChange={(e) => set('reason', e.target.value as Filters['reason'])}>
              <option value="">Any install reason</option>
              <option value="manual">Manually installed</option>
              <option value="automatic">Installed as dependency</option>
            </select>
          )}
          <span className="text-xs text-muted-foreground tabular-nums">{rows.length} / {inv.packages.length}</span>
        </div>

        <div className="min-h-0 flex-1 overflow-auto">
          <table className="w-full text-sm">
            <thead className="sticky top-0 bg-background text-xs text-muted-foreground">
              <tr className="border-b">
                <Th k="name" sort={sort} onSort={toggle}>Name</Th>
                <Th k="version" sort={sort} onSort={toggle}>Version</Th>
                <th className="px-3 py-2 text-left font-medium">Manager</th>
                <th className="px-3 py-2 text-left font-medium">Type</th>
                <Th k="update" sort={sort} onSort={toggle}>Update</Th>
              </tr>
            </thead>
            <tbody>
              {rows.map((p) => (
                <tr
                  key={p.id}
                  tabIndex={0}
                  aria-selected={p.id === selectedId}
                  onClick={() => setSelectedId(p.id)}
                  onKeyDown={(e) => (e.key === 'Enter' || e.key === ' ') && (e.preventDefault(), setSelectedId(p.id))}
                  className={`cursor-pointer border-b transition-colors hover:bg-muted/60 focus-visible:outline-2 ${p.id === selectedId ? 'bg-muted' : ''}`}
                >
                  <td className="max-w-72 truncate px-3 py-2 font-medium">{p.displayName ?? p.name}</td>
                  <td className="px-3 py-2 font-mono text-xs">{p.version ?? '—'}</td>
                  <td className="px-3 py-2"><Badge>{p.packageManager}</Badge></td>
                  <td className="px-3 py-2 text-muted-foreground">{p.kind}</td>
                  <td className="px-3 py-2"><UpdateBadge p={p} /></td>
                </tr>
              ))}
            </tbody>
          </table>
          {rows.length === 0 && (
            <div className="p-12 text-center text-sm text-muted-foreground">
              No packages match your filters.{' '}
              <button className="cursor-pointer underline" onClick={() => setFilters(emptyFilters)}>Clear filters</button>
            </div>
          )}
        </div>
      </div>
      {selected && <Detail p={selected} ops={ops} onClose={() => setSelectedId(null)} />}
    </div>
  )
}
