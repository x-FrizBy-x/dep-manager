import { Search } from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'
import { Badge } from '@/components/ui/badge'
import type { Inventory } from '@/hooks/useInventory'
import { api } from '@/lib/api'
import type { DependencyEdge, DependencyManager, PackageDependencies } from '@/lib/types'

const selectCls = 'h-9 rounded-md border bg-background px-2 text-sm focus-visible:outline-2'

function EdgeList({
  title,
  edges,
  empty,
  onOpen,
  known,
}: {
  title: string
  edges: DependencyEdge[]
  empty: string
  onOpen: (name: string) => void
  known: Set<string>
}) {
  return (
    <section aria-label={title} className="mb-5">
      <h3 className="mb-1 text-sm font-medium">
        {title} <span className="text-muted-foreground">({edges.length})</span>
      </h3>
      {edges.length === 0 ? (
        <p className="text-sm text-muted-foreground">{empty}</p>
      ) : (
        <ul className="max-h-64 space-y-0.5 overflow-auto font-mono text-xs">
          {edges.map((e) => (
            <li key={`${e.name}@${e.version ?? ''}:${e.kind}`} className="flex flex-wrap items-center gap-2">
              {known.has(e.name) ? (
                <button type="button" className="text-primary underline-offset-2 hover:underline" onClick={() => onOpen(e.name)}>
                  {e.name}
                </button>
              ) : (
                <span className={e.installed ? '' : 'text-muted-foreground'}>{e.name}</span>
              )}
              {e.version && <span className="text-muted-foreground">{e.version}</span>}
              {(e.kind !== 'transitive' || !e.installed) && <Badge tone={e.installed ? 'neutral' : 'warning'}>{e.kind}</Badge>}
            </li>
          ))}
        </ul>
      )}
    </section>
  )
}

function Detail({
  managerId,
  name,
  onOpen,
  known,
}: {
  managerId: string
  name: string
  onOpen: (name: string) => void
  known: Set<string>
}) {
  const key = `${managerId}:${name}`
  const [state, setState] = useState<{ key: string; d?: PackageDependencies; error?: string }>({ key: '' })

  useEffect(() => {
    let off = false
    api
      .getPackageDependencies(managerId, name)
      .then((d) => !off && setState({ key, d }))
      .catch((e) => !off && setState({ key, error: String(e) }))
    return () => {
      off = true
    }
  }, [managerId, name, key])

  const loading = state.key !== key
  const d = loading ? undefined : state.d

  return (
    <aside className="min-w-0 flex-1 overflow-y-auto border-l bg-card p-5" aria-label="Dependency details">
      <h2 className="mb-1 truncate text-lg font-semibold">{name}</h2>
      {loading && <p className="text-sm text-muted-foreground" role="status">Reading dependencies from the manager…</p>}
      {!loading && state.error && (
        <p role="alert" className="rounded-md border border-destructive/40 bg-destructive/10 p-2 text-sm">{state.error}</p>
      )}
      {d && (
        <>
          <p className="mb-3 text-sm text-muted-foreground">
            {d.version ? `version ${d.version}` : 'version unknown'}
            {d.installReason && <> · {d.installReason}</>}
          </p>
          {d.orphanHint && (
            <p role="note" className="mb-4 rounded-md border border-amber-500/40 bg-amber-500/10 p-2 text-sm">
              <Badge tone="warning">inferred</Badge> {d.orphanHint}
            </p>
          )}
          <EdgeList title="Depends on" edges={d.dependsOn} empty="No dependencies reported." onOpen={onOpen} known={known} />
          <EdgeList
            title="Indirect (transitive)"
            edges={d.transitive}
            empty="Nothing further down the tree."
            onOpen={onOpen}
            known={known}
          />
          <EdgeList
            title="Required by"
            edges={d.requiredBy}
            empty="No installed package depends on this one."
            onOpen={onOpen}
            known={known}
          />
          <p className="mb-2 text-xs text-muted-foreground">
            Dependency lists are reported by the package manager itself. Only the orphan hint is an inference, and nothing here
            changes your system.
          </p>
          <ul className="list-disc space-y-1 pl-5 text-xs text-muted-foreground">
            {d.notes.map((n) => (
              <li key={n}>{n}</li>
            ))}
          </ul>
        </>
      )}
    </aside>
  )
}

export function Dependencies({ inv }: { inv: Inventory }) {
  const [managers, setManagers] = useState<DependencyManager[] | null>(null)
  const [manager, setManager] = useState('')
  const [filter, setFilter] = useState('')
  const [selected, setSelected] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let off = false
    api
      .getDependencyManagers()
      .then((m) => {
        if (off) return
        setManagers(m)
        setManager((cur) => cur || m[0]?.id || '')
      })
      .catch((e) => !off && setError(String(e)))
    return () => {
      off = true
    }
  }, [])

  const pkgs = useMemo(
    () => inv.packages.filter((p) => p.packageManager === manager).sort((a, b) => a.name.localeCompare(b.name)),
    [inv.packages, manager],
  )
  const known = useMemo(() => new Set(pkgs.map((p) => p.name)), [pkgs])
  const shown = useMemo(() => {
    const f = filter.trim().toLowerCase()
    return f ? pkgs.filter((p) => p.name.toLowerCase().includes(f)) : pkgs
  }, [pkgs, filter])

  if (managers && managers.length === 0) {
    return (
      <div className="p-12 text-center text-sm text-muted-foreground">
        No manager with dependency support was found on this machine (implemented for npm and APT).
      </div>
    )
  }

  return (
    <div className="flex h-full min-h-0">
      <div className="flex w-96 shrink-0 flex-col p-6">
        <h1 className="text-xl font-semibold">Dependencies</h1>
        <p className="mb-4 text-sm text-muted-foreground">
          One manager at a time — graphs are never merged across managers. Pick an installed package.
        </p>
        <div className="mb-3 flex items-center gap-2">
          <select
            aria-label="Package manager"
            className={selectCls}
            value={manager}
            onChange={(e) => {
              setManager(e.target.value)
              setSelected(null)
            }}
          >
            {(managers ?? []).map((m) => (
              <option key={m.id} value={m.id}>{m.displayName}</option>
            ))}
          </select>
          <div className="relative flex-1">
            <Search className="pointer-events-none absolute top-2.5 left-2.5 size-4 text-muted-foreground" aria-hidden />
            <input
              value={filter}
              onChange={(e) => setFilter(e.target.value)}
              placeholder="Filter installed"
              aria-label="Filter installed packages"
              className="h-9 w-full rounded-md border bg-background pr-3 pl-8 text-sm focus-visible:outline-2"
            />
          </div>
        </div>
        {error && <div role="alert" className="mb-3 rounded-md border border-destructive/40 bg-destructive/10 p-2 text-sm">{error}</div>}
        {pkgs.length === 0 ? (
          <p className="p-4 text-sm text-muted-foreground">No installed packages for this manager. Run a scan from Overview first.</p>
        ) : (
          <ul className="min-h-0 flex-1 overflow-auto text-sm" aria-label="Installed packages">
            {shown.map((p) => (
              <li key={p.id}>
                <button
                  type="button"
                  aria-pressed={selected === p.name}
                  onClick={() => setSelected(p.name)}
                  className={`flex w-full items-center justify-between gap-2 border-b px-3 py-2 text-left hover:bg-muted/60 focus-visible:outline-2 ${selected === p.name ? 'bg-muted' : ''}`}
                >
                  <span className="min-w-0 truncate">{p.name}</span>
                  <span className="shrink-0 font-mono text-xs text-muted-foreground">{p.version ?? ''}</span>
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>
      {selected && manager ? (
        <Detail managerId={manager} name={selected} onOpen={setSelected} known={known} />
      ) : (
        <div className="flex flex-1 items-center justify-center border-l p-8 text-sm text-muted-foreground">
          Select a package to see what it depends on and what depends on it.
        </div>
      )}
    </div>
  )
}
