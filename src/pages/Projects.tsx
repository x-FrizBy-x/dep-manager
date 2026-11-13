import { useEffect, useMemo, useState } from 'react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { api } from '@/lib/api'
import type { Ecosystem, ProjectDep, ProjectScan } from '@/lib/types'

const GROUP_ORDER = ['runtime', 'dev', 'build', 'peer', 'optional']
const rank = (g: string) => {
  const i = GROUP_ORDER.indexOf(g)
  return i === -1 ? GROUP_ORDER.length : i
}

/** Describes how an npm dependency's installed version relates to the lockfile. */
function npmState(d: ProjectDep, hasModules: boolean): { label: string; tone: 'success' | 'warning' | 'neutral' } | null {
  if (!hasModules) return null
  if (d.installed === null) return { label: 'not installed', tone: 'warning' }
  if (d.resolved && d.installed !== d.resolved) return { label: 'differs from lockfile', tone: 'warning' }
  return { label: 'installed', tone: 'success' }
}

function EcosystemTable({ eco }: { eco: Ecosystem }) {
  const [query, setQuery] = useState('')
  const hasModules = eco.id === 'npm' && !eco.notes.some((n) => n.startsWith('node_modules not found'))
  const rows = useMemo(() => {
    const q = query.trim().toLowerCase()
    return [...eco.dependencies]
      .filter((d) => !q || d.name.toLowerCase().includes(q))
      .sort((a, b) => rank(a.group) - rank(b.group) || a.name.localeCompare(b.name))
  }, [eco.dependencies, query])

  return (
    <section aria-label={eco.label} className="mb-6">
      <div className="mb-1 flex flex-wrap items-center gap-2">
        <h2 className="text-base font-semibold">{eco.label}</h2>
        <span className="text-xs text-muted-foreground">
          {eco.files.join(', ')}
          {eco.lockfile ? ` · locked by ${eco.lockfile}` : ''} · {eco.dependencies.length} declared
        </span>
        {eco.dependencies.length > 8 && (
          <input
            type="search"
            aria-label={`Filter ${eco.label}`}
            placeholder="Filter"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            className="ml-auto h-8 w-40 rounded-md border bg-background px-2 text-sm"
          />
        )}
      </div>
      {eco.error && (
        <div role="alert" className="mb-2 rounded-md border border-destructive/40 bg-destructive/10 p-2 text-sm">
          {eco.error}
        </div>
      )}
      {eco.notes.length > 0 && (
        <ul className="mb-2 list-disc space-y-0.5 pl-5 text-xs text-muted-foreground">
          {eco.notes.map((n) => (
            <li key={n}>{n}</li>
          ))}
        </ul>
      )}
      {rows.length > 0 && (
        <table className="w-full text-sm">
          <thead className="text-xs text-muted-foreground">
            <tr className="border-b text-left">
              <th className="px-2 py-1 font-medium">Package</th>
              <th className="px-2 py-1 font-medium">Group</th>
              <th className="px-2 py-1 font-medium">Declared</th>
              <th className="px-2 py-1 font-medium">Locked</th>
              {eco.id === 'npm' && <th className="px-2 py-1 font-medium">In node_modules</th>}
            </tr>
          </thead>
          <tbody>
            {rows.map((d) => {
              const st = eco.id === 'npm' ? npmState(d, hasModules) : null
              return (
                <tr key={`${d.group}:${d.name}`} className="border-b">
                  <td className="px-2 py-1 break-all">{d.name}</td>
                  <td className="px-2 py-1">
                    <Badge>{d.group}</Badge>
                  </td>
                  <td className="px-2 py-1 font-mono text-xs break-all">{d.spec ?? '—'}</td>
                  <td className="px-2 py-1 font-mono text-xs">{d.resolved ?? '—'}</td>
                  {eco.id === 'npm' && (
                    <td className="px-2 py-1">
                      <span className="font-mono text-xs">{d.installed ?? '—'}</span>{' '}
                      {st && <Badge tone={st.tone}>{st.label}</Badge>}
                    </td>
                  )}
                </tr>
              )
            })}
          </tbody>
        </table>
      )}
    </section>
  )
}

export function Projects() {
  const [path, setPath] = useState('')
  const [saved, setSaved] = useState<string[]>([])
  const [scan, setScan] = useState<ProjectScan | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    api
      .getProjects()
      .then(setSaved)
      .catch((e) => setError(String(e)))
  }, [])

  const open = async (p: string) => {
    setBusy(true)
    setError(null)
    try {
      const s = await api.scanProject(p)
      setScan(s)
      setPath(s.path)
      setSaved(await api.getProjects())
    } catch (e) {
      setError(String(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="flex h-full min-h-0 flex-col p-6">
      <h1 className="text-xl font-semibold">Project Dependencies</h1>
      <p className="mb-3 text-sm text-muted-foreground">
        What a project folder declares and what its lockfile and <code>node_modules</code> say. This is separate from your globally
        installed packages. UniManage only reads the manifest files in the folder (not sub-folders); it never runs anything in the project and
        never installs.
      </p>

      <form
        className="mb-3 flex flex-wrap items-center gap-2"
        onSubmit={(e) => {
          e.preventDefault()
          if (path.trim()) void open(path)
        }}
      >
        <input
          aria-label="Project folder"
          value={path}
          onChange={(e) => setPath(e.target.value)}
          placeholder="Project folder, e.g. C:\Users\you\code\app or /home/you/app"
          className="h-9 min-w-72 flex-1 rounded-md border bg-background px-2 font-mono text-xs"
        />
        <Button type="submit" size="sm" disabled={busy || !path.trim()}>
          {busy ? 'Reading…' : 'Read dependencies'}
        </Button>
      </form>

      {saved.length > 0 && (
        <ul aria-label="Saved projects" className="mb-3 flex flex-wrap gap-1.5">
          {saved.map((p) => (
            <li key={p} className="flex items-center rounded-full border text-xs">
              <button className="max-w-72 cursor-pointer truncate px-2.5 py-1 hover:bg-muted" title={p} onClick={() => void open(p)}>
                {p}
              </button>
              <button
                aria-label={`Forget ${p}`}
                title="Forget this folder (nothing is deleted)"
                className="cursor-pointer px-2 py-1 text-muted-foreground hover:text-foreground"
                onClick={() =>
                  void api
                    .removeProject(p)
                    .then(setSaved)
                    .catch((e) => setError(String(e)))
                }
              >
                ×
              </button>
            </li>
          ))}
        </ul>
      )}

      {error && (
        <div role="alert" className="mb-3 rounded-lg border border-destructive/40 bg-destructive/10 p-3 text-sm">
          {error}
        </div>
      )}

      <div className="min-h-0 flex-1 overflow-auto">
        {!scan ? (
          <p className="text-sm text-muted-foreground">Enter a project folder to read its declared dependencies.</p>
        ) : (
          <>
            <h2 className="mb-1 text-lg font-semibold break-all">{scan.name}</h2>
            <p className="mb-3 text-xs break-all text-muted-foreground">{scan.path}</p>
            {scan.ecosystems.length === 0 && (
              <p className="text-sm text-muted-foreground">
                No supported manifest found here (package.json, requirements.txt, pyproject.toml, Cargo.toml).
              </p>
            )}
            {scan.ecosystems.map((e) => (
              <EcosystemTable key={e.id} eco={e} />
            ))}
            {scan.unsupported.length > 0 && (
              <p className="text-xs text-muted-foreground">
                Also found but not supported yet: {scan.unsupported.join(', ')}.
              </p>
            )}
          </>
        )}
      </div>
    </div>
  )
}
