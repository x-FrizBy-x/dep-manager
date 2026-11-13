import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import type { Inventory } from '@/hooks/useInventory'
import { VERSION_PINNABLE, type Operations } from '@/hooks/useOperations'
import { api } from '@/lib/api'
import { resultTone } from '@/lib/operations'
import type { DiffEntry, ManifestDiff, ManifestSummary } from '@/lib/types'

const fmt = (t: number) => new Date(t * 1000).toLocaleString()
const keyOf = (e: DiffEntry) => `${e.managerId}\u0000${e.name}`

/** Hands text to the user as a file download. */
function saveText(filename: string, text: string, mime: string) {
  const url = URL.createObjectURL(new Blob([text], { type: mime }))
  const a = document.createElement('a')
  a.href = url
  a.download = filename
  a.click()
  setTimeout(() => URL.revokeObjectURL(url), 1000)
}

const slug = (s: string) => s.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '') || 'manifest'

export function Backup({ inv, ops }: { inv: Inventory; ops: Operations }) {
  const [list, setList] = useState<ManifestSummary[] | null>(null)
  const [name, setName] = useState(() => `Environment ${new Date().toISOString().slice(0, 10)}`)
  const [selected, setSelected] = useState<number | null>(null)
  const [diff, setDiff] = useState<ManifestDiff | null>(null)
  const [picked, setPicked] = useState<Set<string>>(new Set())
  const [confirmDelete, setConfirmDelete] = useState<number | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  const file = useRef<HTMLInputElement>(null)

  const run = useCallback(async (f: () => Promise<void>) => {
    setError(null)
    setNotice(null)
    try {
      await f()
    } catch (e) {
      setError(String(e))
    }
  }, [])

  const reloadList = useCallback(async () => setList(await api.listManifests()), [])
  useEffect(() => {
    api
      .listManifests()
      .then(setList)
      .catch((e) => setError(String(e)))
  }, [])

  // Re-compare whenever the inventory changes (e.g. after a restore operation finished).
  useEffect(() => {
    if (selected === null) return
    let off = false
    api
      .diffManifest(selected)
      .then((d) => {
        if (off) return
        setDiff(d)
        setPicked((p) => {
          const live = new Set(d.managers.flatMap((m) => m.entries.filter((e) => e.restorable).map(keyOf)))
          return new Set([...p].filter((k) => live.has(k)))
        })
      })
      .catch((e) => !off && setError(String(e)))
    return () => {
      off = true
    }
  }, [selected, inv.packages])

  const restorable = useMemo(() => diff?.managers.flatMap((m) => m.entries.filter((e) => e.restorable)) ?? [], [diff])
  const chosen = restorable.filter((e) => picked.has(keyOf(e)))
  const batch = ops.batch
  const running = batch?.phase === 'running'

  const exportAs = (m: ManifestSummary, format: 'json' | 'yaml') =>
    run(async () => {
      const text = await api.exportManifest(m.id, format)
      saveText(`${slug(m.name)}.${format}`, text, format === 'json' ? 'application/json' : 'text/yaml')
      setNotice(`Exported ${m.name} as ${format.toUpperCase()}.`)
    })

  const onImport = (f: File | undefined) => {
    if (!f) return
    void run(async () => {
      const m = await api.importManifest(await f.text())
      await reloadList()
      setSelected(m.id)
      setNotice(`Imported "${m.name}" (${m.packageCount} packages). Nothing was installed.`)
    })
    if (file.current) file.current.value = ''
  }

  return (
    <div className="flex h-full min-h-0 flex-col p-6">
      <h1 className="text-xl font-semibold">Environment Backup</h1>
      <p className="mb-3 text-sm text-muted-foreground">
        A manifest records what each package manager has installed (apt: manually installed only; runtimes skipped). It is not an exact
        reproduction guarantee, and restoring never removes anything. Restore is available only for managers with an install pipeline
        (npm, pipx and per-user Flatpak; Flatpak restores the app, not a pinned version); every restore goes through the usual preview and confirmation.
      </p>

      {error && (
        <div role="alert" className="mb-3 rounded-lg border border-destructive/40 bg-destructive/10 p-3 text-sm">
          {error}
        </div>
      )}
      {notice && (
        <div role="status" className="mb-3 rounded-lg border bg-muted p-3 text-sm">
          {notice}
        </div>
      )}

      <div className="mb-4 flex flex-wrap items-center gap-2">
        <input
          aria-label="Manifest name"
          value={name}
          maxLength={100}
          onChange={(e) => setName(e.target.value)}
          className="h-8 w-64 rounded-md border bg-background px-2 text-sm"
        />
        <Button
          size="sm"
          disabled={!name.trim() || inv.packages.length === 0}
          onClick={() =>
            void run(async () => {
              const m = await api.createManifest(name)
              await reloadList()
              setSelected(m.id)
              setNotice(`Saved "${m.name}" from the last scan (${m.packageCount} packages).`)
            })
          }
        >
          Create from current inventory
        </Button>
        <Button size="sm" variant="outline" onClick={() => file.current?.click()}>
          Import JSON…
        </Button>
        <input ref={file} type="file" accept="application/json,.json" hidden onChange={(e) => onImport(e.target.files?.[0])} />
        {inv.packages.length === 0 && <span className="text-xs text-muted-foreground">Run a scan from Overview first.</span>}
      </div>

      <div className="grid min-h-0 flex-1 gap-4 lg:grid-cols-[22rem_1fr]">
        <section aria-label="Saved manifests" className="min-h-0 overflow-auto">
          {list === null ? (
            <p className="text-sm text-muted-foreground" role="status">
              Loading…
            </p>
          ) : list.length === 0 ? (
            <p className="text-sm text-muted-foreground">No manifests yet.</p>
          ) : (
            <ul className="space-y-2">
              {list.map((m) => (
                <li key={m.id} className={`rounded-lg border p-3 text-sm ${selected === m.id ? 'bg-muted' : ''}`}>
                  <div className="font-medium break-all">{m.name}</div>
                  <div className="text-xs text-muted-foreground">
                    {fmt(m.createdAt)} · {m.platform} · {m.packageCount} packages · {m.managers.join(', ') || 'none'}
                  </div>
                  <div className="mt-2 flex flex-wrap gap-1">
                    <Button size="sm" variant="outline" onClick={() => setSelected(m.id)}>
                      Compare
                    </Button>
                    <Button size="sm" variant="ghost" onClick={() => void exportAs(m, 'json')}>
                      JSON
                    </Button>
                    <Button size="sm" variant="ghost" onClick={() => void exportAs(m, 'yaml')}>
                      YAML
                    </Button>
                    {confirmDelete === m.id ? (
                      <>
                        <Button
                          size="sm"
                          variant="outline"
                          onClick={() =>
                            void run(async () => {
                              await api.deleteManifest(m.id)
                              setConfirmDelete(null)
                              if (selected === m.id) {
                                setSelected(null)
                                setDiff(null)
                              }
                              await reloadList()
                            })
                          }
                        >
                          Confirm delete
                        </Button>
                        <Button size="sm" variant="ghost" onClick={() => setConfirmDelete(null)}>
                          Keep
                        </Button>
                      </>
                    ) : (
                      <Button size="sm" variant="ghost" onClick={() => setConfirmDelete(m.id)}>
                        Delete
                      </Button>
                    )}
                  </div>
                </li>
              ))}
            </ul>
          )}
        </section>

        <section aria-label="Manifest comparison" className="min-h-0 overflow-auto">
          {!diff ? (
            <p className="text-sm text-muted-foreground">Select a manifest and press Compare to see how it differs from this machine.</p>
          ) : (
            <>
              <h2 className="text-base font-semibold break-all">{diff.name}</h2>
              <p className="mb-2 text-xs text-muted-foreground">Compared with the last scan. Packages not in the manifest are only counted.</p>

              {batch && batch.phase !== 'review' && (
                <div aria-label="Restore progress" className="mb-3 rounded-lg border bg-card p-3 text-sm">
                  <div className="font-medium">
                    {running ? `Restoring ${batch.current + 1} of ${batch.plans.length}…` : 'Restore finished'}
                    {batch.stopped && <Badge tone="warning" className="ml-2">stopped</Badge>}
                    {!running && (
                      <Button variant="ghost" size="sm" className="ml-2" onClick={ops.discardBatch}>
                        Dismiss
                      </Button>
                    )}
                  </div>
                  <ul className="mt-1 space-y-0.5">
                    {batch.plans.map((plan, i) => {
                      const r = batch.results[i]
                      const status = r ? (r.record ? r.record.result : 'failed') : running && i === batch.current ? 'running' : 'skipped'
                      return (
                        <li key={plan.id} className="flex items-center gap-2">
                          <span className="break-all">{plan.package}</span>
                          <Badge tone={r?.record ? resultTone(r.record.result) : 'neutral'}>{status}</Badge>
                        </li>
                      )
                    })}
                  </ul>
                </div>
              )}

              <div className="mb-3 flex items-center gap-3">
                <Button
                  size="sm"
                  disabled={chosen.length === 0 || ops.busy}
                  onClick={() =>
                    void ops.previewBatch(
                      chosen.map((e) => ({ managerId: e.managerId, pkg: e.name, kind: 'install', version: VERSION_PINNABLE.has(e.managerId) ? (e.wantedVersion ?? undefined) : undefined })),
                    )
                  }
                >
                  Review {chosen.length} restore{chosen.length === 1 ? '' : 's'}
                </Button>
                {restorable.length > 0 && (
                  <Button
                    size="sm"
                    variant="ghost"
                    disabled={running}
                    onClick={() => setPicked(chosen.length === restorable.length ? new Set() : new Set(restorable.map(keyOf)))}
                  >
                    {chosen.length === restorable.length ? 'Select none' : 'Select all restorable'}
                  </Button>
                )}
              </div>

              {diff.managers.map((m) => (
                <div key={m.managerId} className="mb-4">
                  <h3 className="text-sm font-medium">
                    {m.managerId}{' '}
                    <span className="font-normal text-muted-foreground">
                      · {m.same} match · {m.entries.length} to restore · {m.extra} extra on this machine
                    </span>
                    {!m.detected && <Badge tone="warning" className="ml-2">not detected</Badge>}
                  </h3>
                  {m.entries.length === 0 ? (
                    <p className="text-xs text-muted-foreground">Nothing differs.</p>
                  ) : (
                    <table className="mt-1 w-full text-sm">
                      <thead className="text-xs text-muted-foreground">
                        <tr className="border-b text-left">
                          <th className="w-8 px-2 py-1" />
                          <th className="px-2 py-1 font-medium">Package</th>
                          <th className="px-2 py-1 font-medium">Manifest</th>
                          <th className="px-2 py-1 font-medium">Installed</th>
                          <th className="px-2 py-1 font-medium">Status</th>
                        </tr>
                      </thead>
                      <tbody>
                        {m.entries.map((e) => (
                          <tr key={keyOf(e)} className={`border-b ${e.restorable ? '' : 'opacity-60'}`}>
                            <td className="px-2 py-1">
                              <input
                                type="checkbox"
                                aria-label={`Restore ${e.name}`}
                                disabled={!e.restorable || running}
                                checked={picked.has(keyOf(e))}
                                onChange={() =>
                                  setPicked((p) => {
                                    const n = new Set(p)
                                    if (!n.delete(keyOf(e))) n.add(keyOf(e))
                                    return n
                                  })
                                }
                              />
                            </td>
                            <td className="px-2 py-1 break-all">{e.name}</td>
                            <td className="px-2 py-1 font-mono text-xs">{e.wantedVersion ?? '—'}</td>
                            <td className="px-2 py-1 font-mono text-xs">{e.installedVersion ?? '—'}</td>
                            <td className="px-2 py-1">
                              <Badge tone={e.status === 'missing' ? 'danger' : 'warning'}>{e.status}</Badge>
                              {e.reason && <div className="text-xs text-muted-foreground">{e.reason}</div>}
                            </td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  )}
                </div>
              ))}
            </>
          )}
        </section>
      </div>
    </div>
  )
}
