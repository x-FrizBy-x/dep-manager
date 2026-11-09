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
          <dd>{plan.requiresElevation ? 'Required' : 'Not required'}</dd>
        </dl>
        <div className="mt-3 text-xs text-muted-foreground">Exact command that will run (no shell):</div>
        <pre className="mt-1 overflow-x-auto rounded-md bg-muted p-2 font-mono text-xs">{plan.command}</pre>
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
        <Badge tone={resultTone(rec?.result ?? 'running')}>{rec?.result ?? 'running'}</Badge>
        {rec && rec.newVersion && rec.result === 'success' && (
          <span className="text-xs text-muted-foreground">now {rec.newVersion}</span>
        )}
        <span className="flex-1" />
        {rec ? (
          <Button variant="ghost" size="icon" className="size-7" onClick={ops.dismiss} aria-label="Close console">
            <X className="size-4" />
          </Button>
        ) : (
          <Button variant="outline" size="sm" onClick={() => void ops.cancelRun()}>
            Cancel operation
          </Button>
        )}
      </header>
      <pre ref={box} className="min-h-0 flex-1 overflow-auto p-3 font-mono text-xs leading-5" role="log" aria-live="polite">
        {active.lines.map((l, i) => (
          <div key={i} className={l.stream === 'stderr' ? 'text-warning' : l.stream === 'info' ? 'text-muted-foreground' : ''}>
            {l.line}
          </div>
        ))}
      </pre>
    </section>
  )
}
