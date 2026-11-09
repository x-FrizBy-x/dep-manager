use super::{first_line, non_empty, Adapter};
use crate::error::Result;
use crate::exec::Runner;
use crate::model::{Package, PackageKind, UpdateInfo};

pub struct Flatpak;

const COLS: &str = "--columns=application,name,version,branch,origin,installation,arch";

impl Adapter for Flatpak {
    fn id(&self) -> &'static str {
        "flatpak"
    }
    fn display_name(&self) -> &'static str {
        "Flatpak"
    }
    fn binary(&self) -> &'static str {
        "flatpak"
    }
    fn supports_updates(&self) -> bool {
        true
    }

    fn version(&self, r: &dyn Runner) -> Result<String> {
        Ok(first_line(&r.run("flatpak", &["--version"])?.ok()?.stdout))
    }

    fn list_installed(&self, r: &dyn Runner) -> Result<Vec<Package>> {
        let apps = r.run("flatpak", &["list", "--app", COLS])?.ok()?;
        let rts = r.run("flatpak", &["list", "--runtime", COLS])?.ok()?;
        let mut v = parse_list(&apps.stdout, PackageKind::Application);
        v.extend(parse_list(&rts.stdout, PackageKind::Runtime));
        Ok(v)
    }

    fn check_updates(&self, r: &dyn Runner) -> Result<Vec<UpdateInfo>> {
        let out = r
            .run("flatpak", &["remote-ls", "--updates", "--columns=application,version,branch"])?
            .ok()?;
        Ok(parse_updates(&out.stdout))
    }
}

pub(crate) fn parse_updates(stdout: &str) -> Vec<UpdateInfo> {
    stdout
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            let name = f.first()?.trim();
            if name.is_empty() {
                return None;
            }
            // Flatpak may not expose a version; fall back to the branch.
            let v = f.get(1).and_then(|s| non_empty(s)).or_else(|| f.get(2).and_then(|s| non_empty(s)))?;
            Some(UpdateInfo { name: name.to_string(), latest_version: v })
        })
        .collect()
}

pub(crate) fn parse_list(stdout: &str, kind: PackageKind) -> Vec<Package> {
    stdout
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            if f.len() < 7 || f[0].trim().is_empty() {
                return None;
            }
            let (app, branch, arch) = (f[0].trim(), f[3].trim(), f[6].trim());
            let mut p = Package::new("flatpak", format!("flatpak:{app}/{arch}/{branch}"), app);
            p.display_name = non_empty(f[1]);
            p.version = non_empty(f[2]).or_else(|| non_empty(branch));
            p.installation_source = non_empty(f[4]);
            p.architecture = non_empty(arch);
            p.kind = kind;
            p.metadata.insert("branch".into(), branch.to_string());
            p.metadata.insert("installation".into(), f[5].trim().to_string());
            Some(p)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_apps() {
        let out = "org.mozilla.firefox\tFirefox\t130.0\tstable\tflathub\tsystem\tx86_64\n\
                   org.gnome.Platform\tGNOME Platform\t\t46\tflathub\tsystem\tx86_64\n";
        let p = parse_list(out, PackageKind::Application);
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].display_name.as_deref(), Some("Firefox"));
        assert_eq!(p[0].version.as_deref(), Some("130.0"));
        assert_eq!(p[1].version.as_deref(), Some("46")); // falls back to branch
        assert_eq!(p[0].id, "flatpak:org.mozilla.firefox/x86_64/stable");
    }

    #[test]
    fn parses_updates_with_branch_fallback() {
        let u = parse_updates("org.mozilla.firefox\t131.0\tstable\norg.gnome.Platform\t\t46\n");
        assert_eq!(u[0].latest_version, "131.0");
        assert_eq!(u[1].latest_version, "46");
    }
}
