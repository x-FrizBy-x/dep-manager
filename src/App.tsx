import {
  Boxes,
  Download,
  FolderTree,
  GitFork,
  History,
  LayoutDashboard,
  Moon,
  PackageSearch,
  Settings,
  Sun,
  type LucideIcon,
} from 'lucide-react'
import { useEffect, useState } from 'react'
import { Button } from '@/components/ui/button'
import { BatchDialog, ConfirmDialog, OperationConsole } from '@/components/OperationUI'
import { useInventory } from '@/hooks/useInventory'
import { useOperations } from '@/hooks/useOperations'
import { cn } from '@/lib/utils'
import { History as HistoryPage } from '@/pages/History'
import { Projects } from '@/pages/Projects'
import { SettingsPage, type ThemePref } from '@/pages/Settings'
import { Backup } from '@/pages/Backup'
import { Dependencies } from '@/pages/Dependencies'
import { Explorer } from '@/pages/Explorer'
import { Installed } from '@/pages/Installed'
import { Overview } from '@/pages/Overview'
import { Updates } from '@/pages/Updates'

type PageId = 'overview' | 'projects' | 'installed' | 'updates' | 'explorer' | 'graph' | 'backup' | 'history' | 'settings'

const NAV: { id: PageId; label: string; icon: LucideIcon; ready: boolean }[] = [
  { id: 'overview', label: 'Overview', icon: LayoutDashboard, ready: true },
  { id: 'installed', label: 'Installed Software', icon: Boxes, ready: true },
  { id: 'updates', label: 'Updates', icon: Download, ready: true },
  { id: 'explorer', label: 'Package Explorer', icon: PackageSearch, ready: true },
  { id: 'graph', label: 'Dependencies', icon: GitFork, ready: true },
  { id: 'projects', label: 'Project Dependencies', icon: FolderTree, ready: true },
  { id: 'backup', label: 'Environment Backup', icon: Boxes, ready: true },
  { id: 'history', label: 'Operation History', icon: History, ready: true },
  { id: 'settings', label: 'Settings', icon: Settings, ready: true },
]

function useTheme() {
  const [pref, setPref] = useState<ThemePref>(() => {
    try {
      const s = localStorage.getItem('theme-pref')
      if (s === 'light' || s === 'dark' || s === 'system') return s
      const old = localStorage.getItem('theme') // written by earlier versions
      if (old === 'light' || old === 'dark') return old
    } catch {
      /* storage unavailable */
    }
    return 'system'
  })
  const [systemDark, setSystemDark] = useState(() => window.matchMedia('(prefers-color-scheme: dark)').matches)
  useEffect(() => {
    const mq = window.matchMedia('(prefers-color-scheme: dark)')
    const on = (e: MediaQueryListEvent) => setSystemDark(e.matches)
    mq.addEventListener('change', on)
    return () => mq.removeEventListener('change', on)
  }, [])
  const dark = pref === 'system' ? systemDark : pref === 'dark'
  useEffect(() => {
    document.documentElement.classList.toggle('dark', dark)
    try {
      localStorage.setItem('theme-pref', pref)
    } catch {
      /* storage unavailable */
    }
  }, [dark, pref])
  return { dark, pref, setPref }
}

export default function App() {
  const [page, setPage] = useState<PageId>('overview')
  const { dark, pref, setPref } = useTheme()
  const inv = useInventory()
  const ops = useOperations(() => void inv.reload())

  return (
    <div className="flex h-full">
      <nav className="flex w-56 shrink-0 flex-col border-r bg-sidebar p-3" aria-label="Main">
        <div className="mb-4 flex items-center gap-2 px-2 pt-1 font-semibold">
          <span className="grid size-6 place-items-center rounded-md bg-primary text-primary-foreground text-xs">U</span>
          UniManage
        </div>
        <ul className="flex-1 space-y-0.5">
          {NAV.map(({ id, label, icon: Icon, ready }) => (
            <li key={id}>
              <button
                disabled={!ready}
                aria-current={page === id ? 'page' : undefined}
                onClick={() => setPage(id)}
                className={cn(
                  'flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-sm transition-colors focus-visible:outline-2',
                  ready ? 'cursor-pointer hover:bg-muted' : 'cursor-not-allowed opacity-50',
                  page === id && 'bg-muted font-medium',
                )}
              >
                <Icon className="size-4" aria-hidden />
                <span className="flex-1 text-left">{label}</span>
                {!ready && <span className="text-[10px] text-muted-foreground uppercase">Soon</span>}
              </button>
            </li>
          ))}
        </ul>
        <Button variant="ghost" size="sm" onClick={() => setPref(dark ? 'light' : 'dark')} aria-label="Toggle theme" className="justify-start">
          {dark ? <Sun className="size-4" /> : <Moon className="size-4" />}
          {dark ? 'Light theme' : 'Dark theme'}
        </Button>
      </nav>

      <main className="flex min-w-0 flex-1 flex-col">
        <div className="min-h-0 flex-1 overflow-auto">
          {(inv.error || ops.error) && (
            <div role="alert" className="m-4 rounded-lg border border-destructive/40 bg-destructive/10 p-3 text-sm">
              {inv.error ?? ops.error}
            </div>
          )}
          {inv.loading ? (
            <div className="p-8 text-sm text-muted-foreground" role="status">Loading…</div>
          ) : page === 'installed' ? (
            <Installed inv={inv} ops={ops} />
          ) : page === 'explorer' ? (
            <Explorer ops={ops} onInstalledChange={ops.batch?.phase ?? ops.active?.record?.id ?? null} />
          ) : page === 'graph' ? (
            <Dependencies inv={inv} />
          ) : page === 'projects' ? (
            <Projects />
          ) : page === 'settings' ? (
            <SettingsPage theme={pref} onTheme={setPref} />
          ) : page === 'backup' ? (
            <Backup inv={inv} ops={ops} />
          ) : page === 'updates' ? (
            <Updates inv={inv} ops={ops} />
          ) : page === 'history' ? (
            <HistoryPage refreshKey={ops.active?.record?.id ?? null} />
          ) : (
            <div className="p-6"><Overview inv={inv} refreshKey={ops.active?.record?.id ?? null} onOpen={(p) => setPage(p)} /></div>
          )}
        </div>
        <OperationConsole ops={ops} />
      </main>
      <ConfirmDialog ops={ops} />
      <BatchDialog ops={ops} />
    </div>
  )
}
