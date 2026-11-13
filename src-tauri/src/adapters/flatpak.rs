use super::{first_line, non_empty, Adapter};
use crate::error::Result;
use crate::exec::Runner;
use crate::model::{Package, PackageDetails, PackageKind, SearchHit, UpdateInfo};

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

    fn supports_search(&self) -> bool {
        true
    }

    fn search(&self, r: &dyn Runner, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
        let terms = super::validate_query(query)?;
        let q = terms.join(" ");
        // Piped output is tab-separated without headers; "No matches found" arrives on stdout with exit 0.
        let out = r.run("flatpak", &["search", "--columns=application,version,name,description", &q])?.ok()?;
        let mut hits = parse_search(&out.stdout);
        hits.truncate(limit.clamp(1, 50));
        Ok(hits)
    }

    fn details(&self, r: &dyn Runner, name: &str) -> Result<PackageDetails> {
        validate_app_id(name)?;
        let remotes = r.run("flatpak", &["remotes", "--columns=name"])?.ok()?;
        let mut last_err = None;
        for remote in remotes.stdout.lines().map(str::trim).filter(|l| validate_remote_name(l)) {
            match r.run("flatpak", &["remote-info", remote, name]) {
                Ok(o) if o.success() => return parse_remote_info(&o.stdout, remote),
                Ok(o) => last_err = Some(o.ok().unwrap_err()),
                Err(e) => last_err = Some(e),
            }
        }
        Err(last_err.unwrap_or_else(|| crate::error::AppError::Other("no flatpak remotes configured".into())))
    }
}

/// Reverse-DNS application / runtime ids: `[A-Za-z0-9_-]` segments joined by dots, at least two segments.
pub(crate) fn validate_app_id(id: &str) -> Result<()> {
    let segs: Vec<&str> = id.split('.').collect();
    let ok = id.len() <= 255
        && segs.len() >= 2
        && segs.iter().all(|s| {
            !s.is_empty() && !s.starts_with('-') && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        });
    if ok {
        Ok(())
    } else {
        Err(crate::error::AppError::Other(format!("invalid flatpak id `{id}`")))
    }
}

pub(crate) fn validate_remote_name(n: &str) -> bool {
    !n.is_empty() && !n.starts_with('-') && n.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

/// `flatpak search --columns=application,version,name,description` (tab-separated).
pub(crate) fn parse_search(stdout: &str) -> Vec<SearchHit> {
    stdout
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            if f.len() < 3 || validate_app_id(f[0].trim()).is_err() {
                return None;
            }
            // Show the human name first, keep the id visible in the description.
            let desc = f.get(3).map(|d| d.trim()).filter(|d| !d.is_empty());
            Some(SearchHit {
                name: f[0].trim().to_string(),
                version: non_empty(f[1]),
                description: Some(match (f[2].trim(), desc) {
                    ("", Some(d)) => d.to_string(),
                    (n, Some(d)) => format!("{n} - {d}"),
                    (n, None) => n.to_string(),
                })
                .filter(|d| !d.is_empty()),
            })
        })
        .collect()
}

/// `flatpak remote-info <remote> <id>`: a `Name - summary` title, then indented `Key: value` lines.
pub(crate) fn parse_remote_info(stdout: &str, remote: &str) -> Result<PackageDetails> {
    let mut d = PackageDetails { manager_id: "flatpak".into(), ..Default::default() };
    let mut title = None;
    for line in stdout.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let Some((k, v)) = line.split_once(':') else {
            title.get_or_insert(line.to_string());
            continue;
        };
        // The title itself may contain a colon; real keys have no spaces.
        if k.contains(' ') {
            title.get_or_insert(line.to_string());
            continue;
        }
        let v = v.replace('\u{a0}', " ");
        let v = v.trim();
        match k {
            "ID" => d.name = v.to_string(),
            "Version" => d.version = non_empty(v),
            "License" => d.license = non_empty(v),
            "Ref" | "Arch" | "Branch" | "Download" | "Installed" | "Runtime" | "Sdk" | "Collection" => {
                d.metadata.insert(k.to_lowercase(), v.to_string());
            }
            _ => {}
        }
    }
    if d.name.is_empty() {
        return Err(crate::error::AppError::Parse("flatpak remote-info: no ID field".into()));
    }
    d.description = title;
    d.metadata.insert("remote".into(), remote.to_string());
    Ok(d)
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

    #[test]
    fn parses_search() {
        // Real output from Flatpak 1.16.6 on Ubuntu (WSL), trimmed.
        let out = "org.mozilla.firefox\t160.0\tFirefox\tFast, Private & Safe Web Browser\norg.freedesktop.Platform.VAAPI.nvidia\t\tNvidia VAAPI driver\tVA-API implementation\n";
        let h = parse_search(out);
        assert_eq!(h.len(), 2);
        assert_eq!(h[0].name, "org.mozilla.firefox");
        assert_eq!(h[0].version.as_deref(), Some("160.0"));
        assert_eq!(h[0].description.as_deref(), Some("Firefox - Fast, Private & Safe Web Browser"));
        assert_eq!(h[1].version, None);
        assert!(parse_search("No matches found\n").is_empty());
        assert!(parse_search("").is_empty());
    }

    #[test]
    fn parses_remote_info() {
        let out = "\nFirefox - Fast, Private & Safe Web Browser\n\n        ID: org.mozilla.firefox\n       Ref: app/org.mozilla.firefox/x86_64/stable\n    Version: 160.0\n   License: MPL-2.0\n Download: 130.9 MB\n   Subject: Export org.mozilla.firefox\n";
        let d = parse_remote_info(out, "flathub").unwrap();
        assert_eq!(d.name, "org.mozilla.firefox");
        assert_eq!(d.version.as_deref(), Some("160.0"));
        assert_eq!(d.license.as_deref(), Some("MPL-2.0"));
        assert_eq!(d.description.as_deref(), Some("Firefox - Fast, Private & Safe Web Browser"));
        assert_eq!(d.metadata["remote"], "flathub");
        assert!(parse_remote_info("error: nope", "x").is_err());
    }

    #[test]
    fn validates_app_ids() {
        for ok in ["org.mozilla.firefox", "com.github.User_name.App-1", "org.gtk.Gtk3theme.Mojave-light"] {
            assert!(validate_app_id(ok).is_ok(), "{ok}");
        }
        for bad in ["firefox", "-org.x", "org..x", "org.x;rm", "org.x y", "org.-x", ""] {
            assert!(validate_app_id(bad).is_err(), "{bad}");
        }
    }
}
