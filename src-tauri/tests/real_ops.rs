//! Write-path smoke test against REAL pipx and Flatpak (no fixtures). It installs,
//! upgrades and uninstalls one tiny throwaway package per manager and always
//! tries to clean up. Ignored by default:
//!   cargo test --test real_ops -- --ignored --nocapture --test-threads=1
//! Per-user only; never needs root.

use std::sync::atomic::AtomicBool;
use std::sync::Mutex;
use std::time::Duration;
use unimanage_lib::adapters::{self, Adapter};
use unimanage_lib::db::Db;
use unimanage_lib::exec::SystemRunner;
use unimanage_lib::ops::{self, OpKind, PlanContext};

fn adapter(id: &str) -> Box<dyn Adapter> {
    adapters::all().into_iter().find(|a| a.id() == id).unwrap()
}

fn installed(id: &str, name: &str) -> Option<unimanage_lib::model::Package> {
    adapter(id).list_installed(&SystemRunner::default()).unwrap().into_iter().find(|p| p.name == name)
}

/// plan -> execute -> returns (result, new_version)
fn run(db: &Mutex<Db>, mgr: &str, kind: OpKind, pkg: &str, ver: Option<&str>, ctx: &PlanContext) -> (String, Option<String>) {
    let plan = ops::plan_operation("t".into(), 0, mgr, kind, pkg, ver, ctx).unwrap();
    println!("$ {}", plan.command);
    let cancel = AtomicBool::new(false);
    let rec = ops::execute(
        &plan,
        db,
        &cancel,
        Duration::from_secs(20 * 60),
        &|_| {},
        &|_, s, l| println!("  [{s}] {l}"),
        &|| installed(mgr, pkg).and_then(|p| p.version),
    )
    .unwrap();
    println!("=> {} (new version {:?})", rec.result, rec.new_version);
    (rec.result, rec.new_version)
}

fn best_effort(cmd: &str, args: &[&str]) {
    let _ = std::process::Command::new(cmd).args(args).output();
}

#[test]
#[ignore]
fn pipx_install_pinned_upgrade_uninstall() {
    if adapter("pipx").detect(&SystemRunner::default()).is_none() {
        println!("pipx not installed; skipping");
        return;
    }
    let db = Mutex::new(Db::open_in_memory().unwrap());
    let pkg = "cowsay";
    best_effort("pipx", &["uninstall", pkg]);
    assert!(installed("pipx", pkg).is_none(), "{pkg} must not be installed before the test");

    let none = PlanContext::default();
    let (r, v) = run(&db, "pipx", OpKind::Install, pkg, Some("5.0"), &none);
    assert_eq!(r, "success");
    assert_eq!(v.as_deref(), Some("5.0"));

    let ctx = PlanContext { previous_version: v, ..Default::default() };
    let (r, v2) = run(&db, "pipx", OpKind::Update, pkg, None, &ctx);
    assert_eq!(r, "success");
    assert_ne!(v2.as_deref(), Some("5.0"), "upgrade should have moved past 5.0");

    // Pinned reinstall of an installed package goes through --force.
    let ctx = PlanContext { previous_version: v2, ..Default::default() };
    let (r, v3) = run(&db, "pipx", OpKind::Install, pkg, Some("5.0"), &ctx);
    assert_eq!(r, "success");
    assert_eq!(v3.as_deref(), Some("5.0"));

    let (r, _) = run(&db, "pipx", OpKind::Uninstall, pkg, None, &PlanContext::default());
    assert_eq!(r, "success");
    assert!(installed("pipx", pkg).is_none());
    best_effort("pipx", &["uninstall", pkg]);
}

#[test]
#[ignore]
fn flatpak_install_update_uninstall_per_user() {
    let fp = adapter("flatpak");
    let runner = SystemRunner::default();
    if fp.detect(&runner).is_none() {
        println!("flatpak not installed; skipping");
        return;
    }
    let db = Mutex::new(Db::open_in_memory().unwrap());
    let app = "cc.nift.nsm";
    best_effort("flatpak", &["uninstall", "--user", "--noninteractive", "-y", app]);
    assert!(installed("flatpak", app).is_none(), "{app} must not be installed before the test");

    // Same resolution the preview command does: which remote carries it?
    let remote = fp.details(&runner, app).unwrap().metadata.get("remote").cloned();
    println!("remote = {remote:?}");
    let ctx = PlanContext { remote, ..Default::default() };
    let (r, v) = run(&db, "flatpak", OpKind::Install, app, None, &ctx);
    assert_eq!(r, "success");
    assert!(v.is_some(), "installed app should show up in the rescan");
    let pkg = installed("flatpak", app).unwrap();
    assert_eq!(pkg.metadata.get("installation").map(String::as_str), Some("user"));

    let ctx = PlanContext { previous_version: v, scope: Some("user".into()), ..Default::default() };
    let (r, _) = run(&db, "flatpak", OpKind::Update, app, None, &ctx);
    assert_eq!(r, "success");

    let (r, _) = run(&db, "flatpak", OpKind::Uninstall, app, None, &ctx);
    assert_eq!(r, "success");
    assert!(installed("flatpak", app).is_none());
    best_effort("flatpak", &["uninstall", "--user", "--noninteractive", "-y", app]);

    // A system-wide installation must be refused, never escalated.
    let sys = PlanContext { scope: Some("system".into()), ..Default::default() };
    assert!(ops::plan_operation("t".into(), 0, "flatpak", OpKind::Uninstall, app, None, &sys).is_err());
}
