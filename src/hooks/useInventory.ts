import { useCallback, useEffect, useRef, useState } from 'react'
import { api, inTauri } from '@/lib/api'
import type { ManagerInfo, Pkg, ScanProgress, ScanRecord } from '@/lib/types'

export interface Inventory {
  packages: Pkg[]
  managers: ManagerInfo[]
  lastScan: ScanRecord | null
  progress: ScanProgress | null
  scanning: boolean
  loading: boolean
  error: string | null
  scan: () => Promise<void>
  /** Re-reads the persisted inventory (e.g. after an operation changed it). */
  reload: () => Promise<void>
}

/** Loads the persisted inventory, and runs read-only scans on request. */
export function useInventory(): Inventory {
  const [packages, setPackages] = useState<Pkg[]>([])
  const [managers, setManagers] = useState<ManagerInfo[]>([])
  const [lastScan, setLastScan] = useState<ScanRecord | null>(null)
  const [progress, setProgress] = useState<ScanProgress | null>(null)
  const [scanning, setScanning] = useState(false)
  const [loading, setLoading] = useState(inTauri())
  const [error, setError] = useState<string | null>(
    inTauri() ? null : 'UniManage must run inside the desktop app (npm run tauri dev). The browser preview has no backend.',
  )
  const busy = useRef(false)

  const reload = useCallback(async () => {
    const [p, m, h] = await Promise.all([api.getPackages(), api.getManagers(), api.getScanHistory(1)])
    setPackages(p)
    setManagers(m)
    setLastScan(h[0] ?? null)
  }, [])

  const scan = useCallback(async () => {
    if (busy.current) return
    busy.current = true
    setScanning(true)
    setError(null)
    try {
      await api.startScan(true)
      await reload()
    } catch (e) {
      setError(String(e))
    } finally {
      busy.current = false
      setScanning(false)
      setProgress(null)
    }
  }, [reload])

  useEffect(() => {
    if (!inTauri()) return
    let unlisten: (() => void) | undefined
    let cancelled = false
    let unlistenDone: (() => void) | undefined
    api.onScanProgress(setProgress).then((u) => (cancelled ? u() : (unlisten = u)))
    // Background rescans finish without the UI asking for them.
    api
      .onScanFinished(() => void reload().catch((e) => setError(String(e))))
      .then((u) => (cancelled ? u() : (unlistenDone = u)))
    reload()
      .catch((e) => setError(String(e)))
      .finally(() => setLoading(false))
    return () => {
      cancelled = true
      unlisten?.()
      unlistenDone?.()
    }
  }, [reload])

  return { packages, managers, lastScan, progress, scanning, loading, error, scan, reload }
}
