//! Tauri commands. All are read-only in this milestone: none of them can
//! install, update or remove software.

use crate::db::{OperationLogLine, OperationRecord, ScanRecord};
use crate::error::{AppError, Result};
use crate::exec::SystemRunner;
use crate::model::{ManagerInfo, Package, PackageDetails, SearchManager, SearchResult};
use crate::ops::{self, OpKind, OperationPlan};
use crate::{adapters, scanner, AppState};
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Emitter, State};

fn with_db<T>(state: &AppState, f: impl FnOnce(&crate::db::Db) -> Result<T>) -> Result<T> {
    let db = state.db.lock().map_err(|_| AppError::Db("database lock poisoned".into()))?;
    f(&db)
}

/// Clears a busy flag when dropped, including during unwinding.
struct FlagReset(std::sync::Arc<std::sync::atomic::AtomicBool>);
impl Drop for FlagReset {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

#[tauri::command]
pub async fn start_scan(app: AppHandle, state: State<'_, AppState>, check_updates: Option<bool>) -> Result<ScanRecord> {
    if state.scanning.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() {
        return Err(AppError::Other("a scan is already running".into()));
    }
    if state.op_running.load(Ordering::Acquire) {
        state.scanning.store(false, Ordering::Release);
        return Err(AppError::Other("an operation is running; scan after it finishes".into()));
    }
    let (db, flag) = (state.db.clone(), state.scanning.clone());
    let res = tauri::async_runtime::spawn_blocking(move || {
        let adapters = adapters::all();
        let runner = SystemRunner::default();
        // Resets the flag even if scanning panics.
        let _reset = FlagReset(flag);
        scanner::scan_all(&adapters, &runner, &db, check_updates.unwrap_or(true), &|p| {
            let _ = app.emit("scan-progress", p);
        })
    })
    .await
    .map_err(|e| AppError::Other(e.to_string()))?;
    res
}

#[tauri::command]
pub fn get_packages(state: State<'_, AppState>) -> Result<Vec<Package>> {
    with_db(&state, |d| d.list_packages())
}

#[tauri::command]
pub fn get_managers(state: State<'_, AppState>) -> Result<Vec<ManagerInfo>> {
    with_db(&state, |d| d.list_managers())
}

#[tauri::command]
pub fn get_scan_history(state: State<'_, AppState>, limit: Option<u32>) -> Result<Vec<ScanRecord>> {
    with_db(&state, |d| d.recent_scans(limit.unwrap_or(20)))
}

// ---- operation pipeline ---------------------------------------------------

const PLAN_TTL_SECS: i64 = 600;

#[tauri::command]
pub fn preview_operation(
    state: State<'_, AppState>,
    manager_id: String,
    kind: OpKind,
    package: String,
    version: Option<String>,
) -> Result<OperationPlan> {
    // The previous version comes from our own inventory, not from the caller.
    let previous = with_db(&state, |d| d.list_packages())?
        .into_iter()
        .find(|p| p.package_manager == manager_id && p.name == package)
        .and_then(|p| p.version);
    let now = crate::db::now();
    let n = state.plan_counter.fetch_add(1, Ordering::Relaxed);
    let id = format!("plan-{now}-{n}");
    let plan = ops::plan_operation(id.clone(), now, &manager_id, kind, &package, version.as_deref(), previous.as_deref())?;
    let mut plans = state.plans.lock().map_err(|_| AppError::Other("plan lock poisoned".into()))?;
    plans.retain(|_, p| now - p.created_at < PLAN_TTL_SECS);
    plans.insert(id, plan.clone());
    Ok(plan)
}

#[tauri::command]
pub fn cancel_operation(state: State<'_, AppState>) -> Result<()> {
    if state.op_running.load(Ordering::Acquire) {
        state.cancel.store(true, Ordering::Release);
    }
    Ok(())
}

/// Executes a previously previewed plan. Calling this *is* the user's explicit
/// confirmation; the frontend must only call it after showing the preview.
#[tauri::command]
pub async fn run_operation(app: AppHandle, state: State<'_, AppState>, plan_id: String) -> Result<OperationRecord> {
    let plan = state
        .plans
        .lock()
        .map_err(|_| AppError::Other("plan lock poisoned".into()))?
        .remove(&plan_id)
        .ok_or_else(|| AppError::Other("unknown or already used plan; preview again".into()))?;
    if crate::db::now() - plan.created_at >= PLAN_TTL_SECS {
        return Err(AppError::Other("plan expired; preview again".into()));
    }
    // Global lock: one write operation at a time, never during a scan.
    if state.op_running.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() {
        return Err(AppError::Other("another operation is already running".into()));
    }
    if state.scanning.load(Ordering::Acquire) {
        state.op_running.store(false, Ordering::Release);
        return Err(AppError::Other("a scan is running; try again when it finishes".into()));
    }
    state.cancel.store(false, Ordering::Release);

    let (db, running, cancel) = (state.db.clone(), state.op_running.clone(), state.cancel.clone());
    let res = tauri::async_runtime::spawn_blocking(move || {
        let _reset = FlagReset(running);
        let (a1, a2) = (app.clone(), app.clone());
        let out = ops::execute(
            &plan,
            &db,
            &cancel,
            ops::OP_TIMEOUT,
            &move |id| {
                let _ = a1.emit("operation-started", serde_json::json!({ "operationId": id }));
            },
            &move |id, stream, line| {
                let _ = a2.emit(
                    "operation-log",
                    serde_json::json!({ "operationId": id, "stream": stream, "line": line, "ts": crate::db::now() }),
                );
            },
            &|| rescan_manager(&db, &plan),
        );
        drop(_reset);
        if let Ok(rec) = &out {
            let _ = app.emit("operation-finished", rec);
        }
        out
    })
    .await
    .map_err(|e| AppError::Other(e.to_string()))?;
    res
}

/// Refreshes one manager's inventory after an operation; returns the
/// package's installed version afterwards (None if absent/unknown).
fn rescan_manager(db: &std::sync::Mutex<crate::db::Db>, plan: &OperationPlan) -> Option<String> {
    let adapters = adapters::all();
    let a = adapters.iter().find(|a| a.id() == plan.manager_id)?;
    let res = scanner::scan_adapter(a.as_ref(), &SystemRunner::default(), true);
    let version = res
        .packages
        .as_ref()
        .and_then(|ps| ps.iter().find(|p| p.name == plan.package))
        .and_then(|p| p.version.clone());
    if let Ok(d) = db.lock() {
        let _ = d.save_manager(&res.info, res.packages.as_deref());
    }
    version
}

#[tauri::command]
pub fn get_operations(state: State<'_, AppState>, limit: Option<u32>) -> Result<Vec<OperationRecord>> {
    with_db(&state, |d| d.list_operations(limit.unwrap_or(100)))
}

#[tauri::command]
pub fn get_operation_logs(state: State<'_, AppState>, operation_id: i64) -> Result<Vec<OperationLogLine>> {
    with_db(&state, |d| d.operation_logs(operation_id))
}

// ---- package explorer -----------------------------------------------------

/// Managers that can be searched and are present on this machine.
#[tauri::command]
pub fn get_search_managers() -> Vec<SearchManager> {
    let runner = SystemRunner::default();
    adapters::all()
        .iter()
        .filter(|a| a.supports_search() && a.detect(&runner).is_some())
        .map(|a| SearchManager {
            id: a.id().into(),
            display_name: a.display_name().into(),
            can_install: ops::supports_operations(a.id()),
        })
        .collect()
}

fn adapter_for(manager_id: &str) -> Result<Box<dyn adapters::Adapter>> {
    adapters::all()
        .into_iter()
        .find(|a| a.id() == manager_id && a.supports_search())
        .ok_or_else(|| AppError::Unsupported(format!("{manager_id}: search is not implemented")))
}

#[tauri::command]
pub async fn search_packages(
    state: State<'_, AppState>,
    manager_id: String,
    query: String,
) -> Result<Vec<SearchResult>> {
    let installed = with_db(&state, |d| d.list_packages())?;
    tauri::async_runtime::spawn_blocking(move || {
        let adapter = adapter_for(&manager_id)?;
        let hits = adapter.search(&SystemRunner::default(), &query, 25)?;
        let can_install = ops::supports_operations(&manager_id);
        Ok(hits
            .into_iter()
            .map(|h| {
                let local = installed.iter().find(|p| p.package_manager == manager_id && p.name == h.name);
                SearchResult {
                    manager_id: manager_id.clone(),
                    installed: local.is_some(),
                    installed_version: local.and_then(|p| p.version.clone()),
                    name: h.name,
                    version: h.version,
                    description: h.description,
                    can_install,
                }
            })
            .collect())
    })
    .await
    .map_err(|e| AppError::Other(e.to_string()))?
}

#[tauri::command]
pub async fn get_package_details(manager_id: String, name: String) -> Result<PackageDetails> {
    tauri::async_runtime::spawn_blocking(move || {
        adapter_for(&manager_id)?.details(&SystemRunner::default(), &name)
    })
    .await
    .map_err(|e| AppError::Other(e.to_string()))?
}
