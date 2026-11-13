//! Tauri commands. All are read-only in this milestone: none of them can
//! install, update or remove software.

use crate::db::{OperationLogLine, OperationRecord, ScanRecord};
use crate::error::{AppError, Result};
use crate::exec::SystemRunner;
use crate::model::{DependencyManager, ManagerInfo, Package, PackageDependencies, PackageDetails, SearchManager, SearchResult};
use crate::ops::{self, OpKind, OperationPlan};
use crate::{adapters, scanner, AppState};
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Emitter, Manager, State};

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

/// Runs one scan on the calling (blocking) thread. Shared by the manual and the
/// background scan. Refuses to overlap another scan or a running operation.
fn perform_scan(app: &AppHandle, state: &AppState, check_updates: bool) -> Result<ScanRecord> {
    if state.scanning.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() {
        return Err(AppError::Other("a scan is already running".into()));
    }
    // Resets the flag even if scanning panics or returns early.
    let _reset = FlagReset(state.scanning.clone());
    if state.op_running.load(Ordering::Acquire) {
        return Err(AppError::Other("an operation is running; scan after it finishes".into()));
    }
    let adapters = adapters::all();
    let runner = SystemRunner::default();
    let rec = scanner::scan_all(&adapters, &runner, &state.db, check_updates, &|p| {
        let _ = app.emit("scan-progress", p);
    })?;
    let _ = app.emit("scan-finished", &rec);
    Ok(rec)
}

#[tauri::command]
pub async fn start_scan(app: AppHandle, state: State<'_, AppState>, check_updates: Option<bool>) -> Result<ScanRecord> {
    let _ = &state;
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        perform_scan(&app, &state, check_updates.unwrap_or(true))
    })
    .await
    .map_err(|e| AppError::Other(e.to_string()))?
}

/// Checks every 30 s whether a background rescan is due according to the saved
/// settings (off by default). Scans are read-only and never overlap a manual
/// scan or an operation.
pub fn spawn_background_rescan(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_secs(30));
        let state = app.state::<AppState>();
        let due = state
            .db
            .lock()
            .ok()
            .and_then(|d| Some((d.settings().ok()?, d.last_scan_started().ok()?)))
            .filter(|(s, last)| crate::settings::rescan_due(s, *last, crate::db::now()));
        if let Some((s, _)) = due {
            // Overlap with a manual scan or an operation is simply skipped until the next tick.
            let _ = perform_scan(&app, &state, s.rescan_check_updates);
        }
    });
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Result<crate::settings::Settings> {
    with_db(&state, |d| d.settings())
}

#[tauri::command]
pub fn save_settings(state: State<'_, AppState>, settings: crate::settings::Settings) -> Result<crate::settings::Settings> {
    settings.validate()?;
    with_db(&state, |d| {
        d.save_settings(&settings)?;
        d.settings()
    })
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
pub async fn preview_operation(
    state: State<'_, AppState>,
    manager_id: String,
    kind: OpKind,
    package: String,
    version: Option<String>,
) -> Result<OperationPlan> {
    // Everything about the installed package comes from our own inventory, not from the caller.
    let local = with_db(&state, |d| d.list_packages())?
        .into_iter()
        .find(|p| p.package_manager == manager_id && p.name == package);
    let mut ctx = ops::PlanContext {
        previous_version: local.as_ref().and_then(|p| p.version.clone()),
        scope: local.as_ref().and_then(|p| p.metadata.get("installation").cloned()),
        remote: None,
    };
    if manager_id == "flatpak" && kind == OpKind::Install {
        // Find which remote carries the app (also proves it exists) before building the command.
        let pkg = package.clone();
        ctx.remote = tauri::async_runtime::spawn_blocking(move || {
            adapters::all()
                .into_iter()
                .find(|a| a.id() == "flatpak")
                .and_then(|a| a.details(&SystemRunner::default(), &pkg).ok())
                .and_then(|d| d.metadata.get("remote").cloned())
        })
        .await
        .map_err(|e| AppError::Other(e.to_string()))?;
    }
    let now = crate::db::now();
    let n = state.plan_counter.fetch_add(1, Ordering::Relaxed);
    let id = format!("plan-{now}-{n}");
    let plan = ops::plan_operation(id.clone(), now, &manager_id, kind, &package, version.as_deref(), &ctx)?;
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

/// Deletes the finished operations and their output from the history. Only the
/// record is removed: nothing is undone on the system. Refused while an
/// operation is running so the live record is never touched.
#[tauri::command]
pub fn clear_operation_history(state: State<'_, AppState>) -> Result<usize> {
    if state.op_running.load(Ordering::Acquire) {
        return Err(AppError::Other("an operation is running; clear the history after it finishes".into()));
    }
    with_db(&state, |d| d.clear_operation_history())
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

// ---- dependency explorer --------------------------------------------------

/// Managers whose dependency data can be read and that are present here.
#[tauri::command]
pub fn get_dependency_managers() -> Vec<DependencyManager> {
    let runner = SystemRunner::default();
    adapters::all()
        .iter()
        .filter(|a| a.supports_dependencies() && a.detect(&runner).is_some())
        .map(|a| DependencyManager { id: a.id().into(), display_name: a.display_name().into() })
        .collect()
}

/// Dependencies of one installed package within one manager. Versions the
/// manager did not report are filled in from the local inventory.
#[tauri::command]
pub async fn get_package_dependencies(
    state: State<'_, AppState>,
    manager_id: String,
    name: String,
) -> Result<PackageDependencies> {
    let installed = with_db(&state, |d| d.list_packages())?;
    tauri::async_runtime::spawn_blocking(move || {
        let adapter = adapters::all()
            .into_iter()
            .find(|a| a.id() == manager_id && a.supports_dependencies())
            .ok_or_else(|| AppError::Unsupported(format!("{manager_id}: dependency data is not implemented")))?;
        let mut deps = adapter.dependencies(&SystemRunner::default(), &name)?;
        let version_of = |n: &str| {
            installed
                .iter()
                .find(|p| p.package_manager == manager_id && p.name == n)
                .and_then(|p| p.version.clone())
        };
        if deps.version.is_none() {
            deps.version = version_of(&deps.name);
        }
        for e in deps.depends_on.iter_mut().chain(&mut deps.transitive).chain(&mut deps.required_by) {
            if e.version.is_none() && e.installed {
                e.version = version_of(&e.name);
            }
        }
        Ok(deps)
    })
    .await
    .map_err(|e| AppError::Other(e.to_string()))?
}

// ---- environment manifests ------------------------------------------------
// Creating, importing, exporting and diffing only read the local inventory and
// the manifest table. Restoring goes through the normal preview/confirm flow.

fn load_manifest(state: &AppState, id: i64) -> Result<(i64, crate::manifest::Manifest)> {
    let (created, content) = with_db(state, |d| d.manifest_row(id))?
        .ok_or_else(|| AppError::Other(format!("manifest {id} not found")))?;
    Ok((created, crate::manifest::parse(&content)?))
}

#[tauri::command]
pub fn create_manifest(state: State<'_, AppState>, name: String) -> Result<crate::manifest::ManifestSummary> {
    let name = name.trim().to_string();
    if name.is_empty() || name.chars().count() > 100 || name.chars().any(char::is_control) {
        return Err(AppError::Other("manifest name must be 1-100 characters".into()));
    }
    with_db(&state, |d| {
        let m = crate::manifest::build(&d.list_packages()?, &d.list_managers()?, &name, std::env::consts::OS, crate::db::now());
        let id = d.insert_manifest(&m.name, &m.platform, &crate::manifest::to_json(&m)?)?;
        let created = d.manifest_row(id)?.map(|r| r.0).unwrap_or(m.created_at);
        Ok(crate::manifest::summary(id, created, &m))
    })
}

#[tauri::command]
pub fn list_manifests(state: State<'_, AppState>) -> Result<Vec<crate::manifest::ManifestSummary>> {
    with_db(&state, |d| {
        Ok(d.list_manifest_rows()?
            .into_iter()
            .filter_map(|(id, created, content)| crate::manifest::parse(&content).ok().map(|m| crate::manifest::summary(id, created, &m)))
            .collect())
    })
}

/// Returns the manifest text for the UI to save as a file. `format`: `json` or `yaml`.
#[tauri::command]
pub fn export_manifest(state: State<'_, AppState>, id: i64, format: String) -> Result<String> {
    let (_, m) = load_manifest(&state, id)?;
    match format.as_str() {
        "json" => crate::manifest::to_json(&m),
        "yaml" => Ok(crate::manifest::to_yaml(&m)),
        other => Err(AppError::Unsupported(format!("export format `{other}`"))),
    }
}

/// Validates and stores a manifest from JSON text. Nothing is installed.
#[tauri::command]
pub fn import_manifest(state: State<'_, AppState>, content: String) -> Result<crate::manifest::ManifestSummary> {
    let m = crate::manifest::parse(&content)?;
    with_db(&state, |d| {
        let id = d.insert_manifest(&m.name, &m.platform, &crate::manifest::to_json(&m)?)?;
        let created = d.manifest_row(id)?.map(|r| r.0).unwrap_or(m.created_at);
        Ok(crate::manifest::summary(id, created, &m))
    })
}

#[tauri::command]
pub fn delete_manifest(state: State<'_, AppState>, id: i64) -> Result<()> {
    with_db(&state, |d| d.delete_manifest(id))
}

/// Diff of a stored manifest against the last scan. Read-only.
#[tauri::command]
pub fn diff_manifest(state: State<'_, AppState>, id: i64) -> Result<crate::manifest::ManifestDiff> {
    let (_, m) = load_manifest(&state, id)?;
    with_db(&state, |d| {
        let detected = d.list_managers()?.into_iter().filter(|x| x.detected).map(|x| x.id).collect();
        Ok(crate::manifest::diff(id, &m, &d.list_packages()?, &detected, ops::supports_operations))
    })
}

// ---- project dependencies -------------------------------------------------
// Read-only: manifests and lockfiles are parsed, no command runs in the project.

const MAX_PROJECTS: usize = 50;

/// Saved project folders that still exist.
#[tauri::command]
pub fn get_projects(state: State<'_, AppState>) -> Result<Vec<String>> {
    with_db(&state, |d| d.projects())
}

/// Scans a folder and remembers it in the project list.
#[tauri::command]
pub async fn scan_project(state: State<'_, AppState>, path: String) -> Result<crate::project::ProjectScan> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let dir = crate::project::resolve_dir(&path)?;
        let scan = crate::project::scan_dir(&dir);
        let d = db.lock().map_err(|_| AppError::Db("database lock poisoned".into()))?;
        let mut list = d.projects()?;
        list.retain(|p| p != &scan.path);
        list.insert(0, scan.path.clone());
        list.truncate(MAX_PROJECTS);
        d.save_projects(&list)?;
        Ok(scan)
    })
    .await
    .map_err(|e| AppError::Other(e.to_string()))?
}

/// Forgets a project folder. Does not touch the folder.
#[tauri::command]
pub fn remove_project(state: State<'_, AppState>, path: String) -> Result<Vec<String>> {
    with_db(&state, |d| {
        let mut list = d.projects()?;
        list.retain(|p| p != &path);
        d.save_projects(&list)?;
        Ok(list)
    })
}
