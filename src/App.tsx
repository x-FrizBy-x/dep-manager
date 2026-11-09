import {
  Boxes,
  Download,
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
import { ConfirmDialog, OperationConsole } from '@/components/OperationUI'
import { useInventory } from '@/hooks/useInventory'
import { useOperations } from '@/hooks/useOperations'
import { cn } from '@/lib/utils'
import { History as HistoryPage } from '@/pages/History'
import { Installed } from '@/pages/Installed'
import { Overview } from '@/pages/Overview'

type PageId = 'overview' | 'installed' | 'updates' | 'explorer' | 'graph' | 'backup' | 'history' | 'settings'

const NAV: { id: PageId; label: string; icon: LucideIcon; ready: boolean }[] = [
  { id: 'overview', label: 'Overview', icon: LayoutDashboard, ready: true },
  { id: 'installed', label: 'Installed Software', icon: Boxes, ready: true },
  { id: 'updates', label: 'Updates', icon: Download, ready: false },
  { id: 'explorer', label: 'Package Explorer', icon: PackageSearch, ready: false },
  { id: 'graph', label: 'Dependency Graph', icon: GitFork, ready: false },
  { id: 'backup', label: 'Environment Backup', icon: Boxes, ready: false },
  { id: 'history', label: 'Operation History', icon: History, ready: true },
  { id: 'settings', label: 'Settings', icon: Settings, ready: false },
]

function useTheme() {
  const [dark, setDark] = useState(() => {
    try {
      const s = localStorage.getItem('theme')
      if (s) return s === 'dark'
    } catch {
      /* storage unavailable */
    }
    return window.matchMedia('(prefers-color-scheme: dark)').matches
  })
  useEffect(() => {
    document.documentElement.classList.toggle('dark', dark)
    try {
      localStorage.setItem('theme', dark ? 'dark' : 'light')
    } catch {
      /* storage unavailable */
    }
  }, [dark])
  return [dark, () => setDark((d) => !d)] as const
}

export default function App() {
  const [page, setPage] = useState<PageId>('overview')
  const [dark, toggleTheme] = useTheme()
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
        <Button variant="ghost" size="sm" onClick={toggleTheme} aria-label="Toggle theme" className="justify-start">
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
          ) : page === 'history' ? (
            <HistoryPage refreshKey={ops.active?.record?.id ?? null} />
          ) : (
            <div className="p-6"><Overview inv={inv} /></div>
          )}
        </div>
        <OperationConsole ops={ops} />
      </main>
      <ConfirmDialog ops={ops} />
    </div>
  )
}
