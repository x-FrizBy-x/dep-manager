use super::{first_line, non_empty, Adapter};
use crate::error::{AppError, Result};
use crate::exec::Runner;
use crate::model::{Package, PackageKind, UpdateInfo};
use serde_json::Value;

pub struct Mise;

impl Adapter for Mise {
    fn id(&self) -> &'static str {
        "mise"
    }
    fn display_name(&self) -> &'static str {
        "mise"
    }
    fn binary(&self) -> &'static str {
        "mise"
    }
    fn supports_updates(&self) -> bool {
        true
    }

    fn version(&self, r: &dyn Runner) -> Result<String> {
        Ok(first_line(&r.run("mise", &["--version"])?.ok()?.stdout))
    }

    fn list_installed(&self, r: &dyn Runner) -> Result<Vec<Package>> {
        let out = r.run("mise", &["ls", "--json"])?.ok()?;
        parse_ls(&out.stdout)
    }

    fn check_updates(&self, r: &dyn Runner) -> Result<Vec<UpdateInfo>> {
        let out = r.run("mise", &["outdated", "--json"])?.ok()?;
        parse_outdated(&out.stdout)
    }
}

/// `{ "node": [ { "version": "22.1.0", "install_path": "...", "source": {...}, "installed": true, "active": true } ] }`
pub(crate) fn parse_ls(stdout: &str) -> Result<Vec<Package>> {
    let v: Value = serde_json::from_str(stdout)?;
    let obj = v.as_object().ok_or_else(|| AppError::Parse("mise ls: expected object".into()))?;
    let mut pkgs = Vec::new();
    for (tool, entries) in obj {
        for e in entries.as_array().into_iter().flatten() {
            // Versions listed in config but not installed are not installed software.
            if e.get("installed").and_then(Value::as_bool) == Some(false) {
                continue;
            }
            let version = e.get("version").and_then(Value::as_str).unwrap_or("");
            let mut p = Package::new("mise", format!("mise:{tool}@{version}"), tool.as_str());
            p.version = non_empty(version);
            p.kind = PackageKind::Runtime;
            p.installation_path = e.get("install_path").and_then(Value::as_str).map(String::from);
            p.installation_source = e
                .get("source")
                .and_then(|s| s.get("path"))
                .and_then(Value::as_str)
                .map(|s| format!("config {s}"));
            if let Some(a) = e.get("active").and_then(Value::as_bool) {
                p.metadata.insert("active".into(), a.to_string());
            }
            if let Some(r) = e.get("requested_version").and_then(Value::as_str) {
                p.metadata.insert("requestedVersion".into(), r.into());
            }
            pkgs.push(p);
        }
    }
    Ok(pkgs)
}

/// `{ "node": { "name": "node", "current": "22.1.0", "latest": "22.5.0", ... } }`
pub(crate) fn parse_outdated(stdout: &str) -> Result<Vec<UpdateInfo>> {
    if stdout.trim().is_empty() {
        return Ok(vec![]);
    }
    let v: Value = serde_json::from_str(stdout)?;
    Ok(v.as_object()
        .into_iter()
        .flatten()
        .filter_map(|(k, e)| {
            let latest = e.get("latest").and_then(Value::as_str)?;
            Some(UpdateInfo { name: k.clone(), latest_version: latest.into() })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ls_skipping_uninstalled() {
        let j = r#"{
          "node": [
            {"version":"22.1.0","requested_version":"22","install_path":"/home/u/.local/share/mise/installs/node/22.1.0",
             "source":{"type":"mise.toml","path":"/home/u/.config/mise/config.toml"},"installed":true,"active":true},
            {"version":"18.0.0","installed":false,"active":false}
          ],
          "python": [{"version":"3.12.2","installed":true,"active":false}]
        }"#;
        let p = parse_ls(j).unwrap();
        assert_eq!(p.len(), 2);
        let node = p.iter().find(|p| p.name == "node").unwrap();
        assert_eq!(node.version.as_deref(), Some("22.1.0"));
        assert_eq!(node.metadata["active"], "true");
        assert!(node.installation_path.is_some());
        let py = p.iter().find(|p| p.name == "python").unwrap();
        assert_eq!(py.installation_path, None);
    }

    #[test]
    fn parses_outdated() {
        let u = parse_outdated(r#"{"node":{"name":"node","current":"22.1.0","latest":"22.5.0"}}"#).unwrap();
        assert_eq!(u, vec![UpdateInfo { name: "node".into(), latest_version: "22.5.0".into() }]);
        assert!(parse_outdated("").unwrap().is_empty());
        assert!(parse_outdated("{}").unwrap().is_empty());
    }
}
