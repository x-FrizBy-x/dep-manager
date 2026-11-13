import { Search, X } from 'lucide-react'
import { useEffect, useState } from 'react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import type { Operations } from '@/hooks/useOperations'
import { api } from '@/lib/api'
import type { PackageDetails, SearchManager, SearchResult } from '@/lib/types'

const selectCls = 'h-9 rounded-md border bg-background px-2 text-sm focus-visible:outline-2'

function Details({ hit, onClose }: { hit: SearchResult; onClose: () => void }) {
  const [state, setState] = useState<{ key: string; d?: PackageDetails; error?: string }>({ key: '' })
  const key = `${hit.managerId}:${hit.name}`

  useEffect(() => {
    let off = false
    api
      .getPackageDetails(hit.managerId, hit.name)
      .then((d) => !off && setState({ key, d }))
      .catch((e) => !off && setState({ key, error: String(e) }))
    return () => {
      off = true
    }
  }, [hit.managerId, hit.name, key])

  const loading = state.key !== key
  const d = loading ? undefined : state.d
  const rows: [string, string | null | undefined][] = d
    ? [
        ['Version', d.version],
        ['License', d.license],
        ['Homepage', d.homepage],
        ...Object.entries(d.metadata).map(([k, v]): [string, string] => [k, v]),
      ]
    : []

  return (
    <aside className="w-96 shrink-0 overflow-y-auto border-l bg-card p-4" aria-label="Package details">
      <div className="mb-3 flex items-start justify-between gap-2">
        <h2 className="min-w-0 truncate font-semibold">{hit.name}</h2>
        <Button variant="ghost" size="icon" className="size-7" onClick={onClose} aria-label="Close details">
          <X className="size-4" />
        </Button>
      </div>
      {loading && <p className="text-sm text-muted-foreground" role="status">Loading details…</p>}
      {!loading && state.error && (
        <p role="alert" className="rounded-md border border-destructive/40 bg-destructive/10 p-2 text-sm">{state.error}</p>
      )}
      {d && (
        <>
          {d.description && <p className="mb-3 text-sm text-muted-foreground">{d.description}</p>}
          <dl className="space-y-2 text-sm">
            {rows.map(([k, v]) => (
              <div key={k}>
                <dt className="text-xs text-muted-foreground">{k}</dt>
                <dd className="break-all">{v ?? <span className="text-muted-foreground">—</span>}</dd>
              </div>
            ))}
            <div>
              <dt className="text-xs text-muted-foreground">Dependencies ({d.dependencies.length})</dt>
              <dd>
                {d.dependencies.length === 0 ? (
                  <span className="text-muted-foreground">none reported</span>
                ) : (
                  <ul className="mt-1 space-y-0.5 font-mono text-xs">
                    {d.dependencies.map((x) => (
                      <li key={x} className="break-all">{x}</li>
                    ))}
                  </ul>
                )}
              </dd>
            </div>
          </dl>
        </>
      )}
    </aside>
  )
}

export function Explorer({ ops, onInstalledChange }: { ops: Operations; onInstalledChange?: unknown }) {
  const [managers, setManagers] = useState<SearchManager[] | null>(null)
  const [manager, setManager] = useState('')
  const [query, setQuery] = useState('')
  const [results, setResults] = useState<SearchResult[] | null>(null)
  const [searched, setSearched] = useState('')
  const [searching, setSearching] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [selected, setSelected] = useState<SearchResult | null>(null)

  useEffect(() => {
    let off = false
    api
      .getSearchManagers()
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

  // Re-run the last search after an operation changed what is installed, so the
  // installed badges stay correct.
  useEffect(() => {
    if (!searched || !manager) return
    let off = false
    api
      .searchPackages(manager, searched)
      .then((r) => !off && setResults(r))
      .catch(() => undefined)
    return () => {
      off = true
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [onInstalledChange])

  const run = async (e: React.FormEvent) => {
    e.preventDefault()
    if (!query.trim() || !manager) return
    setSearching(true)
    setError(null)
    setSelected(null)
    try {
      setResults(await api.searchPackages(manager, query))
      setSearched(query)
    } catch (err) {
      setResults(null)
      setError(String(err))
    } finally {
      setSearching(false)
    }
  }

  if (managers && managers.length === 0) {
    return (
      <div className="p-12 text-center text-sm text-muted-foreground">
        No searchable package managers were found on this machine (search is implemented for npm, APT, Snap and Flatpak).
      </div>
    )
  }

  return (
    <div className="flex h-full min-h-0">
      <div className="flex min-w-0 flex-1 flex-col p-6">
        <h1 className="text-xl font-semibold">Package Explorer</h1>
        <p className="mb-4 text-sm text-muted-foreground">
          Search a package manager's registry. Results come straight from the manager; nothing is merged across managers.
        </p>
        <form onSubmit={run} className="mb-4 flex flex-wrap items-center gap-2" aria-label="Search packages">
          <select aria-label="Package manager" className={selectCls} value={manager} onChange={(e) => setManager(e.target.value)}>
            {(managers ?? []).map((m) => (
              <option key={m.id} value={m.id}>{m.displayName}</option>
            ))}
          </select>
          <div className="relative min-w-48 flex-1">
            <Search className="pointer-events-none absolute top-2.5 left-2.5 size-4 text-muted-foreground" aria-hidden />
            <input
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="Search packages (e.g. is-odd, curl)"
              aria-label="Search query"
              className="h-9 w-full rounded-md border bg-background pr-3 pl-8 text-sm focus-visible:outline-2"
            />
          </div>
          <Button type="submit" disabled={searching || !query.trim() || !manager}>
            {searching ? 'Searching…' : 'Search'}
          </Button>
        </form>

        {error && <div role="alert" className="mb-3 rounded-md border border-destructive/40 bg-destructive/10 p-2 text-sm">{error}</div>}
        {results && results.length === 0 && (
          <div className="p-8 text-center text-sm text-muted-foreground">No packages found for “{searched}”.</div>
        )}
        {!results && !error && !searching && (
          <div className="p-8 text-center text-sm text-muted-foreground">Enter a name to search.</div>
        )}
        {results && results.length > 0 && (
          <div className="min-h-0 flex-1 overflow-auto">
            <table className="w-full text-sm">
              <thead className="sticky top-0 bg-background text-xs text-muted-foreground">
                <tr className="border-b text-left">
                  <th className="px-3 py-2 font-medium">Name</th>
                  <th className="px-3 py-2 font-medium">Version</th>
                  <th className="px-3 py-2 font-medium">Description</th>
                  <th className="px-3 py-2 font-medium">Status</th>
                  <th className="px-3 py-2" />
                </tr>
              </thead>
              <tbody>
                {results.map((r) => (
                  <tr
                    key={`${r.managerId}:${r.name}`}
                    tabIndex={0}
                    aria-selected={selected?.name === r.name}
                    onClick={() => setSelected(r)}
                    onKeyDown={(e) => (e.key === 'Enter' || e.key === ' ') && (e.preventDefault(), setSelected(r))}
                    className={`cursor-pointer border-b hover:bg-muted/60 focus-visible:outline-2 ${selected?.name === r.name ? 'bg-muted' : ''}`}
                  >
                    <td className="px-3 py-2 font-medium">{r.name}</td>
                    <td className="px-3 py-2 font-mono text-xs">{r.version ?? '—'}</td>
                    <td className="max-w-md truncate px-3 py-2 text-muted-foreground">{r.description ?? '—'}</td>
                    <td className="px-3 py-2">
                      {r.installed ? <Badge tone="success">Installed {r.installedVersion ?? ''}</Badge> : <Badge>Not installed</Badge>}
                    </td>
                    <td className="px-3 py-2 text-right">
                      {r.canInstall && !r.installed && (
                        <Button
                          size="sm"
                          variant="outline"
                          disabled={ops.busy}
                          onClick={(e) => {
                            e.stopPropagation()
                            void ops.preview(r.managerId, 'install', r.name)
                          }}
                        >
                          Install
                        </Button>
                      )}
                      {!r.canInstall && <span className="text-xs text-muted-foreground">install not supported yet</span>}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>
      {selected && <Details hit={selected} onClose={() => setSelected(null)} />}
    </div>
  )
}
