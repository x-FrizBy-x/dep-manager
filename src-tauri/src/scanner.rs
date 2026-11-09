//! Read-only system scanner. Queries each adapter independently (in parallel),
//! tolerates per-manager failures, and persists results through `Db`.
//! It never walks the filesystem: only package-manager queries are used.

use crate::adapters::Adapter;
use crate::db::{Db, ManagerOutcome, ScanRecord};
use crate::error::{AppError, Result};
use crate::exec::Runner;
use crate::model::*;
use serde::Serialize;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{mpsc, Mutex};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgress {
    /// `started` | `manager` | `finished`
    pub phase: &'static str,
    pub manager: Option<String>,
    pub done: usize,
    pub total: usize,
    pub message: String,
}

pub struct AdapterResult {
    pub info: ManagerInfo,
    pub packages: Option<Vec<Package>>,
    pub outcome: ManagerOutcome,
}

pub fn scan_adapter(a: &dyn Adapter, r: &dyn Runner, check_updates: bool) -> AdapterResult {
    let mut info = ManagerInfo {
        id: a.id().into(),
        display_name: a.display_name().into(),
        detected: false,
        path: None,
        version: None,
        supports_updates: a.supports_updates(),
        package_count: 0,
        last_error: None,
    };
    let mut outcome = ManagerOutcome {
        id: a.id().into(),
        detected: false,
        ok: false,
        package_count: 0,
        update_count: 0,
        error: None,
        update_error: None,
    };

    let Some(path) = a.detect(r) else {
        return AdapterResult { info, packages: None, outcome };
    };
    info.detected = true;
    outcome.detected = true;
    info.path = Some(path.to_string_lossy().into_owned());
    info.version = a.version(r).ok().filter(|v| !v.is_empty());

    let mut packages = match a.list_installed(r) {
        Ok(p) => p,
        Err(e) => {
            info.last_error = Some(e.to_string());
            outcome.error = Some(e.to_string());
            return AdapterResult { info, packages: None, outcome };
        }
    };
    outcome.ok = true;

    if check_updates && a.supports_updates() {
        match a.check_updates(r) {
            Ok(updates) => {
                outcome.update_count = apply_updates(&mut packages, &updates);
            }
            Err(e) => outcome.update_error = Some(e.to_string()),
        }
    }
    info.package_count = packages.len() as u32;
    outcome.package_count = packages.len() as u32;
    AdapterResult { info, packages: Some(packages), outcome }
}

/// A *successful* update check marks everything not listed as up to date.
/// Returns the number of packages with updates.
pub fn apply_updates(packages: &mut [Package], updates: &[UpdateInfo]) -> u32 {
    let mut count = 0;
    for p in packages.iter_mut() {
        let found = updates.iter().find(|u| u.name == p.name && Some(&u.latest_version) != p.version.as_ref());
        match found {
            Some(u) => {
                p.latest_version = Some(u.latest_version.clone());
                p.update_available = Some(true);
                p.update_status = UpdateStatus::Available;
                count += 1;
            }
            None => {
                p.update_available = Some(false);
                p.update_status = UpdateStatus::UpToDate;
            }
        }
    }
    count
}

pub fn scan_all(
    adapters: &[Box<dyn Adapter>],
    runner: &dyn Runner,
    db: &Mutex<Db>,
    check_updates: bool,
    emit: &(dyn Fn(ScanProgress) + Sync),
) -> Result<ScanRecord> {
    let total = adapters.len();
    let scan_id = lock(db)?.begin_scan()?;
    emit(ScanProgress { phase: "started", manager: None, done: 0, total, message: "Scanning package managers".into() });

    let mut outcomes = Vec::new();
    let (mut pkgs, mut upds, mut failures) = (0u32, 0u32, 0usize);

    std::thread::scope(|s| {
        let (tx, rx) = mpsc::channel::<(String, AdapterResult)>();
        for a in adapters {
            let tx = tx.clone();
            let a: &dyn Adapter = a.as_ref();
            s.spawn(move || {
                let res = catch_unwind(AssertUnwindSafe(|| scan_adapter(a, runner, check_updates)))
                    .unwrap_or_else(|_| {
                        let mut r = scan_adapter(&Unavailable, runner, false);
                        r.info.id = a.id().into();
                        r.info.display_name = a.display_name().into();
                        r.info.detected = true;
                        r.info.last_error = Some("internal error: adapter panicked".into());
                        r.outcome.id = a.id().into();
                        r.outcome.detected = true;
                        r.outcome.error = r.info.last_error.clone();
                        r
                    });
                let _ = tx.send((a.id().to_string(), res));
            });
        }
        drop(tx);

        for (done, (id, res)) in rx.iter().enumerate() {
            let saved = lock(db).and_then(|d| d.save_manager(&res.info, res.packages.as_deref()));
            let mut outcome = res.outcome;
            if let Err(e) = saved {
                outcome.ok = false;
                outcome.error = Some(e.to_string());
            }
            if outcome.error.is_some() {
                failures += 1;
            }
            pkgs += outcome.package_count;
            upds += outcome.update_count;
            let message = match (&outcome.error, outcome.detected) {
                (Some(e), _) => format!("{id}: failed - {e}"),
                (None, false) => format!("{id}: not installed"),
                (None, true) => format!("{id}: {} packages", outcome.package_count),
            };
            emit(ScanProgress { phase: "manager", manager: Some(id), done: done + 1, total, message });
            outcomes.push(outcome);
        }
    });

    outcomes.sort_by(|a, b| a.id.cmp(&b.id));
    let status = if failures == 0 {
        "completed"
    } else if failures < outcomes.iter().filter(|o| o.detected).count() {
        "partial"
    } else {
        "failed"
    };
    let d = lock(db)?;
    d.finish_scan(scan_id, status, pkgs, upds, &outcomes)?;
    emit(ScanProgress { phase: "finished", manager: None, done: total, total, message: format!("Scan {status}") });
    d.recent_scans(1)?.into_iter().next().ok_or_else(|| AppError::Other("scan record missing".into()))
}

fn lock(db: &Mutex<Db>) -> Result<std::sync::MutexGuard<'_, Db>> {
    db.lock().map_err(|_| AppError::Db("database lock poisoned".into()))
}

/// Placeholder adapter used only to build a result shell after a panic.
struct Unavailable;
impl Adapter for Unavailable {
    fn id(&self) -> &'static str {
        "unavailable"
    }
    fn display_name(&self) -> &'static str {
        "unavailable"
    }
    fn binary(&self) -> &'static str {
        "unavailable"
    }
    fn version(&self, _: &dyn Runner) -> Result<String> {
        Err(AppError::Unsupported("n/a".into()))
    }
    fn list_installed(&self, _: &dyn Runner) -> Result<Vec<Package>> {
        Err(AppError::Unsupported("n/a".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::{npm::Npm, uv::Uv};
    use crate::exec::fake::FakeRunner;

    #[test]
    fn updates_mark_unlisted_as_up_to_date() {
        let mut v = vec![
            Package::new("npm", "npm:a".into(), "a"),
            Package::new("npm", "npm:b".into(), "b"),
        ];
        v[0].version = Some("1.0.0".into());
        v[1].version = Some("2.0.0".into());
        let n = apply_updates(&mut v, &[UpdateInfo { name: "a".into(), latest_version: "1.1.0".into() }]);
        assert_eq!(n, 1);
        assert_eq!(v[0].update_status, UpdateStatus::Available);
        assert_eq!(v[0].latest_version.as_deref(), Some("1.1.0"));
        assert_eq!(v[1].update_status, UpdateStatus::UpToDate);
        assert_eq!(v[1].update_available, Some(false));
    }

    #[test]
    fn undetected_manager_is_not_an_error() {
        let r = FakeRunner::new(&[]);
        let res = scan_adapter(&Npm, &r, true);
        assert!(!res.info.detected && res.packages.is_none() && res.outcome.error.is_none());
    }

    #[test]
    fn failure_is_isolated_and_reported() {
        // uv "found" but its command has no fixture -> list fails.
        let r = FakeRunner::new(&["uv"]).on("uv --version", 0, "uv 0.4.0\n");
        let res = scan_adapter(&Uv, &r, true);
        assert!(res.info.detected);
        assert!(res.packages.is_none());
        assert!(res.outcome.error.is_some());
        assert_eq!(res.info.version.as_deref(), Some("uv 0.4.0"));
    }

    #[test]
    fn unsupported_updates_stay_unknown() {
        let r = FakeRunner::new(&["uv"]).on("uv --version", 0, "uv 0.4.0\n").on("uv tool list", 0, "ruff v0.4.1\n- ruff\n");
        let res = scan_adapter(&Uv, &r, true);
        let p = &res.packages.unwrap()[0];
        assert_eq!(p.update_status, UpdateStatus::Unknown);
        assert_eq!(p.update_available, None);
    }

    #[test]
    fn full_scan_persists_and_reports_partial() {
        let db = Mutex::new(Db::open_in_memory().unwrap());
        let r = FakeRunner::new(&["uv", "npm"])
            .on("uv --version", 0, "uv 0.4.0\n")
            .on("uv tool list", 0, "ruff v0.4.1\n- ruff\n")
            .on("npm --version", 0, "10.5.0\n");
        // npm has no `ls` fixture -> fails; uv succeeds => partial.
        let adapters: Vec<Box<dyn Adapter>> = vec![Box::new(Uv), Box::new(Npm)];
        let events = Mutex::new(Vec::new());
        let rec = scan_all(&adapters, &r, &db, true, &|e| events.lock().unwrap().push(e.phase)).unwrap();
        assert_eq!(rec.status, "partial");
        assert_eq!(rec.package_count, 1);
        assert_eq!(db.lock().unwrap().list_packages().unwrap().len(), 1);
        let ev = events.lock().unwrap();
        assert_eq!(ev.first(), Some(&"started"));
        assert_eq!(ev.last(), Some(&"finished"));
    }
}
