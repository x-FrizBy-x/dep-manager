use super::{non_empty, Adapter};
use crate::error::Result;
use crate::exec::Runner;
use crate::model::{Package, PackageKind, UpdateInfo};

pub struct Snap;

impl Adapter for Snap {
    fn id(&self) -> &'static str {
        "snap"
    }
    fn display_name(&self) -> &'static str {
        "Snap"
    }
    fn binary(&self) -> &'static str {
        "snap"
    }
    fn supports_updates(&self) -> bool {
        true
    }

    fn version(&self, r: &dyn Runner) -> Result<String> {
        let out = r.run("snap", &["--version"])?.ok()?;
        Ok(out.stdout.lines().next().unwrap_or("").trim().to_string())
    }

    fn list_installed(&self, r: &dyn Runner) -> Result<Vec<Package>> {
        let out = r.run("snap", &["list"])?.ok()?;
        Ok(parse_list(&out.stdout))
    }

    fn check_updates(&self, r: &dyn Runner) -> Result<Vec<UpdateInfo>> {
        // Prints "All snaps up to date." on stderr (exit 0) when nothing to do.
        let out = r.run("snap", &["refresh", "--list"])?.ok()?;
        Ok(parse_refresh(&out.stdout))
    }
}

/// `Name Version Rev Tracking Publisher Notes`
pub(crate) fn parse_list(stdout: &str) -> Vec<Package> {
    stdout
        .lines()
        .skip(1)
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            if f.len() < 5 {
                return None;
            }
            let name = f[0];
            let notes = f.get(5).copied().unwrap_or("-");
            let mut p = Package::new("snap", format!("snap:{name}"), name);
            p.version = non_empty(f[1]);
            p.installation_source = non_empty(f[3]).map(|t| format!("channel {t}"));
            p.kind = if matches!(name, "core" | "bare" | "snapd") || notes.contains("base") || notes.contains("snapd") {
                PackageKind::Runtime
            } else {
                PackageKind::Application
            };
            p.metadata.insert("revision".into(), f[2].to_string());
            p.metadata.insert("publisher".into(), f[4].to_string());
            if notes != "-" {
                p.metadata.insert("notes".into(), notes.to_string());
            }
            Some(p)
        })
        .collect()
}

/// `Name Version Rev Size Publisher Notes`
pub(crate) fn parse_refresh(stdout: &str) -> Vec<UpdateInfo> {
    stdout
        .lines()
        .skip(1)
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            (f.len() >= 3).then(|| UpdateInfo { name: f[0].into(), latest_version: f[1].into() })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_list() {
        let out = "Name     Version  Rev   Tracking       Publisher   Notes\n\
                   core22   20240111 1380  latest/stable  canonical   base\n\
                   firefox  130.0    4336  latest/stable  mozilla     -\n";
        let p = parse_list(out);
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].kind, PackageKind::Runtime);
        assert_eq!(p[1].kind, PackageKind::Application);
        assert_eq!(p[1].version.as_deref(), Some("130.0"));
        assert!(!p[1].metadata.contains_key("notes"));
    }

    #[test]
    fn parses_refresh() {
        let out = "Name     Version  Rev   Size  Publisher  Notes\nfirefox  131.0  4400  250MB  mozilla  -\n";
        assert_eq!(parse_refresh(out), vec![UpdateInfo { name: "firefox".into(), latest_version: "131.0".into() }]);
        assert!(parse_refresh("").is_empty());
    }
}
