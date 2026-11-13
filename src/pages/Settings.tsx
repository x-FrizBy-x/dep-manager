import { useEffect, useState } from 'react'
import { api } from '@/lib/api'
import type { Settings } from '@/lib/types'

export type ThemePref = 'system' | 'light' | 'dark'

const INTERVALS: [number, string][] = [
  [15, 'Every 15 minutes'],
  [30, 'Every 30 minutes'],
  [60, 'Every hour'],
  [180, 'Every 3 hours'],
  [720, 'Every 12 hours'],
  [1440, 'Every day'],
]

const selectCls = 'h-9 rounded-md border bg-background px-2 text-sm focus-visible:outline-2'

export function SettingsPage({ theme, onTheme }: { theme: ThemePref; onTheme: (t: ThemePref) => void }) {
  const [s, setS] = useState<Settings | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [saved, setSaved] = useState(false)

  useEffect(() => {
    api
      .getSettings()
      .then(setS)
      .catch((e) => setError(String(e)))
  }, [])

  const update = async (patch: Partial<Settings>) => {
    if (!s) return
    const next = { ...s, ...patch }
    setS(next)
    setError(null)
    setSaved(false)
    try {
      setS(await api.saveSettings(next))
      setSaved(true)
    } catch (e) {
      setError(String(e))
      setS(s)
    }
  }

  return (
    <div className="max-w-2xl space-y-6 p-6">
      <header>
        <h1 className="text-xl font-semibold">Settings</h1>
        <p className="text-sm text-muted-foreground">Saved on this machine. Nothing here changes installed software.</p>
      </header>

      {error && (
        <div role="alert" className="rounded-lg border border-destructive/40 bg-destructive/10 p-3 text-sm">
          {error}
        </div>
      )}

      <section aria-labelledby="appearance" className="space-y-2 rounded-lg border bg-card p-4">
        <h2 id="appearance" className="font-medium">
          Appearance
        </h2>
        <label className="flex items-center justify-between gap-4 text-sm">
          Theme
          <select aria-label="Theme" className={selectCls} value={theme} onChange={(e) => onTheme(e.target.value as ThemePref)}>
            <option value="system">Follow system</option>
            <option value="light">Light</option>
            <option value="dark">Dark</option>
          </select>
        </label>
      </section>

      <section aria-labelledby="rescan" className="space-y-3 rounded-lg border bg-card p-4">
        <h2 id="rescan" className="font-medium">
          Background rescan
        </h2>
        <p className="text-xs text-muted-foreground">
          While UniManage is open, rescans the inventory on a schedule. Scans only read from package managers, never run while an
          operation or another scan is running, and are off unless you turn them on.
        </p>
        {s === null ? (
          <p className="text-sm text-muted-foreground" role="status">
            Loading…
          </p>
        ) : (
          <>
            <label className="flex items-center justify-between gap-4 text-sm">
              Rescan automatically
              <input type="checkbox" checked={s.rescanEnabled} onChange={(e) => void update({ rescanEnabled: e.target.checked })} />
            </label>
            <label className="flex items-center justify-between gap-4 text-sm">
              Interval
              <select
                aria-label="Rescan interval"
                className={selectCls}
                disabled={!s.rescanEnabled}
                value={s.rescanIntervalMinutes}
                onChange={(e) => void update({ rescanIntervalMinutes: Number(e.target.value) })}
              >
                {INTERVALS.map(([m, label]) => (
                  <option key={m} value={m}>
                    {label}
                  </option>
                ))}
              </select>
            </label>
            <label className="flex items-center justify-between gap-4 text-sm">
              Also check for updates
              <input
                type="checkbox"
                disabled={!s.rescanEnabled}
                checked={s.rescanCheckUpdates}
                onChange={(e) => void update({ rescanCheckUpdates: e.target.checked })}
              />
            </label>
            <p className="text-xs text-muted-foreground" role="status">
              {saved ? 'Saved.' : 'The schedule is checked every 30 seconds, so a rescan can start up to 30 seconds late.'}
            </p>
          </>
        )}
      </section>
    </div>
  )
}
