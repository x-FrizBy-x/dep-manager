import { RefreshCw } from 'lucide-react'
import { useMemo, useState } from 'react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import type { Inventory } from '@/hooks/useInventory'
import { OPERABLE_MANAGERS, type Operations } from '@/hooks/useOperations'
import { resultTone } from '@/lib/operations'
import { updateSeverity, type Severity } from '@/lib/severity'

const SEVERITY_TONE: Record<Severity, 'danger' | 'warning' | 'success' | 'neutral'> = {
  major: 'danger',
  minor: 'warning',
  patch: 'success',
  unknown: 'neutral',
}

export function Updates({ inv, ops }: { inv: Inventory; ops: Operations }) {
  const [picked, setPicked] = useState<Set<string>>(new Set())

  const updates = useMemo(
    () =>
      inv.packages
        .filter((p) => p.updateStatus === 'available')
        .map((p) => ({
          p,
          operable: OPERABLE_MANAGERS.has(p.packageManager),
          severity: updateSeverity(p.version, p.latestVersion),
        })),
    [inv.packages],
  )
  const selectable = updates.filter((u) => u.operable)
  const chosen = selectable.filter((u) => picked.has(u.p.id))
  const batch = ops.batch
  const running = batch?.phase === 'running'
  const toggle = (id: string) =>
    setPicked((s) => {
      const n = new Set(s)
      if (n.has(id)) n.delete(id)
      else n.add(id)
      return n
    })
  const allPicked = selectable.length > 0 && chosen.length === selectable.length

  return (
    <div className="flex h-full min-h-0 flex-col p-6">
      <div className="flex items-start justify-between gap-4">
        <div>
          <h1 className="text-xl font-semibold">Update Center</h1>
          <p className="mb-4 text-sm text-muted-foreground">
            Pick the updates you want. You review every command before anything runs; there is no automatic "update all".
          </p>
        </div>
        <Button variant="outline" size="sm" onClick={() => void inv.scan()} disabled={inv.scanning || ops.busy}>
          <RefreshCw className={`size-4 ${inv.scanning ? 'animate-spin' : ''}`} aria-hidden />
          Check for updates
        </Button>
      </div>

      {batch && batch.phase !== 'review' && (
        <section aria-label="Batch progress" className="mb-4 rounded-lg border bg-card p-3 text-sm">
          <div className="mb-2 flex items-center gap-2 font-medium">
            {running ? `Updating ${batch.current + 1} of ${batch.plans.length}…` : 'Batch finished'}
            {batch.stopped && <Badge tone="warning">stopped</Badge>}
            {!running && (
              <Button variant="ghost" size="sm" className="ml-auto" onClick={ops.discardBatch}>
                Dismiss
              </Button>
            )}
          </div>
          <ul className="space-y-1">
            {batch.plans.map((plan, i) => {
              const r = batch.results[i]
              const status = r ? (r.record ? r.record.result : 'failed') : running && i === batch.current ? 'running' : 'skipped'
              const problem = r?.error ?? r?.record?.error
              return (
                <li key={plan.id} className="flex items-center gap-2">
                  <Badge tone={status === 'skipped' ? 'neutral' : resultTone(status as never)}>{status}</Badge>
                  <span className="font-medium">{plan.package}</span>
                  {r?.record?.newVersion && (
                    <span className="font-mono text-xs text-muted-foreground">→ {r.record.newVersion}</span>
                  )}
                  {problem && <span className="truncate text-xs text-destructive">{problem}</span>}
                </li>
              )
            })}
          </ul>
        </section>
      )}

      {updates.length === 0 ? (
        <div className="p-12 text-center text-sm text-muted-foreground">
          {inv.packages.length === 0
            ? 'No inventory yet. Run a scan from Overview first.'
            : 'No updates available. Packages from managers without update checks (e.g. pipx, uv) are not listed.'}
        </div>
      ) : (
        <>
          <div className="mb-2 flex items-center gap-3">
            <Button
              size="sm"
              disabled={chosen.length === 0 || ops.busy}
              onClick={() => void ops.previewBatch(chosen.map((u) => ({ managerId: u.p.packageManager, pkg: u.p.name })))}
            >
              Review {chosen.length} selected update{chosen.length === 1 ? '' : 's'}
            </Button>
            <span className="text-xs text-muted-foreground">{updates.length} available</span>
          </div>
          <div className="min-h-0 flex-1 overflow-auto">
            <table className="w-full text-sm">
              <thead className="sticky top-0 bg-background text-xs text-muted-foreground">
                <tr className="border-b text-left">
                  <th className="w-8 px-3 py-2">
                    <input
                      type="checkbox"
                      aria-label="Select all supported updates"
                      checked={allPicked}
                      disabled={selectable.length === 0 || running}
                      onChange={() => setPicked(allPicked ? new Set() : new Set(selectable.map((u) => u.p.id)))}
                    />
                  </th>
                  <th className="px-3 py-2 font-medium">Package</th>
                  <th className="px-3 py-2 font-medium">Manager</th>
                  <th className="px-3 py-2 font-medium">Version</th>
                  <th className="px-3 py-2 font-medium">Size</th>
                </tr>
              </thead>
              <tbody>
                {updates.map(({ p, operable, severity }) => (
                  <tr key={p.id} className={`border-b ${operable ? '' : 'opacity-60'}`}>
                    <td className="px-3 py-2">
                      <input
                        type="checkbox"
                        aria-label={`Select ${p.name}`}
                        checked={operable && picked.has(p.id)}
                        disabled={!operable || running}
                        onChange={() => toggle(p.id)}
                      />
                    </td>
                    <td className="px-3 py-2 font-medium">{p.displayName ?? p.name}</td>
                    <td className="px-3 py-2">
                      <Badge>{p.packageManager}</Badge>
                      {!operable && <span className="ml-2 text-xs text-muted-foreground">updates not supported yet</span>}
                    </td>
                    <td className="px-3 py-2 font-mono text-xs">
                      {p.version ?? '—'} → {p.latestVersion ?? '—'}
                    </td>
                    <td className="px-3 py-2">
                      <Badge tone={SEVERITY_TONE[severity]}>{severity}</Badge>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <p className="mt-2 text-xs text-muted-foreground">
            Size is derived from the version numbers only. Changelogs and security flags are not available from these managers.
          </p>
        </>
      )}
    </div>
  )
}
