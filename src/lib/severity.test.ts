import { describe, expect, it } from 'vitest'
import { updateSeverity } from './severity'

describe('updateSeverity', () => {
  it('classifies major / minor / patch', () => {
    expect(updateSeverity('1.2.3', '2.0.0')).toBe('major')
    expect(updateSeverity('1.2.3', '1.3.0')).toBe('minor')
    expect(updateSeverity('1.2.3', '1.2.4')).toBe('patch')
    expect(updateSeverity('0.24.0', '0.27.1')).toBe('minor')
  })
  it('does not guess for unparseable, equal or downgrade versions', () => {
    expect(updateSeverity(null, '1.0.0')).toBe('unknown')
    expect(updateSeverity('2024-01', '1.0.0')).toBe('unknown')
    expect(updateSeverity('1.0.0', '1.0.0')).toBe('unknown')
    expect(updateSeverity('2.0.0', '1.0.0')).toBe('unknown')
  })
  it('tolerates a leading v and two-part versions', () => {
    expect(updateSeverity('v1.2', '1.3')).toBe('minor')
  })
})
