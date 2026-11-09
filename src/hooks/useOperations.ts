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

export interface BatchResult {
  plan: OperationPlan
  /** Finished operation, or `null` if it could not be started / was skipped. */
  record: OperationRecord | null
  error: string | null
}

export interface Batch {
  /** `review`: previewed, awaiting confirmation. `running`: executing one by one. `done`: finished. */
  phase: 'review' | 'running' | 'done'
  plans: OperationPlan[]
  results: BatchResult[]
  /** Index of the plan currently running. */
  current: number
  /** The user cancelled; remaining updates were skipped. */
  stopped: boolean
}

export interface Operations {
  /** Multi-package update in progress / awaiting review (Update Center). */
  batch: Batch | null
  previewBatch: (items: { managerId: string; pkg: string }[]) => Promise<void>
  /** Runs every previewed update sequentially. Only called from the confirm button. */
  confirmBatch: () => Promise<void>
  discardBatch: () => void
  /** A previewed plan waiting for the user's explicit confirmation. */
  plan: OperationPlan | null
  active: ActiveOp | null
  /** True while a preview request or an operation is in flight. */
  busy: boolean
  /** Cancel was requested; waiting for the process to stop and the inventory to refresh. */
  cancelling: boolean
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
  const [cancelling, setCancelling] = useState(false)
  const [batch, setBatch] = useState<Batch | null>(null)
  const stopBatch = useRef(false)
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
    setCancelling(false)
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

  const previewBatch = useCallback(async (items: { managerId: string; pkg: string }[]) => {
    setError(null)
    setPreviewing(true)
    try {
      const plans = await Promise.all(items.map((i) => api.previewOperation(i.managerId, 'update', i.pkg)))
      setBatch({ phase: 'review', plans, results: [], current: 0, stopped: false })
    } catch (e) {
      setError(String(e))
    } finally {
      setPreviewing(false)
    }
  }, [])

  const confirmBatch = useCallback(async () => {
    if (!batch || batch.phase !== 'review') return
    const plans = batch.plans
    stopBatch.current = false
    setError(null)
    setBatch({ ...batch, phase: 'running', results: [], current: 0 })
    const results: BatchResult[] = []
    let stopped = false
    for (let i = 0; i < plans.length; i++) {
      if (stopBatch.current) {
        stopped = true
        break
      }
      const plan = plans[i]
      setBatch((b) => (b ? { ...b, current: i } : b))
      setCancelling(false)
      setActive({ plan, operationId: null, lines: [], record: null })
      try {
        const record = await api.runOperation(plan.id)
        setActive((a) => (a ? { ...a, operationId: record.id, record } : a))
        results.push({ plan, record, error: null })
        if (record.result === 'cancelled') {
          stopped = true
          break
        }
      } catch (e) {
        results.push({ plan, record: null, error: String(e) })
      }
      setBatch((b) => (b ? { ...b, results: [...results] } : b))
    }
    setBatch((b) => (b ? { ...b, phase: 'done', results, stopped } : b))
    finished.current()
  }, [batch])

  const cancelRun = useCallback(async () => {
    stopBatch.current = true
    setCancelling(true)
    try {
      await api.cancelOperation()
    } catch (e) {
      setError(String(e))
    }
  }, [])

  return {
    plan,
    active,
    cancelling,
    batch,
    previewBatch,
    confirmBatch,
    discardBatch: () => setBatch(null),
    busy: previewing || batch?.phase === 'running' || (active !== null && active.record === null),
    error,
    preview,
    confirm,
    discardPlan: () => setPlan(null),
    cancelRun,
    dismiss: () => setActive(null),
  }
}
