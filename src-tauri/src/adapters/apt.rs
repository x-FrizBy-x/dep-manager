use super::{first_line, non_empty, Adapter};
use crate::error::Result;
use crate::exec::Runner;
use crate::model::{DependencyEdge, Package, PackageDependencies, PackageDetails, PackageKind, SearchHit, UpdateInfo};
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

    fn supports_dependencies(&self) -> bool {
        true
    }

    fn dependencies(&self, r: &dyn Runner, name: &str) -> Result<PackageDependencies> {
        validate_apt_name(name)?;
        // Only hard dependencies (Depends / Pre-Depends) among INSTALLED packages.
        const HARD: [&str; 6] =
            ["--installed", "--no-recommends", "--no-suggests", "--no-conflicts", "--no-breaks", "--no-replaces"];
        let mut fwd: Vec<&str> = vec!["depends", "--recurse", "--no-enhances"];
        fwd.extend(HARD);
        fwd.push(name);
        let closure = r.run("apt-cache", &fwd)?;
        // An unknown or uninstalled package prints nothing on stdout.
        if closure.stdout.trim().is_empty() {
            return Err(crate::error::AppError::Other(format!("`{name}` is not an installed apt package")));
        }

        let mut rev: Vec<&str> = vec!["rdepends", "--no-enhances"];
        rev.extend(HARD);
        rev.push(name);
        let hard_rdeps = parse_rdepends(&r.run("apt-cache", &rev)?.stdout, name);

        // Recommends also keep an automatic package from being auto-removed.
        let rev_all = [
            "rdepends", "--installed", "--no-suggests", "--no-conflicts", "--no-breaks", "--no-replaces", "--no-enhances", name,
        ];
        let all_rdeps = parse_rdepends(&r.run("apt-cache", &rev_all)?.stdout, name);

        let auto = r.run("apt-mark", &["showauto"])?.ok()?.stdout.lines().any(|l| l.trim() == name);
        build_dependencies(name, &closure.stdout, &hard_rdeps, &all_rdeps, auto)
    }
}

/// One stanza of `apt-cache depends --recurse`: a package and its dependency lines.
pub(crate) struct DepStanza {
    pub name: String,
    pub deps: Vec<DependencyEdge>,
}

/// Parses `apt-cache depends` output. Package names start in column 0;
/// `  Depends: x` lines are dependencies, `  |Depends: x` are alternatives,
/// `<x>` is a virtual package, and lines indented further list its providers.
pub(crate) fn parse_depends(stdout: &str) -> Vec<DepStanza> {
    let mut out: Vec<DepStanza> = Vec::new();
    for line in stdout.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if !line.starts_with(' ') {
            out.push(DepStanza { name: line.trim().to_string(), deps: vec![] });
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        let Some(cur) = out.last_mut() else { continue };
        if indent > 2 {
            continue; // provider of a virtual package
        }
        let t = line.trim();
        let (alt, t) = t.strip_prefix('|').map(|r| (true, r)).unwrap_or((false, t));
        let Some((kind, name)) = t.split_once(": ") else { continue };
        let name = name.trim();
        let virt = name.starts_with('<');
        let mut kind = kind.trim().to_string();
        if alt {
            kind.push_str(" (alternative)");
        }
        if virt {
            kind.push_str(" (virtual)");
        }
        cur.deps.push(DependencyEdge {
            name: name.trim_matches(|c| c == '<' || c == '>').to_string(),
            version: None,
            kind,
            installed: true,
        });
    }
    out
}

/// `apt-cache rdepends`: header lines, then one package per line (repeated per
/// relation, `|` marks alternatives). Returns each name once.
pub(crate) fn parse_rdepends(stdout: &str, subject: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for line in stdout.lines().filter(|l| l.starts_with(' ')) {
        let n = line.trim().trim_start_matches('|').trim();
        if !n.is_empty() && n != subject && !names.iter().any(|x| x == n) {
            names.push(n.to_string());
        }
    }
    names
}

pub(crate) fn build_dependencies(
    name: &str,
    closure: &str,
    hard_rdeps: &[String],
    all_rdeps: &[String],
    auto: bool,
) -> Result<PackageDependencies> {
    let stanzas = parse_depends(closure);
    let first = stanzas
        .iter()
        .find(|s| s.name == name)
        .ok_or_else(|| crate::error::AppError::Other(format!("`{name}` is not an installed apt package")))?;
    let depends_on = first.deps.clone();
    let direct: HashSet<&str> = depends_on.iter().map(|e| e.name.as_str()).collect();
    let mut transitive: Vec<DependencyEdge> = stanzas
        .iter()
        .filter(|s| s.name != name && !direct.contains(s.name.as_str()))
        .map(|s| DependencyEdge { name: s.name.clone(), version: None, kind: "transitive".into(), installed: true })
        .collect();
    transitive.sort_by(|a, b| a.name.cmp(&b.name));
    let required_by = hard_rdeps
        .iter()
        .map(|n| DependencyEdge { name: n.clone(), version: None, kind: "Depends".into(), installed: true })
        .collect();

    let orphan_hint = (auto && all_rdeps.is_empty()).then(|| {
        "Inferred: installed automatically and no installed package depends on or recommends it, so `apt autoremove` would likely offer to remove it. UniManage never removes anything on its own.".to_string()
    });
    Ok(PackageDependencies {
        manager_id: "apt".into(),
        name: name.to_string(),
        version: None,
        install_reason: Some(
            if auto { "Installed automatically (as a dependency)" } else { "Installed manually" }.into(),
        ),
        depends_on,
        transitive,
        required_by,
        orphan_hint,
        notes: vec![
            "Only Depends and Pre-Depends between installed packages are shown; Recommends and Suggests are not treated as dependencies.".into(),
            "\"alternative\" means any one of several packages satisfies the dependency; \"virtual\" means a capability provided by some installed package.".into(),
        ],
    })
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

    /// Shapes captured from `apt-cache` on Ubuntu 26.04 (WSL).
    const CURL_CLOSURE: &str = "curl\n  Depends: libcurl4t64\n  Depends: libc6\n  Depends: zlib1g\nlibcurl4t64\n  Depends: libbrotli1\n  Depends: libc6\n  Depends: zlib1g\nlibc6\n  Depends: libgcc-s1\nzlib1g\n  Depends: libc6\nlibbrotli1\n  Depends: libc6\nlibgcc-s1\n  Depends: libc6\n";

    #[test]
    fn dependencies_direct_transitive_and_reverse() {
        let rdeps = parse_rdepends("libcurl4t64\nReverse Depends:\n  curl\n  curl\n  |wget\n  libcurl4t64\n", "libcurl4t64");
        assert_eq!(rdeps, vec!["curl", "wget"]);
        let d = build_dependencies("curl", CURL_CLOSURE, &[], &[], false).unwrap();
        assert_eq!(d.depends_on.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), vec!["libcurl4t64", "libc6", "zlib1g"]);
        let t: Vec<_> = d.transitive.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(t, vec!["libbrotli1", "libgcc-s1"]);
        assert_eq!(d.install_reason.as_deref(), Some("Installed manually"));
        assert!(d.orphan_hint.is_none());
    }

    #[test]
    fn orphan_hint_only_for_auto_without_reverse_deps() {
        let c = "foo\n  Depends: libc6\n";
        assert!(build_dependencies("foo", c, &[], &[], true).unwrap().orphan_hint.is_some());
        assert!(build_dependencies("foo", c, &[], &["bar".into()], true).unwrap().orphan_hint.is_none());
        assert!(build_dependencies("foo", c, &[], &[], false).unwrap().orphan_hint.is_none());
        assert!(build_dependencies("foo", "", &[], &[], true).is_err());
    }

    #[test]
    fn depends_parser_handles_alternatives_virtuals_and_providers() {
        let s = parse_depends("pkg\n  PreDepends: a\n  |Depends: b\n  Depends: c\n  Depends: <mail-transport-agent>\n    postfix\n    exim4\n  Depends: d\n");
        assert_eq!(s.len(), 1);
        let k: Vec<_> = s[0].deps.iter().map(|e| (e.name.as_str(), e.kind.as_str())).collect();
        assert_eq!(
            k,
            vec![
                ("a", "PreDepends"),
                ("b", "Depends (alternative)"),
                ("c", "Depends"),
                ("mail-transport-agent", "Depends (virtual)"),
                ("d", "Depends"),
            ]
        );
    }

    #[test]
    fn dependencies_validate_name() {
        let r = FakeRunner::new(&["apt-cache"]);
        assert!(Apt.dependencies(&r, "bad name; rm").is_err());
        assert!(Apt.dependencies(&r, "--help").is_err());
    }

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
