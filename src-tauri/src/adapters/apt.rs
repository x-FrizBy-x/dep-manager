use super::{first_line, non_empty, Adapter};
use crate::error::Result;
use crate::exec::Runner;
use crate::model::{Package, PackageKind, UpdateInfo};
use std::collections::HashSet;

pub struct Apt;

const FMT: &str =
    "${db:Status-Abbrev}\t${Package}\t${Version}\t${Architecture}\t${Installed-Size}\t${Homepage}\t${binary:Summary}\n";

impl Adapter for Apt {
    fn id(&self) -> &'static str {
        "apt"
    }
    fn display_name(&self) -> &'static str {
        "APT"
    }
    fn binary(&self) -> &'static str {
        "dpkg-query"
    }
    fn supports_updates(&self) -> bool {
        true
    }

    fn version(&self, r: &dyn Runner) -> Result<String> {
        let out = r.run("apt", &["--version"])?.ok()?;
        Ok(first_line(&out.stdout))
    }

    fn list_installed(&self, r: &dyn Runner) -> Result<Vec<Package>> {
        let out = r.run("dpkg-query", &["-W", "-f", FMT])?.ok()?;
        // Manual-vs-automatic is best-effort; absence is not an error.
        let manual: HashSet<String> = r
            .run("apt-mark", &["showmanual"])
            .ok()
            .filter(|o| o.success())
            .map(|o| o.stdout.lines().map(|l| l.trim().to_string()).collect())
            .unwrap_or_default();
        let have_manual = !manual.is_empty();
        Ok(parse_dpkg(&out.stdout, &manual, have_manual))
    }

    fn check_updates(&self, r: &dyn Runner) -> Result<Vec<UpdateInfo>> {
        let out = r.run("apt", &["list", "--upgradable"])?.ok()?;
        Ok(parse_upgradable(&out.stdout))
    }
}

pub(crate) fn parse_dpkg(stdout: &str, manual: &HashSet<String>, have_manual: bool) -> Vec<Package> {
    let mut pkgs = Vec::new();
    for line in stdout.lines() {
        let f: Vec<&str> = line.splitn(7, '\t').collect();
        if f.len() < 7 || !f[0].starts_with("ii") {
            continue; // only fully installed packages
        }
        let (name, arch) = (f[1], f[3]);
        let mut p = Package::new("apt", format!("apt:{name}:{arch}"), name);
        p.version = non_empty(f[2]);
        p.architecture = non_empty(arch);
        p.homepage = non_empty(f[5]);
        p.description = non_empty(f[6]);
        p.kind = PackageKind::System;
        p.installation_source = Some("dpkg".into());
        if let Ok(kb) = f[4].trim().parse::<u64>() {
            p.metadata.insert("installedSizeKb".into(), kb.to_string());
        }
        if have_manual {
            let reason = if manual.contains(name) { "manual" } else { "automatic" };
            p.metadata.insert("installReason".into(), reason.into());
        }
        pkgs.push(p);
    }
    pkgs
}

/// `bash/jammy-updates 5.1-6ubuntu1.1 amd64 [upgradable from: 5.1-6ubuntu1]`
pub(crate) fn parse_upgradable(stdout: &str) -> Vec<UpdateInfo> {
    stdout
        .lines()
        .filter(|l| l.contains("[upgradable from"))
        .filter_map(|l| {
            let name = l.split_whitespace().next()?.split('/').next()?;
            let latest = l.split_whitespace().nth(1)?;
            Some(UpdateInfo { name: name.to_string(), latest_version: latest.to_string() })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exec::fake::FakeRunner;

    #[test]
    fn parses_installed_and_skips_removed() {
        let out = "ii \tgit\t1:2.43.0-1ubuntu7\tamd64\t21500\thttps://git-scm.com/\tfast, scalable VCS\n\
                   rc \told\t1.0\tamd64\t10\t\tconfig only\n\
                   ii \tlibc6\t2.39-0ubuntu8\ti386\t12000\t\tGNU C Library\n";
        let manual: HashSet<String> = ["git".to_string()].into();
        let p = parse_dpkg(out, &manual, true);
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].id, "apt:git:amd64");
        assert_eq!(p[0].version.as_deref(), Some("1:2.43.0-1ubuntu7"));
        assert_eq!(p[0].homepage.as_deref(), Some("https://git-scm.com/"));
        assert_eq!(p[0].metadata["installReason"], "manual");
        assert_eq!(p[1].homepage, None); // missing -> null, not invented
        assert_eq!(p[1].metadata["installReason"], "automatic");
    }

    #[test]
    fn parses_upgradable() {
        let out = "Listing...\nbash/jammy-updates 5.1-6ubuntu1.1 amd64 [upgradable from: 5.1-6ubuntu1]\n\
                   curl/jammy-security,now 7.81.0-1ubuntu1.18 amd64 [upgradable from: 7.81.0-1ubuntu1.15]\n";
        let u = parse_upgradable(out);
        assert_eq!(u.len(), 2);
        assert_eq!(u[0], UpdateInfo { name: "bash".into(), latest_version: "5.1-6ubuntu1.1".into() });
        assert_eq!(u[1].name, "curl");
    }

    #[test]
    fn version_via_runner() {
        let r = FakeRunner::new(&["apt"]).on("apt --version", 0, "apt 2.7.14 (amd64)\n");
        assert_eq!(Apt.version(&r).unwrap(), "apt 2.7.14 (amd64)");
    }
}
