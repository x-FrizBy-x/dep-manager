//! Operation pipeline (write path): plan -> preview -> explicit confirmation
//! -> execute with live output -> record -> rescan that manager.
//!
//! Safety rules enforced here:
//! * Only a fixed set of (manager, operation) combinations can be planned.
//! * Package names / versions are strictly validated; they are passed as
//!   separate argv entries (never a shell string) and can never start with `-`.
//! * A plan can only be executed once, by id, after it was previewed.
//! * Everything is logged to `operations` / `operation_logs`.

use crate::db::{Db, OperationRecord};
use crate::error::{AppError, Result};
use crate::exec::run_streaming;
use serde::{Deserialize, Serialize};
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OpKind {
    Install,
    Update,
    Uninstall,
}

impl OpKind {
    pub fn as_str(self) -> &'static str {
        match self {
            OpKind::Install => "install",
            OpKind::Update => "update",
            OpKind::Uninstall => "uninstall",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationPlan {
    pub id: String,
    pub manager_id: String,
    pub kind: OpKind,
    pub package: String,
    /// Target version (install/update); `None` = latest.
    pub version: Option<String>,
    pub previous_version: Option<String>,
    pub program: String,
    pub args: Vec<String>,
    /// Human-readable rendering for the preview. Display only, never executed.
    pub command: String,
    pub requires_elevation: bool,
    /// False for elevated operations: a root child cannot be stopped by an unprivileged
    /// app, and killing apt/snap halfway can leave the system in a broken state.
    pub cancellable: bool,
    /// What a dry run says will change (apt: `Inst ...` / `Remv ...` lines). Display only.
    pub effects: Vec<String>,
    pub warnings: Vec<String>,
    #[serde(skip)]
    pub created_at: i64,
}

/// Managers that currently support write operations. npm, pipx and per-user Flatpak
/// need no privileges; apt and snap run through polkit's `pkexec` (see `ELEVATOR`).
pub fn supports_operations(manager_id: &str) -> bool {
    matches!(manager_id, "npm" | "pipx" | "flatpak" | "apt" | "snap")
}

/// The only way UniManage gains privileges: polkit shows its own authentication
/// dialog and UniManage never sees, asks for or stores a password. Absolute path so
/// a poisoned PATH cannot substitute another program.
pub const ELEVATOR: &str = "/usr/bin/pkexec";

/// Managers whose operations run as root through `ELEVATOR`.
pub fn needs_elevation(manager_id: &str) -> bool {
    matches!(manager_id, "apt" | "snap")
}

/// Whether an exact version can be requested at install time. Flatpak installs a
/// branch, not a pinned version, so manifests restore it unpinned.
pub fn supports_version_pin(manager_id: &str) -> bool {
    matches!(manager_id, "npm" | "pipx")
}

/// Facts about the package taken from UniManage's own inventory (never from the caller).
#[derive(Debug, Clone, Default)]
pub struct PlanContext {
    pub previous_version: Option<String>,
    /// Flatpak: the remote that carries the package (resolved before install).
    pub remote: Option<String>,
    /// Flatpak: `user` or `system` as recorded by the scan.
    pub scope: Option<String>,
    /// apt: lines of an unprivileged `apt-get -s` dry run for this exact operation.
    pub simulation: Vec<String>,
}

/// Keeps the `Inst`/`Remv`/`Purg` lines of `apt-get -s` output (what would change).
pub fn parse_apt_simulation(stdout: &str) -> Vec<String> {
    stdout
        .lines()
        .filter(|l| l.starts_with("Inst ") || l.starts_with("Remv ") || l.starts_with("Purg "))
        .map(|l| l.trim().to_string())
        .collect()
}

/// Packages whose removal can make the machine unbootable or lock the user out.
const APT_PROTECTED: &[&str] = &[
    "apt", "dpkg", "libc6", "systemd", "sudo", "policykit-1", "polkitd", "pkexec", "bash", "coreutils",
    "ubuntu-minimal", "ubuntu-standard", "ubuntu-desktop", "ubuntu-server", "snapd", "init", "login",
];

/// Snaps that other snaps or snapd itself are built on.
fn snap_is_protected(name: &str) -> bool {
    name == "snapd"
        || name == "bare"
        || name == "core"
        || (name.starts_with("core") && name[4..].chars().all(|c| c.is_ascii_digit()))
}

/// PyPI-style project names: letters, digits, `.`, `_`, `-`; must start and end alphanumeric.
pub fn validate_pipx_name(name: &str) -> Result<()> {
    let ok = !name.is_empty()
        && name.len() <= 100
        && name.starts_with(|c: char| c.is_ascii_alphanumeric())
        && name.ends_with(|c: char| c.is_ascii_alphanumeric())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if ok {
        Ok(())
    } else {
        Err(AppError::Other(format!("invalid pipx package name `{name}`")))
    }
}

/// npm package name rules (subset): optional `@scope/`, lowercase letters,
/// digits, `-`, `.`, `_`, `~`; max 214 chars; no leading `.`/`_`/`-`.
pub fn validate_npm_name(name: &str) -> Result<()> {
    let bad = |why: &str| Err(AppError::Other(format!("invalid npm package name `{name}`: {why}")));
    if name.is_empty() || name.len() > 214 {
        return bad("length must be 1-214");
    }
    let (scope, base) = match name.strip_prefix('@') {
        Some(rest) => match rest.split_once('/') {
            Some((s, b)) => (Some(s), b),
            None => return bad("scoped name needs `@scope/name`"),
        },
        None => (None, name),
    };
    for part in scope.into_iter().chain(std::iter::once(base)) {
        if part.is_empty() || part.starts_with(['.', '_', '-']) {
            return bad("segments must not be empty or start with . _ -");
        }
        if !part.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '.' | '_' | '~')) {
            return bad("only a-z 0-9 - . _ ~ allowed");
        }
    }
    Ok(())
}

/// Versions or dist-tags: `1.2.3`, `1.2.3-beta.1+build`, `latest`, `next`.
pub fn validate_version(v: &str) -> Result<()> {
    let ok = !v.is_empty()
        && v.len() <= 64
        && v.starts_with(|c: char| c.is_ascii_alphanumeric())
        && v.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+' | '_'));
    if ok {
        Ok(())
    } else {
        Err(AppError::Other(format!("invalid version `{v}`")))
    }
}

/// program + argv + warnings for one (manager, operation). Pure.
type Built = (&'static str, Vec<String>, Vec<String>);

fn strs(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}

fn build_npm(kind: OpKind, package: &str, version: Option<&str>) -> Result<Built> {
    validate_npm_name(package)?;
    let mut warnings = Vec::new();
    let args = match kind {
        OpKind::Install | OpKind::Update => {
            let target = version.unwrap_or("latest");
            vec!["install".into(), "-g".into(), "--no-fund".into(), "--no-audit".into(), format!("{package}@{target}")]
        }
        OpKind::Uninstall => {
            if package == "npm" {
                return Err(AppError::Other("refusing to uninstall npm itself through UniManage".into()));
            }
            warnings.push("Packages that depend on this one are not checked; nothing else is removed.".into());
            strs(&["uninstall", "-g", package])
        }
    };
    if cfg!(unix) {
        warnings.push(
            "If the global npm prefix is root-owned this will fail with EACCES; UniManage will not escalate privileges."
                .into(),
        );
    }
    Ok(("npm", args, warnings))
}

fn build_pipx(kind: OpKind, package: &str, version: Option<&str>, ctx: &PlanContext) -> Result<Built> {
    validate_pipx_name(package)?;
    let mut warnings = vec!["Cancelling can leave a partly created environment; the rescan shows the real state.".to_string()];
    let args = match kind {
        OpKind::Install => {
            let mut a = strs(&["install"]);
            if ctx.previous_version.is_some() {
                // Already installed: pipx would refuse; --force reinstalls it at the requested version.
                a.push("--force".into());
                warnings.push("Already installed: --force reinstalls this package's environment.".into());
            }
            a.push(match version {
                Some(v) => format!("{package}=={v}"),
                None => package.to_string(),
            });
            a
        }
        OpKind::Update => {
            if version.is_some() {
                return Err(AppError::Other("pipx upgrade cannot target a version; install a pinned version instead".into()));
            }
            strs(&["upgrade", package])
        }
        OpKind::Uninstall => strs(&["uninstall", package]),
    };
    Ok(("pipx", args, warnings))
}

fn build_flatpak(kind: OpKind, package: &str, version: Option<&str>, ctx: &PlanContext) -> Result<Built> {
    crate::adapters::flatpak::validate_app_id(package)?;
    if version.is_some() {
        return Err(AppError::Other("Flatpak installs a branch, not a pinned version; leave the version empty".into()));
    }
    // UniManage never escalates: only per-user installations are changed.
    let mut warnings = vec!["Only the per-user Flatpak installation is changed (no root).".to_string()];
    let args = match kind {
        OpKind::Install => {
            let remote = ctx.remote.as_deref().ok_or_else(|| AppError::Other("no Flatpak remote carries this app".into()))?;
            if !crate::adapters::flatpak::validate_remote_name(remote) {
                return Err(AppError::Other(format!("invalid Flatpak remote `{remote}`")));
            }
            warnings.push("Runtimes the app needs may be downloaded too; this can be large.".into());
            strs(&["install", "--user", "--noninteractive", "-y", remote, package])
        }
        OpKind::Update | OpKind::Uninstall => {
            match ctx.scope.as_deref() {
                Some("user") => {}
                Some(other) => {
                    return Err(AppError::Other(format!(
                        "{package} is installed for `{other}`; UniManage only changes per-user Flatpak installations because it never escalates privileges"
                    )))
                }
                None => {
                    return Err(AppError::Other(format!("cannot tell whether {package} is a per-user installation; rescan first")))
                }
            }
            if kind == OpKind::Update {
                strs(&["update", "--user", "--noninteractive", "-y", package])
            } else {
                warnings.push("Unused runtimes and the app's data in ~/.var are kept.".into());
                strs(&["uninstall", "--user", "--noninteractive", "-y", package])
            }
        }
    };
    Ok(("flatpak", args, warnings))
}

/// Wraps a privileged command: `pkexec env DEBIAN_FRONTEND=... <tool> args`. `pkexec` starts
/// with a clean environment, so the non-interactive setting must be passed explicitly.
fn elevated(tool: &str, args: Vec<String>, noninteractive: bool) -> Vec<String> {
    let mut v = Vec::new();
    if noninteractive {
        v.push("/usr/bin/env".to_string());
        v.push("DEBIAN_FRONTEND=noninteractive".to_string());
    }
    v.push(tool.to_string());
    v.extend(args);
    v
}

fn build_apt(kind: OpKind, package: &str, version: Option<&str>, ctx: &PlanContext) -> Result<Built> {
    crate::adapters::apt::validate_apt_name(package)?;
    if version.is_some() {
        return Err(AppError::Other("apt installs the version your repositories offer; leave the version empty".into()));
    }
    let mut warnings = vec![
        "Runs as root: your system will show its own password dialog. UniManage never sees the password.".to_string(),
        "This cannot be cancelled once started; stopping apt halfway can leave packages half-configured.".to_string(),
    ];
    let args = match kind {
        OpKind::Install => {
            if ctx.previous_version.is_some() {
                return Err(AppError::Other(format!("{package} is already installed; use update instead")));
            }
            vec!["install".to_string(), "-y".into(), package.into()]
        }
        OpKind::Update => {
            if ctx.previous_version.is_none() {
                return Err(AppError::Other(format!("{package} is not installed")));
            }
            // --only-upgrade: upgrades this package only, never installs anything new by itself.
            vec!["install".to_string(), "--only-upgrade".into(), "-y".into(), package.into()]
        }
        OpKind::Uninstall => {
            if APT_PROTECTED.contains(&package) {
                return Err(AppError::Other(format!("refusing to remove `{package}`: the system depends on it")));
            }
            warnings.push(
                "`remove` keeps configuration files; dependencies that become unused are NOT removed (no autoremove).".into(),
            );
            vec!["remove".to_string(), "-y".into(), package.into()]
        }
    };
    Ok((ELEVATOR, elevated("/usr/bin/apt-get", args, true), warnings))
}

fn build_snap(kind: OpKind, package: &str, version: Option<&str>, ctx: &PlanContext) -> Result<Built> {
    crate::adapters::snap::validate_snap_name(package)?;
    if version.is_some() {
        return Err(AppError::Other("snap installs from a channel, not a pinned version; leave the version empty".into()));
    }
    let mut warnings = vec![
        "Runs as root: your system will show its own password dialog. UniManage never sees the password.".to_string(),
        "This cannot be cancelled once started.".to_string(),
    ];
    let args = match kind {
        OpKind::Install => {
            if ctx.previous_version.is_some() {
                return Err(AppError::Other(format!("{package} is already installed; use update instead")));
            }
            warnings.push(
                "Installs from the stable channel. Snaps that need classic confinement will fail and are not forced.".into(),
            );
            vec!["install".to_string(), package.into()]
        }
        OpKind::Update => {
            if ctx.previous_version.is_none() {
                return Err(AppError::Other(format!("{package} is not installed")));
            }
            vec!["refresh".to_string(), package.into()]
        }
        OpKind::Uninstall => {
            if snap_is_protected(package) {
                return Err(AppError::Other(format!("refusing to remove `{package}`: other snaps or snapd depend on it")));
            }
            warnings.push("snapd saves a snapshot of the snap's data automatically before removing it.".into());
            vec!["remove".to_string(), package.into()]
        }
    };
    Ok((ELEVATOR, elevated("/usr/bin/snap", args, false), warnings))
}

/// Builds a plan. Pure: runs nothing.
pub fn plan_operation(
    id: String,
    now: i64,
    manager_id: &str,
    kind: OpKind,
    package: &str,
    version: Option<&str>,
    ctx: &PlanContext,
) -> Result<OperationPlan> {
    if !supports_operations(manager_id) {
        return Err(AppError::Unsupported(format!(
            "{manager_id}: write operations are not implemented for this manager yet"
        )));
    }
    if let Some(v) = version {
        validate_version(v)?;
    }
    let (program, args, mut warnings) = match manager_id {
        "npm" => build_npm(kind, package, version)?,
        "pipx" => build_pipx(kind, package, version, ctx)?,
        "flatpak" => build_flatpak(kind, package, version, ctx)?,
        "apt" => build_apt(kind, package, version, ctx)?,
        "snap" => build_snap(kind, package, version, ctx)?,
        other => return Err(AppError::Unsupported(format!("{other}: write operations are not implemented"))),
    };
    if kind == OpKind::Update && ctx.previous_version.is_none() {
        warnings.push("Installed version is unknown.".into());
    }
    let elevated = needs_elevation(manager_id);
    let effects = ctx.simulation.clone();
    if manager_id == "apt" {
        let removed = effects.iter().filter(|l| l.starts_with("Remv ") || l.starts_with("Purg ")).count();
        match kind {
            OpKind::Uninstall if removed > 1 => warnings.insert(
                0,
                format!("This will also REMOVE {} other package(s) that depend on {package}. Review the list below.", removed - 1),
            ),
            OpKind::Install | OpKind::Update if removed > 0 => warnings.insert(
                0,
                format!("This will REMOVE {removed} installed package(s) to resolve conflicts. Review the list below."),
            ),
            _ => {}
        }
        if effects.is_empty() {
            warnings.push("The dry run reported no changes; this may do nothing.".into());
        }
    }
    let command = format!("{program} {}", args.join(" "));
    Ok(OperationPlan {
        id,
        manager_id: manager_id.into(),
        kind,
        package: package.into(),
        version: version.map(String::from),
        previous_version: ctx.previous_version.clone(),
        program: program.into(),
        args,
        command,
        requires_elevation: elevated,
        cancellable: !elevated,
        effects,
        warnings,
        created_at: now,
    })
}

/// Generous because a Flatpak runtime download can be large; the user can always cancel.
pub const OP_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// Elevated operations cannot be cancelled, so only an extreme timeout applies.
const ELEVATED_TIMEOUT: Duration = Duration::from_secs(6 * 60 * 60);

fn lock(db: &Mutex<Db>) -> Result<std::sync::MutexGuard<'_, Db>> {
    db.lock().map_err(|_| AppError::Db("database lock poisoned".into()))
}

/// Executes a confirmed plan. Records the operation and every output line,
/// streams lines to `on_line(op_id, stream, line)`, then calls `rescan` (which
/// should refresh that manager's inventory and return the package's new
/// installed version, if any).
pub fn execute(
    plan: &OperationPlan,
    db: &Mutex<Db>,
    cancel: &AtomicBool,
    timeout: Duration,
    on_started: &(dyn Fn(i64) + Sync),
    on_line: &(dyn Fn(i64, &str, &str) + Sync),
    rescan: &dyn Fn() -> Option<String>,
) -> Result<OperationRecord> {
    let id = lock(db)?.begin_operation(
        plan.kind.as_str(),
        &plan.package,
        &plan.manager_id,
        plan.previous_version.as_deref(),
        &plan.command,
    )?;
    on_started(id);
    let log = |stream: &str, line: &str| {
        if let Ok(d) = lock(db) {
            let _ = d.append_log(id, stream, line);
        }
        on_line(id, stream, line);
    };
    log("info", &format!("$ {}", plan.command));

    let args: Vec<&str> = plan.args.iter().map(String::as_str).collect();
    // A privileged child cannot be signalled by us, and killing only `pkexec` would leave apt
    // running unobserved, so elevated operations ignore cancel and only stop at a long timeout.
    let never = AtomicBool::new(false);
    let (cancel, timeout) = if plan.requires_elevation { (&never, ELEVATED_TIMEOUT) } else { (cancel, timeout) };
    let run = run_streaming(&plan.program, &args, timeout, cancel, &|s, l| log(s, l));

    let (result, error) = match run {
        Err(e) => ("failed", Some(e.to_string())),
        Ok(r) if r.cancelled => ("cancelled", Some("cancelled by user".to_string())),
        Ok(r) if r.timed_out => ("timeout", Some(format!("timed out after {}s", timeout.as_secs()))),
        Ok(r) if r.status == Some(0) => ("success", None),
        Ok(r) => ("failed", Some(format!("exited with status {}", r.status.map_or("unknown".into(), |c| c.to_string())))),
    };
    log("info", &format!("result: {result}"));
    if let Some(e) = &error {
        log("info", e);
    }

    // Even after a failed/cancelled run the inventory may have changed.
    log("info", "refreshing inventory");
    let new_version = rescan();
    lock(db)?.finish_operation(id, result, new_version.as_deref(), error.as_deref())?;
    lock(db)?
        .get_operation(id)?
        .ok_or_else(|| AppError::Other("operation record missing".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(kind: OpKind, pkg: &str, ver: Option<&str>) -> Result<OperationPlan> {
        plan_operation("p1".into(), 0, "npm", kind, pkg, ver, &PlanContext { previous_version: Some("1.0.0".into()), ..Default::default() })
    }

    #[test]
    fn name_validation() {
        for ok in ["typescript", "@qwen-code/qwen-code", "left-pad", "a.b_c~d"] {
            assert!(validate_npm_name(ok).is_ok(), "{ok}");
        }
        for bad in ["", "-rf", "--registry=evil", "Foo", "a b", "a;b", "a&&b", "$(x)", "@scope", "@/x", "@s/", ".hidden", "_x", "a|b", "a\nb", "../x"] {
            assert!(validate_npm_name(bad).is_err(), "{bad}");
        }
        assert!(validate_npm_name(&"a".repeat(215)).is_err());
    }

    #[test]
    fn version_validation() {
        for ok in ["1.2.3", "latest", "1.0.0-beta.1+b5", "next"] {
            assert!(validate_version(ok).is_ok(), "{ok}");
        }
        for bad in ["", "-1", "1 2", "1;2", "^1.0.0", "1.0.0 --force", "x".repeat(65).as_str()] {
            assert!(validate_version(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn update_plan_is_an_argv_array() {
        let p = plan(OpKind::Update, "typescript", None).unwrap();
        assert_eq!(p.program, "npm");
        assert_eq!(p.args.last().unwrap(), "typescript@latest");
        assert!(!p.requires_elevation);
    }

    #[test]
    fn install_pins_version() {
        let p = plan(OpKind::Install, "@a/b", Some("2.0.1")).unwrap();
        assert_eq!(p.args.last().unwrap(), "@a/b@2.0.1");
    }

    fn harmless(args: &[&str]) -> OperationPlan {
        let mut p = plan(OpKind::Update, "typescript", None).unwrap();
        p.program = "cargo".into();
        p.args = args.iter().map(|s| s.to_string()).collect();
        p.command = format!("cargo {}", args.join(" "));
        p
    }

    #[test]
    fn execute_records_success_and_logs() {
        let db = Mutex::new(Db::open_in_memory().unwrap());
        let cancel = AtomicBool::new(false);
        let seen = Mutex::new(Vec::new());
        let rec = execute(
            &harmless(&["--version"]),
            &db,
            &cancel,
            Duration::from_secs(30),
            &|_| {},
            &|_, s, l| seen.lock().unwrap().push(format!("{s}:{l}")),
            &|| Some("9.9.9".into()),
        )
        .unwrap();
        assert_eq!(rec.result, "success");
        assert_eq!(rec.new_version.as_deref(), Some("9.9.9"));
        assert_eq!(rec.previous_version.as_deref(), Some("1.0.0"));
        assert!(seen.lock().unwrap().iter().any(|l| l.starts_with("stdout:cargo")));
        let logs = db.lock().unwrap().operation_logs(rec.id).unwrap();
        assert!(logs.iter().any(|l| l.stream == "stdout"));
        assert_eq!(logs[0].line, "$ cargo --version");
    }

    #[test]
    fn execute_records_failure_and_missing_program() {
        let db = Mutex::new(Db::open_in_memory().unwrap());
        let cancel = AtomicBool::new(false);
        let t = Duration::from_secs(30);
        let rec = execute(&harmless(&["--no-such-flag"]), &db, &cancel, t, &|_| {}, &|_, _, _| {}, &|| None).unwrap();
        assert_eq!(rec.result, "failed");
        assert!(rec.error.is_some());
        let mut p = harmless(&[]);
        p.program = "no-such-program-xyz".into();
        let rec = execute(&p, &db, &cancel, t, &|_| {}, &|_, _, _| {}, &|| None).unwrap();
        assert_eq!(rec.result, "failed");
    }

    #[test]
    fn uninstall_and_guards() {
        let p = plan(OpKind::Uninstall, "typescript", None).unwrap();
        assert_eq!(p.args, vec!["uninstall", "-g", "typescript"]);
        assert!(plan(OpKind::Uninstall, "npm", None).is_err());
        assert!(plan_operation("x".into(), 0, "apt", OpKind::Update, "curl", None, &PlanContext::default()).is_err());
        assert!(plan(OpKind::Install, "ok", Some("--force")).is_err());
    }

    fn plan_for(mgr: &str, kind: OpKind, pkg: &str, ver: Option<&str>, ctx: &PlanContext) -> Result<OperationPlan> {
        plan_operation("p".into(), 0, mgr, kind, pkg, ver, ctx)
    }

    #[test]
    fn pipx_plans() {
        let none = PlanContext::default();
        let p = plan_for("pipx", OpKind::Install, "black", None, &none).unwrap();
        assert_eq!(p.program, "pipx");
        assert_eq!(p.args, ["install", "black"]);
        assert_eq!(plan_for("pipx", OpKind::Install, "black", Some("24.1.0"), &none).unwrap().args, ["install", "black==24.1.0"]);
        let installed = PlanContext { previous_version: Some("23.0".into()), ..Default::default() };
        assert_eq!(
            plan_for("pipx", OpKind::Install, "black", Some("24.1.0"), &installed).unwrap().args,
            ["install", "--force", "black==24.1.0"]
        );
        assert_eq!(plan_for("pipx", OpKind::Update, "black", None, &installed).unwrap().args, ["upgrade", "black"]);
        assert!(plan_for("pipx", OpKind::Update, "black", Some("1.0"), &installed).is_err());
        assert_eq!(plan_for("pipx", OpKind::Uninstall, "black", None, &installed).unwrap().args, ["uninstall", "black"]);
        for bad in ["", "-x", "--pip-args=evil", "a b", "a;b", "git+https://x/y", "../x", "a/b", "x-", ".x", "$(x)"] {
            assert!(plan_for("pipx", OpKind::Install, bad, None, &none).is_err(), "{bad}");
        }
        assert!(plan_for("pipx", OpKind::Install, "black", Some("--force"), &none).is_err());
    }

    #[test]
    fn flatpak_plans_are_per_user_only() {
        let user = PlanContext { previous_version: Some("1".into()), scope: Some("user".into()), ..Default::default() };
        let sys = PlanContext { scope: Some("system".into()), ..Default::default() };
        let id = "org.mozilla.firefox";
        assert_eq!(
            plan_for("flatpak", OpKind::Update, id, None, &user).unwrap().args,
            ["update", "--user", "--noninteractive", "-y", id]
        );
        assert_eq!(
            plan_for("flatpak", OpKind::Uninstall, id, None, &user).unwrap().args,
            ["uninstall", "--user", "--noninteractive", "-y", id]
        );
        assert!(plan_for("flatpak", OpKind::Update, id, None, &sys).is_err(), "system installs are refused, not escalated");
        assert!(plan_for("flatpak", OpKind::Uninstall, id, None, &PlanContext::default()).is_err());
        let with_remote = PlanContext { remote: Some("flathub".into()), ..Default::default() };
        let p = plan_for("flatpak", OpKind::Install, id, None, &with_remote).unwrap();
        assert_eq!(p.args, ["install", "--user", "--noninteractive", "-y", "flathub", id]);
        assert!(!p.requires_elevation);
        assert!(plan_for("flatpak", OpKind::Install, id, None, &PlanContext::default()).is_err());
        assert!(plan_for("flatpak", OpKind::Install, id, Some("1.0"), &with_remote).is_err());
        for bad in ["firefox", "-org.x", "org.x;rm", "org.x y", "org..x"] {
            assert!(plan_for("flatpak", OpKind::Install, bad, None, &with_remote).is_err(), "{bad}");
        }
        let evil = PlanContext { remote: Some("--system".into()), ..Default::default() };
        assert!(plan_for("flatpak", OpKind::Install, id, None, &evil).is_err());
    }

    #[test]
    fn apt_plans_go_through_pkexec_without_a_shell() {
        let installed = PlanContext { previous_version: Some("1.0".into()), ..Default::default() };
        let none = PlanContext::default();
        let p = plan_operation("p".into(), 0, "apt", OpKind::Install, "hello", None, &none).unwrap();
        assert_eq!(p.program, ELEVATOR);
        assert_eq!(p.args, ["/usr/bin/env", "DEBIAN_FRONTEND=noninteractive", "/usr/bin/apt-get", "install", "-y", "hello"]);
        assert!(p.requires_elevation && !p.cancellable);
        let u = plan_operation("p".into(), 0, "apt", OpKind::Update, "hello", None, &installed).unwrap();
        assert!(u.args.contains(&"--only-upgrade".to_string()));
        let r = plan_operation("p".into(), 0, "apt", OpKind::Uninstall, "hello", None, &installed).unwrap();
        assert!(r.args.contains(&"remove".to_string()) && !r.command.contains("autoremove") && !r.command.contains("purge"));
        assert!(plan_operation("p".into(), 0, "apt", OpKind::Install, "hello", None, &installed).is_err());
        assert!(plan_operation("p".into(), 0, "apt", OpKind::Update, "hello", None, &none).is_err());
        assert!(plan_operation("p".into(), 0, "apt", OpKind::Install, "hello", Some("1.0"), &none).is_err());
        for bad in ["-oAPT::x=1", "Hello", "a b", "x;y", "a", "--purge"] {
            assert!(plan_operation("p".into(), 0, "apt", OpKind::Install, bad, None, &none).is_err(), "{bad}");
        }
        for protected in ["apt", "dpkg", "libc6", "systemd", "sudo", "policykit-1", "polkitd", "pkexec"] {
            assert!(plan_operation("p".into(), 0, "apt", OpKind::Uninstall, protected, None, &installed).is_err(), "{protected}");
        }
    }

    #[test]
    fn apt_simulation_drives_warnings() {
        let sim = parse_apt_simulation("Reading package lists...\nRemv foo [1.0]\nRemv bar [2.0]\nInst baz [1] (2 x)\nConf baz\n");
        assert_eq!(sim.len(), 3);
        let ctx = PlanContext { previous_version: Some("1.0".into()), simulation: sim, ..Default::default() };
        let p = plan_operation("p".into(), 0, "apt", OpKind::Uninstall, "foo", None, &ctx).unwrap();
        assert!(p.warnings[0].contains("REMOVE 1 other"));
        assert_eq!(p.effects.len(), 3);
        let u = plan_operation("p".into(), 0, "apt", OpKind::Update, "foo", None, &ctx).unwrap();
        assert!(u.warnings[0].contains("REMOVE 2"));
    }

    #[test]
    fn snap_plans() {
        let installed = PlanContext { previous_version: Some("1".into()), ..Default::default() };
        let none = PlanContext::default();
        let p = plan_operation("p".into(), 0, "snap", OpKind::Install, "hello-world", None, &none).unwrap();
        assert_eq!(p.program, ELEVATOR);
        assert_eq!(p.args, ["/usr/bin/snap", "install", "hello-world"]);
        let u = plan_operation("p".into(), 0, "snap", OpKind::Update, "hello-world", None, &installed).unwrap();
        assert_eq!(u.args[1], "refresh");
        for protected in ["snapd", "core", "core22", "bare"] {
            assert!(plan_operation("p".into(), 0, "snap", OpKind::Uninstall, protected, None, &installed).is_err(), "{protected}");
        }
        assert!(plan_operation("p".into(), 0, "snap", OpKind::Install, "--classic", None, &none).is_err());
        assert!(plan_operation("p".into(), 0, "snap", OpKind::Install, "x", Some("1"), &none).is_err());
    }

    #[test]
    fn capabilities() {
        assert!(supports_operations("pipx") && supports_operations("flatpak") && supports_operations("apt") && supports_operations("snap") && !supports_operations("uv"));
        assert!(supports_version_pin("npm") && supports_version_pin("pipx") && !supports_version_pin("flatpak") && !supports_version_pin("apt"));
        assert!(needs_elevation("apt") && needs_elevation("snap") && !needs_elevation("pipx"));
    }
}
