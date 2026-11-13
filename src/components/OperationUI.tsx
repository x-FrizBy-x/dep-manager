import { Terminal, X } from 'lucide-react'
import { useEffect, useRef } from 'react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import type { Operations } from '@/hooks/useOperations'
import { resultTone } from '@/lib/operations'

const KIND_LABEL = { install: 'Install', update: 'Update', uninstall: 'Uninstall' } as const

/** Preview + explicit confirmation. Nothing runs until "Run" is pressed. */
export function ConfirmDialog({ ops }: { ops: Operations }) {
  const { plan } = ops
  const ref = useRef<HTMLButtonElement>(null)
  useEffect(() => {
    if (plan) ref.current?.focus()
  }, [plan])
  if (!plan) return null
  const destructive = plan.kind === 'uninstall'
  return (
    <div className="fixed inset-0 z-50 grid place-items-center bg-black/50 p-4" onKeyDown={(e) => e.key === 'Escape' && ops.discardPlan()}>
      <div role="dialog" aria-modal="true" aria-labelledby="op-title" className="w-full max-w-lg rounded-xl border bg-card p-5 shadow-xl">
        <h2 id="op-title" className="text-lg font-semibold">
          {KIND_LABEL[plan.kind]} {plan.package}?
        </h2>
        <dl className="mt-3 grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-sm">
          <dt className="text-muted-foreground">Manager</dt>
          <dd>{plan.managerId}</dd>
          <dt className="text-muted-foreground">Package</dt>
          <dd className="break-all">{plan.package}</dd>
          {plan.kind !== 'uninstall' && (
            <>
              <dt className="text-muted-foreground">Version</dt>
              <dd>
                {plan.previousVersion ?? 'not installed'} → {plan.version ?? 'latest'}
              </dd>
            </>
          )}
          <dt className="text-muted-foreground">Elevation</dt>
          <dd>{plan.requiresElevation ? 'Required: a system password dialog (polkit) will appear' : 'Not required'}</dd>
        </dl>
        <div className="mt-3 text-xs text-muted-foreground">Exact command that will run (no shell):</div>
        <pre className="mt-1 overflow-x-auto rounded-md bg-muted p-2 font-mono text-xs">{plan.command}</pre>
        {plan.effects.length > 0 && (
          <div className="mt-3">
            <div className="text-xs text-muted-foreground">Dry run: what would change ({plan.effects.length})</div>
            <ul className="mt-1 max-h-40 overflow-auto rounded-md bg-muted p-2 font-mono text-xs">
              {plan.effects.map((e) => (
                <li key={e} className={e.startsWith('Remv') || e.startsWith('Purg') ? 'text-destructive' : ''}>
                  {e}
                </li>
              ))}
            </ul>
          </div>
        )}
        {plan.warnings.length > 0 && (
          <ul className="mt-3 list-disc space-y-1 rounded-md bg-warning/10 p-2 pl-6 text-xs">
            {plan.warnings.map((w) => (
              <li key={w}>{w}</li>
            ))}
          </ul>
        )}
        <div className="mt-5 flex justify-end gap-2">
          <Button ref={ref} variant="outline" onClick={ops.discardPlan}>
            Cancel
          </Button>
          <Button variant={destructive ? 'destructive' : 'default'} onClick={() => void ops.confirm()}>
            Run {KIND_LABEL[plan.kind].toLowerCase()}
          </Button>
        </div>
      </div>
    </div>
  )
}

/** Review of a multi-package update. Nothing runs until "Run N updates" is pressed. */
export function BatchDialog({ ops }: { ops: Operations }) {
  const batch = ops.batch
  const ref = useRef<HTMLButtonElement>(null)
  const open = batch?.phase === 'review'
  useEffect(() => {
    if (open) ref.current?.focus()
  }, [open])
  if (!batch || batch.phase !== 'review') return null
  const warnings = [...new Set(batch.plans.flatMap((p) => p.warnings))]
  const n = batch.plans.length
  const kinds = new Set(batch.plans.map((p) => p.kind))
  const noun = kinds.size === 1 ? KIND_LABEL[batch.plans[0].kind].toLowerCase() : 'operation'
  return (
    <div className="fixed inset-0 z-50 grid place-items-center bg-black/50 p-4" onKeyDown={(e) => e.key === 'Escape' && ops.discardBatch()}>
      <div role="dialog" aria-modal="true" aria-labelledby="batch-title" className="flex max-h-[85vh] w-full max-w-2xl flex-col rounded-xl border bg-card p-5 shadow-xl">
        <h2 id="batch-title" className="text-lg font-semibold">
          Run {n} {noun}{n === 1 ? '' : 's'}?
        </h2>
        <p className="mt-1 text-sm text-muted-foreground">
          They run one at a time, in this order. A failure does not stop the rest; Cancel stops the batch. Nothing is removed.
        </p>
        <ul className="mt-3 min-h-0 flex-1 space-y-2 overflow-auto">
          {batch.plans.map((p) => (
            <li key={p.id} className="rounded-md border p-2 text-sm">
              <div className="flex items-center justify-between gap-2">
                <span className="font-medium break-all">{p.package}</span>
                <span className="font-mono text-xs whitespace-nowrap">
                  {p.previousVersion ?? '?'} → {p.version ?? 'latest'}
                </span>
              </div>
              <pre className="mt-1 overflow-x-auto font-mono text-xs text-muted-foreground">{p.command}</pre>
              {p.requiresElevation && <Badge tone="warning">needs elevation</Badge>}
            </li>
          ))}
        </ul>
        {warnings.length > 0 && (
          <ul className="mt-3 list-disc space-y-1 rounded-md bg-warning/10 p-2 pl-6 text-xs">
            {warnings.map((w) => (
              <li key={w}>{w}</li>
            ))}
          </ul>
        )}
        <div className="mt-4 flex justify-end gap-2">
          <Button ref={ref} variant="outline" onClick={ops.discardBatch}>
            Cancel
          </Button>
          <Button onClick={() => void ops.confirmBatch()}>
            Run {n} {noun}{n === 1 ? '' : 's'}
          </Button>
        </div>
      </div>
    </div>
  )
}

/** Terminal-style live output for the running / last operation. */
export function OperationConsole({ ops }: { ops: Operations }) {
  const { active } = ops
  const box = useRef<HTMLPreElement>(null)
  const count = active?.lines.length ?? 0
  useEffect(() => {
    if (box.current) box.current.scrollTop = box.current.scrollHeight
  }, [count])
  if (!active) return null
  const rec = active.record
  return (
    <section aria-label="Operation console" className="flex h-56 shrink-0 flex-col border-t bg-card">
      <header className="flex items-center gap-2 border-b px-3 py-1.5 text-sm">
        <Terminal className="size-4" aria-hidden />
        <span className="font-medium">
          {KIND_LABEL[active.plan.kind]} {active.plan.package}
        </span>
        <Badge tone={rec ? resultTone(rec.result) : ops.cancelling ? 'warning' : 'primary'}>
          {rec?.result ?? (ops.cancelling ? 'cancelling…' : 'running')}
        </Badge>
        {rec && rec.newVersion && rec.result === 'success' && (
          <span className="text-xs text-muted-foreground">now {rec.newVersion}</span>
        )}
        <span className="flex-1" />
        {rec ? (
          <Button variant="ghost" size="icon" className="size-7" onClick={ops.dismiss} aria-label="Close console">
            <X className="size-4" />
          </Button>
        ) : active.plan.cancellable ? (
          <Button variant="outline" size="sm" disabled={ops.cancelling} onClick={() => void ops.cancelRun()}>
            Cancel operation
          </Button>
        ) : (
          <span className="text-xs text-muted-foreground">Runs as root: cannot be cancelled</span>
        )}
      </header>
      <pre ref={box} className="min-h-0 flex-1 overflow-auto p-3 font-mono text-xs leading-5 break-all whitespace-pre-wrap" role="log" aria-live="polite">
        {active.lines.map((l, i) => (
          <div key={i} className={l.stream === 'stderr' ? 'text-warning' : l.stream === 'info' ? 'text-muted-foreground' : ''}>
            {l.line}
          </div>
        ))}
      </pre>
    </section>
  )
}
