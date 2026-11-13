import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import type {
  ManagerInfo,
  ManifestDiff,
  ManifestSummary,
  OperationLogEvent,
  OperationLogLine,
  OperationPlan,
  OperationRecord,
  OpKind,
  DependencyManager,
  PackageDependencies,
  PackageDetails,
  Pkg,
  ScanProgress,
  ScanRecord,
  SearchManager,
  SearchResult,
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
  getSearchManagers: () => invoke<SearchManager[]>('get_search_managers'),
  searchPackages: (managerId: string, query: string) => invoke<SearchResult[]>('search_packages', { managerId, query }),
  getPackageDetails: (managerId: string, name: string) =>
    invoke<PackageDetails>('get_package_details', { managerId, name }),
  getDependencyManagers: () => invoke<DependencyManager[]>('get_dependency_managers'),
  getPackageDependencies: (managerId: string, name: string) =>
    invoke<PackageDependencies>('get_package_dependencies', { managerId, name }),
  createManifest: (name: string) => invoke<ManifestSummary>('create_manifest', { name }),
  listManifests: () => invoke<ManifestSummary[]>('list_manifests'),
  exportManifest: (id: number, format: 'json' | 'yaml') => invoke<string>('export_manifest', { id, format }),
  importManifest: (content: string) => invoke<ManifestSummary>('import_manifest', { content }),
  deleteManifest: (id: number) => invoke<void>('delete_manifest', { id }),
  diffManifest: (id: number) => invoke<ManifestDiff>('diff_manifest', { id }),
  onScanProgress: (cb: (p: ScanProgress) => void) => listen<ScanProgress>('scan-progress', (e) => cb(e.payload)),
}
