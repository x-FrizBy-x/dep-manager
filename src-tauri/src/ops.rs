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
    pub warnings: Vec<String>,
    #[serde(skip)]
    pub created_at: i64,
}

/// Managers that currently support write operations.
pub fn supports_operations(manager_id: &str) -> bool {
    manager_id == "npm"
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

/// Builds a plan. Pure: runs nothing.
pub fn plan_operation(
    id: String,
    now: i64,
    manager_id: &str,
    kind: OpKind,
    package: &str,
    version: Option<&str>,
    previous_version: Option<&str>,
) -> Result<OperationPlan> {
    if !supports_operations(manager_id) {
        return Err(AppError::Unsupported(format!(
            "{manager_id}: write operations are not implemented for this manager yet"
        )));
    }
    validate_npm_name(package)?;
    if let Some(v) = version {
        validate_version(v)?;
    }
    let mut warnings = Vec::new();
    let args: Vec<String> = match kind {
        OpKind::Install | OpKind::Update => {
            let target = version.unwrap_or("latest");
            vec!["install".into(), "-g".into(), "--no-fund".into(), "--no-audit".into(), format!("{package}@{target}")]
        }
        OpKind::Uninstall => {
            if package == "npm" {
                return Err(AppError::Other("refusing to uninstall npm itself through UniManage".into()));
            }
            warnings.push("Packages that depend on this one are not checked; nothing else is removed.".into());
            vec!["uninstall".into(), "-g".into(), package.into()]
        }
    };
    if kind == OpKind::Update && previous_version.is_none() {
        warnings.push("Installed version is unknown.".into());
    }
    if cfg!(unix) {
        warnings.push(
            "If the global npm prefix is root-owned this will fail with EACCES; UniManage will not escalate privileges."
                .into(),
        );
    }
    let command = format!("npm {}", args.join(" "));
    Ok(OperationPlan {
        id,
        manager_id: manager_id.into(),
        kind,
        package: package.into(),
        version: version.map(String::from),
        previous_version: previous_version.map(String::from),
        program: "npm".into(),
        args,
        command,
        requires_elevation: false,
        warnings,
        created_at: now,
    })
}

pub const OP_TIMEOUT: Duration = Duration::from_secs(10 * 60);

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
        plan_operation("p1".into(), 0, "npm", kind, pkg, ver, Some("1.0.0"))
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
        assert!(plan_operation("x".into(), 0, "apt", OpKind::Update, "curl", None, None).is_err());
        assert!(plan(OpKind::Install, "ok", Some("--force")).is_err());
    }
}
