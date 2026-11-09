import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import type {
  ManagerInfo,
  OperationLogEvent,
  OperationLogLine,
  OperationPlan,
  OperationRecord,
  OpKind,
  Pkg,
  ScanProgress,
  ScanRecord,
} from './types'

/** True when running inside the Tauri webview (not a plain browser/test). */
export const inTauri = () => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window

export const api = {
  getPackages: () => invoke<Pkg[]>('get_packages'),
  getManagers: () => invoke<ManagerInfo[]>('get_managers'),
  getScanHistory: (limit = 20) => invoke<ScanRecord[]>('get_scan_history', { limit }),
  startScan: (checkUpdates = true) => invoke<ScanRecord>('start_scan', { checkUpdates }),
  /** Step 1: build a preview. Executes nothing. */
  previewOperation: (managerId: string, kind: OpKind, pkg: string, version?: string) =>
    invoke<OperationPlan>('preview_operation', { managerId, kind, package: pkg, version: version ?? null }),
  /** Step 2: run a previewed plan. Only call after the user explicitly confirmed the preview. */
  runOperation: (planId: string) => invoke<OperationRecord>('run_operation', { planId }),
  cancelOperation: () => invoke<void>('cancel_operation'),
  getOperations: (limit = 100) => invoke<OperationRecord[]>('get_operations', { limit }),
  getOperationLogs: (operationId: number) => invoke<OperationLogLine[]>('get_operation_logs', { operationId }),
  onOperationLog: (cb: (e: OperationLogEvent) => void) => listen<OperationLogEvent>('operation-log', (e) => cb(e.payload)),
  onOperationStarted: (cb: (id: number) => void) =>
    listen<{ operationId: number }>('operation-started', (e) => cb(e.payload.operationId)),
  onOperationFinished: (cb: (r: OperationRecord) => void) => listen<OperationRecord>('operation-finished', (e) => cb(e.payload)),
  onScanProgress: (cb: (p: ScanProgress) => void) => listen<ScanProgress>('scan-progress', (e) => cb(e.payload)),
}
