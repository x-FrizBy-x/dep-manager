use super::{first_line, Adapter};
use crate::error::{AppError, Result};
use crate::exec::Runner;
use crate::model::{Package, PackageKind, UpdateInfo};
use serde_json::Value;

/// Global pnpm packages only (not project dependencies).
pub struct Pnpm;

impl Adapter for Pnpm {
    fn id(&self) -> &'static str {
        "pnpm"
    }
    fn display_name(&self) -> &'static str {
        "pnpm (global)"
    }
    fn binary(&self) -> &'static str {
        "pnpm"
    }
    fn supports_updates(&self) -> bool {
        true
    }

    fn version(&self, r: &dyn Runner) -> Result<String> {
        Ok(first_line(&r.run("pnpm", &["--version"])?.ok()?.stdout))
    }

    fn list_installed(&self, r: &dyn Runner) -> Result<Vec<Package>> {
        let out = r.run("pnpm", &["ls", "-g", "--depth=0", "--json"])?.ok()?;
        parse_ls(&out.stdout)
    }

    fn check_updates(&self, r: &dyn Runner) -> Result<Vec<UpdateInfo>> {
        let out = r.run("pnpm", &["outdated", "-g", "--format", "json"])?;
        if !matches!(out.status, Some(0 | 1)) {
            return out.ok().map(|_| vec![]);
        }
        parse_outdated(&out.stdout)
    }
}

/// pnpm prints an array of workspace roots; for `-g` there is one, whose
/// `dependencies` map holds the global packages.
pub(crate) fn parse_ls(stdout: &str) -> Result<Vec<Package>> {
    if stdout.trim().is_empty() {
        return Ok(vec![]);
    }
    let v: Value = serde_json::from_str(stdout)?;
    let roots = v.as_array().ok_or_else(|| AppError::Parse("pnpm ls: expected array".into()))?;
    let mut pkgs = Vec::new();
    for root in roots {
        for (name, e) in root.get("dependencies").and_then(Value::as_object).into_iter().flatten() {
            let mut p = Package::new("pnpm", format!("pnpm:{name}"), name.as_str());
            p.version = e.get("version").and_then(Value::as_str).map(String::from);
            p.kind = PackageKind::Library;
            p.installation_path = e.get("path").and_then(Value::as_str).map(String::from);
            p.metadata.insert("scope".into(), "global".into());
            pkgs.push(p);
        }
    }
    Ok(pkgs)
}

pub(crate) fn parse_outdated(stdout: &str) -> Result<Vec<UpdateInfo>> {
    if stdout.trim().is_empty() {
        return Ok(vec![]);
    }
    let v: Value = serde_json::from_str(stdout)?;
    Ok(v.as_object()
        .into_iter()
        .flatten()
        .filter_map(|(k, e)| Some(UpdateInfo { name: k.clone(), latest_version: e.get("latest")?.as_str()?.into() }))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ls() {
        let j = r#"[{"path":"/home/u/.local/share/pnpm/global/5","dependencies":{"turbo":{"from":"turbo","version":"2.0.1","path":"/x/turbo"}}}]"#;
        let p = parse_ls(j).unwrap();
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].version.as_deref(), Some("2.0.1"));
        assert_eq!(p[0].installation_path.as_deref(), Some("/x/turbo"));
        assert!(parse_ls("").unwrap().is_empty());
    }

    #[test]
    fn parses_outdated() {
        let u = parse_outdated(r#"{"turbo":{"current":"2.0.1","latest":"2.1.0","wanted":"2.0.1"}}"#).unwrap();
        assert_eq!(u[0].name, "turbo");
    }
}
