export type Severity = 'major' | 'minor' | 'patch' | 'unknown'

function parse(v: string | null): number[] | null {
  if (!v) return null
  const m = /^v?(\d+)\.(\d+)(?:\.(\d+))?/.exec(v.trim())
  return m ? [Number(m[1]), Number(m[2]), Number(m[3] ?? 0)] : null
}

/**
 * Size of an update from the version numbers alone. Only dotted numeric
 * versions are classified; anything else is `unknown` (nothing is guessed,
 * and no changelog is implied).
 */
export function updateSeverity(current: string | null, latest: string | null): Severity {
  const a = parse(current)
  const b = parse(latest)
  if (!a || !b) return 'unknown'
  if (b[0] !== a[0]) return b[0] > a[0] ? 'major' : 'unknown'
  if (b[1] !== a[1]) return b[1] > a[1] ? 'minor' : 'unknown'
  if (b[2] !== a[2]) return b[2] > a[2] ? 'patch' : 'unknown'
  return 'unknown'
}
