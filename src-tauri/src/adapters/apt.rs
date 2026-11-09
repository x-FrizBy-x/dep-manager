use super::{first_line, non_empty, Adapter};
use crate::error::Result;
use crate::exec::Runner;
use crate::model::{Package, PackageDetails, PackageKind, SearchHit, UpdateInfo};
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

    fn supports_search(&self) -> bool {
        true
    }

    fn search(&self, r: &dyn Runner, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
        let terms = super::validate_query(query)?;
        let mut args: Vec<&str> = vec!["search", "--names-only"];
        args.extend(terms.iter().map(String::as_str));
        // apt-cache exits 0 with no output when nothing matches.
        let out = r.run("apt-cache", &args)?.ok()?;
        let mut hits = parse_search(&out.stdout);
        hits.truncate(limit.clamp(1, 50));
        Ok(hits)
    }

    fn details(&self, r: &dyn Runner, name: &str) -> Result<PackageDetails> {
        validate_apt_name(name)?;
        let out = r.run("apt-cache", &["show", name])?.ok()?;
        parse_show(&out.stdout)
    }
}

/// Debian package names: lowercase alphanumerics and `+ - .`, starting with an
/// alphanumeric, at least two characters.
pub(crate) fn validate_apt_name(name: &str) -> Result<()> {
    let ok = name.len() >= 2
        && name.len() <= 100
        && name.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '+' | '-' | '.'));
    if ok {
        Ok(())
    } else {
        Err(crate::error::AppError::Other(format!("invalid apt package name `{name}`")))
    }
}

/// `apt-cache search --names-only`: one `name - short description` per line.
pub(crate) fn parse_search(stdout: &str) -> Vec<SearchHit> {
    stdout
        .lines()
        .filter_map(|l| {
            let (name, desc) = l.split_once(" - ").map(|(a, b)| (a, Some(b))).unwrap_or((l.trim(), None));
            let name = name.trim();
            (!name.is_empty()).then(|| SearchHit {
                name: name.to_string(),
                version: None,
                description: desc.map(|d| d.trim().to_string()).filter(|d| !d.is_empty()),
            })
        })
        .collect()
}

/// `apt-cache show <pkg>`: deb822 stanzas separated by blank lines. The first
/// stanza is the candidate version; later ones (e.g. the installed copy) are ignored.
pub(crate) fn parse_show(stdout: &str) -> Result<PackageDetails> {
    let stanza = stdout.split("\n\n").find(|s| !s.trim().is_empty()).unwrap_or("");
    let mut fields: Vec<(String, String)> = Vec::new();
    for line in stanza.lines() {
        if line.starts_with([' ', '\t']) {
            if let Some((_, v)) = fields.last_mut() {
                if line.trim() != "." {
                    v.push(' ');
                    v.push_str(line.trim());
                } else {
                    v.push(' ');
                }
            }
        } else if let Some((k, v)) = line.split_once(':') {
            fields.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    let get = |k: &str| fields.iter().find(|(f, _)| f == k).map(|(_, v)| v.clone()).filter(|v| !v.is_empty());
    let name = get("Package").ok_or_else(|| crate::error::AppError::Parse("apt-cache show: no Package field".into()))?;
    // `Description` or a translated `Description-xx`.
    let description = fields
        .iter()
        .find(|(k, _)| k == "Description" || k.starts_with("Description-") && k != "Description-md5")
        .map(|(_, v)| v.trim().to_string());
    let mut d = PackageDetails {
        manager_id: "apt".into(),
        name,
        version: get("Version"),
        description,
        homepage: get("Homepage"),
        ..Default::default()
    };
    if let Some(dep) = get("Depends") {
        d.dependencies = dep.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect();
    }
    for key in ["Section", "Priority", "Architecture", "Installed-Size"] {
        if let Some(v) = get(key) {
            d.metadata.insert(key.to_lowercase(), v);
        }
    }
    Ok(d)
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

#[cfg(test)]
mod search_tests {
    use super::*;

    /// Real output from Ubuntu (WSL), trimmed.
    const SHOW: &str = "Package: curl\nPriority: optional\nSection: web\nInstalled-Size: 512\nMaintainer: Ubuntu Developers <ubuntu-devel-discuss@lists.ubuntu.com>\nArchitecture: amd64\nVersion: 8.18.0-1ubuntu2.8\nDepends: libcurl4t64 (= 8.18.0-1ubuntu2.8), libc6 (>= 2.34), zlib1g (>= 1:1.1.4)\nHomepage: https://curl.se/\nDescription-en: command line tool for transferring data with URL syntax\n curl is a command line tool for transferring data.\n .\n curl supports SSL certificates.\nDescription-md5: f83293d1\n\nPackage: curl\nStatus: install ok installed\nVersion: 1.0\n";

    #[test]
    fn parses_search_lines() {
        let h = parse_search("curl - command line tool for transferring data with URL syntax\ncurlftpfs - filesystem to access FTP hosts based on FUSE and cURL\n");
        assert_eq!(h.len(), 2);
        assert_eq!(h[0].name, "curl");
        assert!(h[1].description.as_deref().unwrap().starts_with("filesystem"));
        assert!(parse_search("").is_empty());
    }

    #[test]
    fn parses_show_first_stanza() {
        let d = parse_show(SHOW).unwrap();
        assert_eq!((d.name.as_str(), d.version.as_deref()), ("curl", Some("8.18.0-1ubuntu2.8")));
        assert_eq!(d.homepage.as_deref(), Some("https://curl.se/"));
        assert_eq!(d.dependencies.len(), 3);
        assert!(d.description.as_deref().unwrap().starts_with("command line tool"));
        assert!(d.description.as_deref().unwrap().contains("SSL certificates"));
        assert_eq!(d.metadata["section"], "web");
        assert!(!d.metadata.contains_key("maintainer"));
        assert!(parse_show("").is_err());
    }

    #[test]
    fn validates_names_and_queries() {
        for ok in ["curl", "g++", "libc6", "python3.12"] {
            assert!(validate_apt_name(ok).is_ok(), "{ok}");
        }
        for bad in ["", "a", "-x", "Curl", "a b", "a;b", "curl:amd64", "../x"] {
            assert!(validate_apt_name(bad).is_err(), "{bad}");
        }
        use crate::exec::fake::FakeRunner;
        let r = FakeRunner::new(&["apt-cache"]);
        assert!(Apt.search(&r, "--version", 5).is_err());
        let r = FakeRunner::new(&["apt-cache"]).on("apt-cache search --names-only curl", 0, "curl - x\n");
        assert_eq!(Apt.search(&r, "curl", 5).unwrap().len(), 1);
    }
}
