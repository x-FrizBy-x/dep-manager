// Mirrors src-tauri/src/model.rs. `null` always means "unknown", never a guess.

export type ManagementStatus = 'managed' | 'external' | 'unknown'
export type UpdateStatus = 'unknown' | 'up_to_date' | 'available'
export type PackageKind = 'application' | 'runtime' | 'library' | 'system' | 'unknown'

export interface Pkg {
  id: string
  name: string
  displayName: string | null
  version: string | null
  latestVersion: string | null
  description: string | null
  packageManager: string
  category: string | null
  kind: PackageKind
  installationSource: string | null
  installationPath: string | null
  architecture: string | null
  installedDate: string | null
  updateAvailable: boolean | null
  updateStatus: UpdateStatus
  managementStatus: ManagementStatus
  homepage: string | null
  license: string | null
  dependencies: string[]
  metadata: Record<string, string>
  alsoFoundIn: string[]
}

export interface ManagerInfo {
  id: string
  displayName: string
  detected: boolean
  path: string | null
  version: string | null
  supportsUpdates: boolean
  packageCount: number
  lastError: string | null
}

export interface ManagerOutcome {
  id: string
  detected: boolean
  ok: boolean
  packageCount: number
  updateCount: number
  error: string | null
  updateError: string | null
}

export interface ScanRecord {
  id: number
  startedAt: number
  finishedAt: number | null
  status: 'running' | 'completed' | 'partial' | 'failed'
  packageCount: number
  updateCount: number
  details: ManagerOutcome[] | null
}

export interface ScanProgress {
  phase: 'started' | 'manager' | 'finished'
  manager: string | null
  done: number
  total: number
  message: string
}

export type OpKind = 'install' | 'update' | 'uninstall'

/** Result of `preview_operation`. Nothing has been executed yet. */
export interface OperationPlan {
  id: string
  managerId: string
  kind: OpKind
  package: string
  version: string | null
  previousVersion: string | null
  program: string
  args: string[]
  command: string
  requiresElevation: boolean
  warnings: string[]
}

export interface OperationRecord {
  id: number
  startedAt: number
  finishedAt: number | null
  operationType: OpKind
  packageName: string
  managerId: string
  previousVersion: string | null
  newVersion: string | null
  command: string | null
  result: 'running' | 'success' | 'failed' | 'cancelled' | 'timeout' | 'interrupted'
  error: string | null
}

export interface OperationLogLine {
  ts: number
  stream: 'stdout' | 'stderr' | 'info'
  line: string
}

export interface OperationLogEvent extends OperationLogLine {
  operationId: number
}
