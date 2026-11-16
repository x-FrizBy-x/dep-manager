import type { OperationRecord } from './types'

export function resultTone(r: OperationRecord['result']) {
  if (r === 'success') return 'success' as const
  if (r === 'running') return 'primary' as const
  if (r === 'cancelled') return 'warning' as const
  return 'danger' as const
}

/**
 * Label for one entry of a batch. Entries that have not been reached yet are `queued` while the
 * batch is still running; `skipped` is only for entries that were never run because the batch
 * stopped or ended early.
 */
export function batchItemStatus(
  result: { record: OperationRecord | null } | undefined,
  index: number,
  running: boolean,
  current: number,
) {
  if (result) return result.record ? result.record.result : ('failed' as const)
  if (!running) return 'skipped' as const
  return index === current ? ('running' as const) : ('queued' as const)
}
