import { useEffect, useMemo, useState } from 'react'
import { AlertTriangle, CheckCircle2, CircleSlash, Package, RefreshCw, XCircle } from 'lucide-react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import type { Inventory } from '@/hooks/useInventory'
import { api } from '@/lib/api'
import { formatTimestamp, packagesNeedingAttention, recentlySeen } from '@/lib/inventory'
import { resultTone } from '@/lib/operations'
import { updateSeverity } from '@/lib/severity'
import type { OperationRecord } from '@/lib/types'

function Stat({ label, value, hint }: { label: string; value: string | number; hint?: string }) {
  return (
    <div className="rounded-lg border bg-card p-4">
      <div className="text-sm text-muted-foreground">{label}</div>
      <div className="mt-1 text-2xl font-semibold tabular-nums">{value}</div>
      {hint && <div className="mt-1 text-xs text-muted-foreground">{hint}</div>}
    </div>
  )
}

type NavTarget = 'updates' | 'history' | 'installed'

export function Overview({ inv, refreshKey, onOpen }: { inv: Inventory; refreshKey: unknown; onOpen: (p: NavTarget) => void }) {
  const { packages, managers, lastScan, scanning, progress } = inv
  const [recentOps, setRecentOps] = useState<OperationRecord[]>([])
  useEffect(() => {
    let off = false
    api
      .getOperations(5)
      .then((o) => !off && setRecentOps(o))
      .catch(() => !off && setRecentOps([]))
    return () => {
      off = true
    }
  }, [refreshKey])
  const attention = useMemo(() => packagesNeedingAttention(packages), [packages])
  const fresh = useMemo(() => recentlySeen(packages), [packages])
  const detected = managers.filter((m) => m.detected)
  const updates = packages.filter((p) => p.updateStatus === 'available').length
  const failed = managers.filter((m) => m.lastError)
  const scanned = lastScan !== null && lastScan.status !== 'running'
  const pct = progress && progress.total ? Math.round((progress.done / progress.total) * 100) : 0

  return (
    <div className="space-y-6">
      <header className="flex items-start justify-between gap-4">
        <div>
          <h1 className="text-xl font-semibold">Overview</h1>
          <p className="text-sm text-muted-foreground">
            Read-only inventory of software across your package managers.
          </p>
        </div>
        <Button onClick={inv.scan} disabled={scanning}>
          <RefreshCw className={scanning ? 'size-4 animate-spin' : 'size-4'} aria-hidden />
          {scanning ? 'Scanning…' : scanned ? 'Rescan' : 'Scan system'}
        </Button>
      </header>

      {scanning && (
        <div className="rounded-lg border bg-card p-4" role="status" aria-live="polite">
          <div className="mb-2 flex justify-between text-sm">
            <span>{progress?.message ?? 'Starting scan…'}</span>
            <span className="tabular-nums text-muted-foreground">{pct}%</span>
          </div>
          <div className="h-1.5 overflow-hidden rounded-full bg-muted">
            <div className="h-full bg-primary transition-all duration-300" style={{ width: `${pct}%` }} />
          </div>
        </div>
      )}

      {!scanned && !scanning ? (
        <div className="flex flex-col items-center gap-3 rounded-lg border border-dashed p-12 text-center">
          <Package className="size-8 text-muted-foreground" aria-hidden />
          <div className="font-medium">No scan yet</div>
          <p className="max-w-md text-sm text-muted-foreground">
            Run a scan to detect installed package managers and list their packages. Scanning only reads from package
            manager databases — nothing is changed on your system.
          </p>
        </div>
      ) : (
        <>
          <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
            <Stat label="Installed packages" value={packages.length} />
            <Stat
              label="Updates available"
              value={updates}
              hint="Only managers that report updates natively are counted"
            />
            <Stat label="Package managers" value={`${detected.length} / ${managers.length}`} hint="detected" />
            <Stat
              label="Last scan"
              value={lastScan?.status ?? '—'}
              hint={lastScan ? formatTimestamp(lastScan.finishedAt ?? lastScan.startedAt) : undefined}
            />
          </div>

          <section aria-labelledby="mgrs">
            <h2 id="mgrs" className="mb-2 text-sm font-medium">
              Package managers
            </h2>
            <ul className="divide-y rounded-lg border bg-card">
              {managers.map((m) => (
                <li key={m.id} className="flex items-center gap-3 px-4 py-3 text-sm">
                  {m.lastError ? (
                    <XCircle className="size-4 text-destructive" aria-label="Failed" />
                  ) : m.detected ? (
                    <CheckCircle2 className="size-4 text-success" aria-label="Detected" />
                  ) : (
                    <CircleSlash className="size-4 text-muted-foreground" aria-label="Not installed" />
                  )}
                  <span className="w-32 font-medium">{m.displayName}</span>
                  <span className="min-w-0 flex-1 truncate text-muted-foreground">
                    {m.lastError ?? (m.detected ? (m.version ?? m.path) : 'Not installed')}
                  </span>
                  {m.detected && !m.lastError && <Badge>{m.packageCount} packages</Badge>}
                  {m.detected && !m.supportsUpdates && <Badge tone="warning">no update info</Badge>}
                </li>
              ))}
            </ul>
          </section>

          <div className="grid gap-4 lg:grid-cols-3">
            <section aria-labelledby="attn" className="rounded-lg border bg-card p-4">
              <h2 id="attn" className="mb-2 text-sm font-medium">
                Needs attention
              </h2>
              {attention.length === 0 ? (
                <p className="text-xs text-muted-foreground">No updates reported.</p>
              ) : (
                <ul className="space-y-1 text-sm">
                  {attention.map((p) => (
                    <li key={p.id} className="flex items-center justify-between gap-2">
                      <span className="min-w-0 flex-1 truncate" title={p.name}>
                        {p.name}
                      </span>
                      <span className="flex max-w-[60%] min-w-0 items-center gap-1 font-mono text-xs">
                        <span className="truncate" title={`${p.version ?? '?'} → ${p.latestVersion}`}>
                          {p.version ?? '?'} → {p.latestVersion}
                        </span>
                        {updateSeverity(p.version, p.latestVersion) === 'major' && <Badge tone="danger">major</Badge>}
                      </span>
                    </li>
                  ))}
                </ul>
              )}
              {updates > 0 && (
                <Button variant="ghost" size="sm" className="mt-2" onClick={() => onOpen('updates')}>
                  Open Update Center ({updates})
                </Button>
              )}
            </section>

            <section aria-labelledby="fresh" className="rounded-lg border bg-card p-4">
              <h2 id="fresh" className="mb-2 text-sm font-medium">
                Recently seen
              </h2>
              {fresh.length === 0 ? (
                <p className="text-xs text-muted-foreground">No new packages since the first scan.</p>
              ) : (
                <ul className="space-y-1 text-sm">
                  {fresh.map((p) => (
                    <li key={p.id} className="flex items-center justify-between gap-2">
                      <span className="truncate">{p.name}</span>
                      <span className="text-xs whitespace-nowrap text-muted-foreground">{formatTimestamp(p.firstSeenAt)}</span>
                    </li>
                  ))}
                </ul>
              )}
              <p className="mt-2 text-xs text-muted-foreground">First seen by UniManage, not an install date.</p>
            </section>

            <section aria-labelledby="ops" className="rounded-lg border bg-card p-4">
              <h2 id="ops" className="mb-2 text-sm font-medium">
                Recent operations
              </h2>
              {recentOps.length === 0 ? (
                <p className="text-xs text-muted-foreground">Nothing has been run yet.</p>
              ) : (
                <ul className="space-y-1 text-sm">
                  {recentOps.map((o) => (
                    <li key={o.id} className="flex items-center justify-between gap-2">
                      <span className="truncate">
                        {o.operationType} {o.packageName}
                      </span>
                      <Badge tone={resultTone(o.result)}>{o.result}</Badge>
                    </li>
                  ))}
                </ul>
              )}
              <Button variant="ghost" size="sm" className="mt-2" onClick={() => onOpen('history')}>
                Open history
              </Button>
            </section>
          </div>

          {failed.length > 0 && (
            <div className="flex gap-2 rounded-lg border border-warning/40 bg-warning/10 p-3 text-sm" role="alert">
              <AlertTriangle className="mt-0.5 size-4 shrink-0 text-warning" aria-hidden />
              <span>
                {failed.length} package manager{failed.length > 1 ? 's' : ''} failed to report. Previously scanned data
                is kept where available.
              </span>
            </div>
          )}
        </>
      )}
    </div>
  )
}
