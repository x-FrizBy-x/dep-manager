use super::{first_line, Adapter};
use crate::error::{AppError, Result};
use crate::exec::Runner;
use crate::model::{Package, PackageKind};
use serde_json::Value;

/// pipx has no native "outdated" query, so this adapter does not implement
/// `check_updates`; update status stays `unknown` rather than being guessed.
pub struct Pipx;

impl Adapter for Pipx {
    fn id(&self) -> &'static str {
        "pipx"
    }
    fn display_name(&self) -> &'static str {
        "pipx"
    }
    fn binary(&self) -> &'static str {
        "pipx"
    }

    fn version(&self, r: &dyn Runner) -> Result<String> {
        Ok(first_line(&r.run("pipx", &["--version"])?.ok()?.stdout))
    }

    fn list_installed(&self, r: &dyn Runner) -> Result<Vec<Package>> {
        let out = r.run("pipx", &["list", "--json"])?.ok()?;
        parse_list(&out.stdout)
    }
}

pub(crate) fn parse_list(stdout: &str) -> Result<Vec<Package>> {
    let v: Value = serde_json::from_str(stdout)?;
    let venvs = v
        .get("venvs")
        .and_then(Value::as_object)
        .ok_or_else(|| AppError::Parse("pipx list: missing `venvs`".into()))?;
    Ok(venvs
        .iter()
        .map(|(name, e)| {
            let meta = e.get("metadata");
            let main = meta.and_then(|m| m.get("main_package"));
            let mut p = Package::new("pipx", format!("pipx:{name}"), name.as_str());
            p.version = main.and_then(|m| m.get("package_version")).and_then(Value::as_str).map(String::from);
            p.kind = PackageKind::Application;
            p.installation_source = main.and_then(|m| m.get("package_or_url")).and_then(Value::as_str).map(String::from);
            if let Some(py) = meta.and_then(|m| m.get("python_version")).and_then(Value::as_str) {
                p.metadata.insert("python".into(), py.into());
            }
            if let Some(apps) = main.and_then(|m| m.get("apps")).and_then(Value::as_array) {
                let names: Vec<&str> = apps.iter().filter_map(Value::as_str).collect();
                p.metadata.insert("apps".into(), names.join(", "));
            }
            p
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_list() {
        let j = r#"{"pipx_spec_version":"0.1","venvs":{"ruff":{"metadata":{"main_package":{"package":"ruff","package_version":"0.4.1","package_or_url":"ruff","apps":["ruff"]},"python_version":"Python 3.12.2"}}}}"#;
        let p = parse_list(j).unwrap();
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].version.as_deref(), Some("0.4.1"));
        assert_eq!(p[0].metadata["apps"], "ruff");
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_list("not json").is_err());
        assert!(parse_list("{}").is_err());
    }
}
