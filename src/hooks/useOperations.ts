import { useCallback, useEffect, useRef, useState } from 'react'
import { api, inTauri } from '@/lib/api'
import type { OperationLogLine, OperationPlan, OperationRecord, OpKind } from '@/lib/types'

/** Managers whose write operations are implemented in the backend. */
export const OPERABLE_MANAGERS = new Set(['npm'])

export interface ActiveOp {
  plan: OperationPlan
  operationId: number | null
  lines: OperationLogLine[]
  /** Set once the operation has finished (success, failure, cancel...). */
  record: OperationRecord | null
}

export interface Operations {
  /** A previewed plan waiting for the user's explicit confirmation. */
  plan: OperationPlan | null
  active: ActiveOp | null
  /** True while a preview request or an operation is in flight. */
  busy: boolean
  error: string | null
  preview: (managerId: string, kind: OpKind, pkg: string, version?: string) => Promise<void>
  /** Runs the currently previewed plan. Only called from the confirm button. */
  confirm: () => Promise<void>
  discardPlan: () => void
  cancelRun: () => Promise<void>
  dismiss: () => void
}

export function useOperations(onFinished: () => void): Operations {
  const [plan, setPlan] = useState<OperationPlan | null>(null)
  const [active, setActive] = useState<ActiveOp | null>(null)
  const [previewing, setPreviewing] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const finished = useRef(onFinished)
  useEffect(() => {
    finished.current = onFinished
  })

  useEffect(() => {
    if (!inTauri()) return
    const unlisten: (() => void)[] = []
    let cancelled = false
    const keep = (p: Promise<() => void>) => p.then((u) => (cancelled ? u() : unlisten.push(u)))
    keep(
      api.onOperationStarted((id) => setActive((a) => (a && !a.record ? { ...a, operationId: id } : a))),
    )
    keep(
      api.onOperationLog((e) =>
        setActive((a) =>
          a && !a.record ? { ...a, operationId: a.operationId ?? e.operationId, lines: [...a.lines, e] } : a,
        ),
      ),
    )
    return () => {
      cancelled = true
      unlisten.forEach((u) => u())
    }
  }, [])

  const preview = useCallback(async (managerId: string, kind: OpKind, pkg: string, version?: string) => {
    setError(null)
    setPreviewing(true)
    try {
      setPlan(await api.previewOperation(managerId, kind, pkg, version))
    } catch (e) {
      setError(String(e))
    } finally {
      setPreviewing(false)
    }
  }, [])

  const confirm = useCallback(async () => {
    if (!plan) return
    const p = plan
    setPlan(null)
    setError(null)
    setActive({ plan: p, operationId: null, lines: [], record: null })
    try {
      const record = await api.runOperation(p.id)
      setActive((a) => (a ? { ...a, operationId: record.id, record } : a))
    } catch (e) {
      setActive(null)
      setError(String(e))
    } finally {
      finished.current()
    }
  }, [plan])

  const cancelRun = useCallback(async () => {
    try {
      await api.cancelOperation()
    } catch (e) {
      setError(String(e))
    }
  }, [])

  return {
    plan,
    active,
    busy: previewing || (active !== null && active.record === null),
    error,
    preview,
    confirm,
    discardPlan: () => setPlan(null),
    cancelRun,
    dismiss: () => setActive(null),
  }
}
