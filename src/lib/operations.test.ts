import { describe, expect, it } from 'vitest'
import { batchItemStatus } from './operations'
import type { OperationRecord } from './types'

const done = { record: { result: 'success' } as OperationRecord }

describe('batchItemStatus', () => {
  it('shows finished entries by their result', () => {
    expect(batchItemStatus(done, 0, true, 1)).toBe('success')
    expect(batchItemStatus({ record: null }, 0, true, 1)).toBe('failed')
  })
  it('labels the current entry running and later ones queued while the batch runs', () => {
    expect(batchItemStatus(undefined, 1, true, 1)).toBe('running')
    expect(batchItemStatus(undefined, 2, true, 1)).toBe('queued')
  })
  it('labels entries never run as skipped once the batch has ended or stopped', () => {
    expect(batchItemStatus(undefined, 2, false, 1)).toBe('skipped')
  })
})
