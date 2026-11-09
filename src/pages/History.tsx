import { useEffect, useState } from 'react'
import { Badge } from '@/components/ui/badge'
import { api } from '@/lib/api'
import { resultTone } from '@/lib/operations'
import type { OperationLogLine, OperationRecord } from '@/lib/types'

const fmt = (t: number | null) => (t ? new Date(t * 1000).toLocaleString() : '—')

/** `refreshKey` changes whenever an operation finishes so the list reloads. */
export function History({ refreshKey }: { refreshKey: unknown }) {
  const [ops, setOps] = useState<OperationRecord[] | null>(null)
  const [selected, setSelected] = useState<number | null>(null)
  const [logs, setLogs] = useState<OperationLogLine[]>([])
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let off = false
    api
      .getOperations(200)
      .then((o) => !off && setOps(o))
      .catch((e) => !off && setError(String(e)))
    return () => {
      off = true
    }
  }, [refreshKey])

  useEffect(() => {
    if (selected === null) return
    let off = false
    api
      .getOperationLogs(selected)
      .then((l) => !off && setLogs(l))
      .catch((e) => !off && setError(String(e)))
    return () => {
      off = true
    }
  }, [selected])

  return (
    <div className="flex h-full min-h-0 flex-col p-6">
      <h1 className="text-xl font-semibold">Operation History</h1>
      <p className="mb-4 text-sm text-muted-foreground">Every install, update and removal UniManage has run, with its full output.</p>
      {error && <div role="alert" className="mb-3 rounded-md border border-destructive/40 bg-destructive/10 p-2 text-sm">{error}</div>}
      {ops && ops.length === 0 && <div className="p-8 text-center text-sm text-muted-foreground">No operations yet.</div>}
      <div className="min-h-0 flex-1 overflow-auto">
        {ops && ops.length > 0 && (
          <table className="w-full text-sm">
            <thead className="sticky top-0 bg-background text-xs text-muted-foreground">
              <tr className="border-b text-left">
                <th className="px-3 py-2 font-medium">When</th>
                <th className="px-3 py-2 font-medium">Action</th>
                <th className="px-3 py-2 font-medium">Package</th>
                <th className="px-3 py-2 font-medium">Manager</th>
                <th className="px-3 py-2 font-medium">Version</th>
                <th className="px-3 py-2 font-medium">Result</th>
              </tr>
            </thead>
            <tbody>
              {ops.map((o) => (
                <tr
                  key={o.id}
                  tabIndex={0}
                  aria-selected={o.id === selected}
                  onClick={() => setSelected(o.id)}
                  onKeyDown={(e) => (e.key === 'Enter' || e.key === ' ') && (e.preventDefault(), setSelected(o.id))}
                  className={`cursor-pointer border-b hover:bg-muted/60 focus-visible:outline-2 ${o.id === selected ? 'bg-muted' : ''}`}
                >
                  <td className="px-3 py-2 text-xs">{fmt(o.startedAt)}</td>
                  <td className="px-3 py-2">{o.operationType}</td>
                  <td className="px-3 py-2 font-medium">{o.packageName}</td>
                  <td className="px-3 py-2"><Badge>{o.managerId}</Badge></td>
                  <td className="px-3 py-2 font-mono text-xs">{o.previousVersion ?? '—'} → {o.newVersion ?? '—'}</td>
                  <td className="px-3 py-2"><Badge tone={resultTone(o.result)}>{o.result}</Badge></td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
      {selected !== null && (
        <pre className="mt-3 h-48 shrink-0 overflow-auto rounded-md border bg-card p-3 font-mono text-xs leading-5" aria-label="Operation output">
          {logs.map((l, i) => (
            <div key={i} className={l.stream === 'stderr' ? 'text-warning' : l.stream === 'info' ? 'text-muted-foreground' : ''}>{l.line}</div>
          ))}
        </pre>
      )}
    </div>
  )
}
