import type { OperationRecord } from './types'

export function resultTone(r: OperationRecord['result']) {
  if (r === 'success') return 'success' as const
  if (r === 'running') return 'primary' as const
  if (r === 'cancelled') return 'warning' as const
  return 'danger' as const
}
