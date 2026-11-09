use super::{first_line, Adapter};
use crate::error::{AppError, Result};
use crate::exec::Runner;
use crate::model::{Package, PackageKind, UpdateInfo};
use serde_json::Value;

/// Global npm packages only. Project-level dependencies are deliberately
/// out of scope and must never be mixed into this adapter.
pub struct Npm;

impl Adapter for Npm {
    fn id(&self) -> &'static str {
        "npm"
    }
    fn display_name(&self) -> &'static str {
        "npm (global)"
    }
    fn binary(&self) -> &'static str {
        "npm"
    }
    fn supports_updates(&self) -> bool {
        true
    }

    fn version(&self, r: &dyn Runner) -> Result<String> {
        Ok(first_line(&r.run("npm", &["--version"])?.ok()?.stdout))
    }

    fn list_installed(&self, r: &dyn Runner) -> Result<Vec<Package>> {
        // npm exits non-zero for e.g. extraneous/invalid trees yet still prints
        // valid JSON, so exit status is not checked here.
        let out = r.run("npm", &["ls", "-g", "--depth=0", "--json"])?;
        let root = r.run("npm", &["root", "-g"]).ok().filter(|o| o.success()).map(|o| first_line(&o.stdout));
        parse_ls(&out.stdout, root.as_deref())
    }

    fn check_updates(&self, r: &dyn Runner) -> Result<Vec<UpdateInfo>> {
        // Exit code 1 simply means "something is outdated".
        let out = r.run("npm", &["outdated", "-g", "--json"])?;
        if !matches!(out.status, Some(0 | 1)) {
            return out.ok().map(|_| vec![]);
        }
        parse_outdated(&out.stdout)
    }
}

pub(crate) fn parse_ls(stdout: &str, root: Option<&str>) -> Result<Vec<Package>> {
    let v: Value = serde_json::from_str(stdout)?;
    let deps = match v.get("dependencies").and_then(Value::as_object) {
        Some(d) => d,
        None => return Ok(vec![]),
    };
    Ok(deps
        .iter()
        .map(|(name, e)| {
            let mut p = Package::new("npm", format!("npm:{name}"), name.as_str());
            p.version = e.get("version").and_then(Value::as_str).map(String::from);
            p.kind = PackageKind::Library;
            p.installation_path = root.map(|r| format!("{r}/{name}"));
            p.installation_source = e.get("resolved").and_then(Value::as_str).map(String::from);
            p.metadata.insert("scope".into(), "global".into());
            p
        })
        .collect())
}

/// `{ "typescript": { "current": "5.4.0", "wanted": "5.4.5", "latest": "5.5.0" } }`
pub(crate) fn parse_outdated(stdout: &str) -> Result<Vec<UpdateInfo>> {
    if stdout.trim().is_empty() {
        return Ok(vec![]);
    }
    let v: Value = serde_json::from_str(stdout)?;
    let obj = v.as_object().ok_or_else(|| AppError::Parse("npm outdated: expected object".into()))?;
    Ok(obj
        .iter()
        .filter_map(|(k, e)| {
            Some(UpdateInfo { name: k.clone(), latest_version: e.get("latest")?.as_str()?.into() })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ls() {
        let j = r#"{"name":"lib","dependencies":{"typescript":{"version":"5.4.0","resolved":"https://registry.npmjs.org/typescript/-/typescript-5.4.0.tgz"},"npm":{"version":"10.5.0"}}}"#;
        let p = parse_ls(j, Some("/usr/lib/node_modules")).unwrap();
        assert_eq!(p.len(), 2);
        let ts = p.iter().find(|p| p.name == "typescript").unwrap();
        assert_eq!(ts.installation_path.as_deref(), Some("/usr/lib/node_modules/typescript"));
        assert_eq!(ts.metadata["scope"], "global");
    }

    /// Real output captured from npm 12.1 on Windows 11.
    #[test]
    fn parses_real_npm12_output() {
        let ls = r#"{"name":"npm","dependencies":{"@qwen-code/qwen-code":{"version":"0.24.0","overridden":false},"npm":{"version":"12.1.0","overridden":false}}}"#;
        let p = parse_ls(ls, None).unwrap();
        assert_eq!(p.len(), 2);
        assert!(p.iter().any(|p| p.name == "@qwen-code/qwen-code" && p.version.as_deref() == Some("0.24.0")));
        let out = r#"{"npm":{"current":"12.1.0","wanted":"12.2.2","latest":"12.2.2","dependent":"global","location":"C:\\Users\\x\\npm"}}"#;
        let u = parse_outdated(out).unwrap();
        assert_eq!(u[0].latest_version, "12.2.2");
    }

    #[test]
    fn empty_tree_is_ok() {
        assert!(parse_ls("{}", None).unwrap().is_empty());
    }

    #[test]
    fn parses_outdated() {
        let u = parse_outdated(r#"{"typescript":{"current":"5.4.0","wanted":"5.4.5","latest":"5.5.0"}}"#).unwrap();
        assert_eq!(u[0].latest_version, "5.5.0");
    }
}
