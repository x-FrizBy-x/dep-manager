use super::{first_line, Adapter};
use crate::error::Result;
use crate::exec::Runner;
use crate::model::{Package, PackageKind};

/// uv tools. Update checks are not implemented (flag support varies by uv
/// version), so update status stays `unknown`.
pub struct Uv;

impl Adapter for Uv {
    fn id(&self) -> &'static str {
        "uv"
    }
    fn display_name(&self) -> &'static str {
        "uv tools"
    }
    fn binary(&self) -> &'static str {
        "uv"
    }

    fn version(&self, r: &dyn Runner) -> Result<String> {
        Ok(first_line(&r.run("uv", &["--version"])?.ok()?.stdout))
    }

    fn list_installed(&self, r: &dyn Runner) -> Result<Vec<Package>> {
        let out = r.run("uv", &["tool", "list"])?;
        // "No tools installed" is reported with exit 0 on stderr; non-zero is a real failure.
        Ok(parse_list(&out.ok()?.stdout))
    }
}

/// ```text
/// ruff v0.4.1
/// - ruff
/// black v24.3.0
/// - black
/// - blackd
/// ```
pub(crate) fn parse_list(stdout: &str) -> Vec<Package> {
    let mut pkgs: Vec<Package> = Vec::new();
    for line in stdout.lines() {
        let l = line.trim();
        if l.is_empty() {
            continue;
        }
        if let Some(exe) = l.strip_prefix("- ") {
            if let Some(last) = pkgs.last_mut() {
                let e = last.metadata.entry("executables".into()).or_default();
                if !e.is_empty() {
                    e.push_str(", ");
                }
                e.push_str(exe.trim());
            }
            continue;
        }
        let mut it = l.split_whitespace();
        let (Some(name), Some(ver)) = (it.next(), it.next()) else { continue };
        let mut p = Package::new("uv", format!("uv:{name}"), name);
        p.version = Some(ver.trim_start_matches('v').to_string());
        p.kind = PackageKind::Application;
        pkgs.push(p);
    }
    pkgs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tools_and_executables() {
        let p = parse_list("ruff v0.4.1\n- ruff\nblack v24.3.0\n- black\n- blackd\n");
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].version.as_deref(), Some("0.4.1"));
        assert_eq!(p[1].metadata["executables"], "black, blackd");
    }

    #[test]
    fn empty_output() {
        assert!(parse_list("").is_empty());
    }
}
