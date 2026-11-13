//! Smoke test against the machine's REAL package managers (no fixtures).
//! Ignored by default because the result depends on what is installed:
//!   cargo test --test real_system -- --ignored --nocapture
//! It only reads: scan, search, details and dependency queries. Nothing is installed.

use std::sync::Mutex;
use unimanage_lib::adapters;
use unimanage_lib::db::Db;
use unimanage_lib::exec::SystemRunner;
use unimanage_lib::scanner;

#[test]
#[ignore]
fn real_scan_search_details_dependencies() {
    let runner = SystemRunner::default();
    let all = adapters::all();
    let db = Mutex::new(Db::open_in_memory().unwrap());

    let rec = scanner::scan_all(&all, &runner, &db, true, &|_| {}).unwrap();
    println!("scan: status={} packages={} updates={}", rec.status, rec.package_count, rec.update_count);

    let db = db.lock().unwrap();
    for m in db.list_managers().unwrap() {
        println!(
            "manager {:<8} detected={:<5} packages={:<5} version={:?} error={:?}",
            m.id, m.detected, m.package_count, m.version, m.last_error
        );
    }
    let pkgs = db.list_packages().unwrap();
    for id in ["apt", "snap", "flatpak"] {
        let sample: Vec<_> = pkgs.iter().filter(|p| p.package_manager == id).take(3).map(|p| format!("{} {:?}", p.name, p.version)).collect();
        println!("sample {id}: {sample:?}");
    }
    let upd: Vec<_> = pkgs.iter().filter(|p| p.update_available == Some(true)).take(3).map(|p| format!("{}:{} -> {:?}", p.package_manager, p.name, p.latest_version)).collect();
    println!("updates sample: {upd:?}");

    for a in &all {
        if a.detect(&runner).is_none() {
            continue;
        }
        if a.supports_search() {
            match a.search(&runner, "curl", 5) {
                Ok(h) => {
                    println!("{} search 'curl': {} hits, first={:?}", a.id(), h.len(), h.first().map(|x| &x.name));
                    if let Some(first) = h.first() {
                        match a.details(&runner, &first.name) {
                            Ok(d) => println!("{} details {}: version={:?} desc={:?}", a.id(), d.name, d.version, d.description.as_deref().map(|s| &s[..s.len().min(60)])),
                            Err(e) => println!("{} details ERROR: {e}", a.id()),
                        }
                    }
                }
                Err(e) => println!("{} search ERROR: {e}", a.id()),
            }
        }
        if a.supports_dependencies() {
            if let Some(p) = pkgs.iter().find(|p| p.package_manager == a.id()) {
                match a.dependencies(&runner, &p.name) {
                    Ok(d) => println!(
                        "{} dependencies {}: direct={} transitive={} required_by={} orphan_hint={}",
                        a.id(), d.name, d.depends_on.len(), d.transitive.len(), d.required_by.len(), d.orphan_hint.is_some()
                    ),
                    Err(e) => println!("{} dependencies ERROR: {e}", a.id()),
                }
            }
        }
    }

    // Orphan hints for auto-installed apt packages must agree with apt's own (simulated) autoremove.
    if let Some(apt) = all.iter().find(|a| a.id() == "apt").filter(|a| a.detect(&runner).is_some()) {
        let sim = std::process::Command::new("apt-get").args(["-s", "autoremove"]).output();
        if let Ok(sim) = sim {
            let text = String::from_utf8_lossy(&sim.stdout);
            let removable: std::collections::HashSet<&str> =
                text.lines().filter_map(|l| l.strip_prefix("Remv ")).filter_map(|l| l.split_whitespace().next()).collect();
            let mut auto: Vec<_> = pkgs
                .iter()
                .filter(|p| p.package_manager == "apt" && p.metadata.get("installReason").map(String::as_str) == Some("automatic"))
                .collect();
            // Candidates apt would remove go first so they are always part of the sample.
            auto.sort_by_key(|p| !removable.contains(p.name.as_str()));
            auto.truncate(60);
            let (mut hinted, mut disagree) = (0, Vec::new());
            for p in &auto {
                if let Ok(d) = apt.dependencies(&runner, &p.name) {
                    if d.orphan_hint.is_some() {
                        hinted += 1;
                        if !removable.contains(p.name.as_str()) {
                            disagree.push(p.name.clone());
                        }
                    }
                }
            }
            println!(
                "orphan check: {} auto sampled, {} hinted, {} autoremove candidates overall, hinted-but-not-autoremovable={:?}",
                auto.len(), hinted, removable.len(), disagree
            );
        }
    }

    if cfg!(target_os = "linux") {
        assert!(!pkgs.is_empty(), "expected at least one package on a Linux machine");
    }
}
