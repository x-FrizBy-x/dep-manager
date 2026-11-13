use super::{non_empty, Adapter};
use crate::error::Result;
use crate::exec::Runner;
use crate::model::{Package, PackageDetails, PackageKind, SearchHit, UpdateInfo};

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
        // `snap --version` pads its first line with runs of spaces ("snap    2.76.3").
        Ok(out.stdout.lines().next().unwrap_or("").split_whitespace().collect::<Vec<_>>().join(" "))
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

    fn supports_search(&self) -> bool {
        true
    }

    fn search(&self, r: &dyn Runner, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
        let terms = super::validate_query(query)?;
        // `snap find` takes one query string; terms are validated and sent as a single argv entry.
        let q = terms.join(" ");
        let out = r.run("snap", &["find", &q])?;
        // "No matching snaps for ..." goes to stderr with a non-zero exit.
        if !out.success() && out.stdout.trim().is_empty() {
            return Ok(Vec::new());
        }
        let mut hits = parse_find(&out.stdout);
        hits.truncate(limit.clamp(1, 50));
        Ok(hits)
    }

    fn details(&self, r: &dyn Runner, name: &str) -> Result<PackageDetails> {
        validate_snap_name(name)?;
        let out = r.run("snap", &["info", name])?.ok()?;
        parse_info(&out.stdout)
    }
}

/// Snap names: lowercase alphanumerics and single hyphens, 2-40 chars, no leading/trailing hyphen.
pub(crate) fn validate_snap_name(name: &str) -> Result<()> {
    let ok = (2..=40).contains(&name.len())
        && !name.starts_with('-')
        && !name.ends_with('-')
        && !name.contains("--")
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if ok {
        Ok(())
    } else {
        Err(crate::error::AppError::Other(format!("invalid snap name `{name}`")))
    }
}

/// `snap find`: fixed-width columns located from the header (`Name Version Publisher Notes Summary`).
pub(crate) fn parse_find(stdout: &str) -> Vec<SearchHit> {
    let mut lines = stdout.lines();
    let Some(header) = lines.next() else { return Vec::new() };
    let (Some(v), Some(pubc), Some(sum)) = (header.find("Version"), header.find("Publisher"), header.find("Summary")) else {
        return Vec::new();
    };
    // `find` gives byte offsets; the header is ASCII, but rows may contain wide chars, so cut by chars.
    let col = |l: &str, from: usize, to: Option<usize>| -> String {
        let chars: Vec<char> = l.chars().collect();
        let end = to.unwrap_or(chars.len()).min(chars.len());
        if from >= end {
            return String::new();
        }
        chars[from..end].iter().collect::<String>().trim().to_string()
    };
    lines
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| {
            let name = col(l, 0, Some(v));
            (!name.is_empty() && !name.contains(' ')).then(|| SearchHit {
                name,
                version: non_empty(&col(l, v, Some(pubc))),
                description: non_empty(&col(l, sum, None)),
            })
        })
        .collect()
}

/// `snap info <name>`: `key: value` lines, a `description: |` block, and a `channels:` table.
pub(crate) fn parse_info(stdout: &str) -> Result<PackageDetails> {
    let mut d = PackageDetails { manager_id: "snap".into(), ..Default::default() };
    let mut desc: Vec<String> = Vec::new();
    let mut section = "";
    for line in stdout.lines() {
        if line.starts_with(' ') {
            match section {
                "description" => desc.push(line.trim().to_string()),
                "channels" => {
                    if let Some((ch, rest)) = line.trim().split_once(':') {
                        if ch == "latest/stable" && d.version.is_none() {
                            let v = rest.split_whitespace().next().unwrap_or("");
                            if v != "--" && v != "^" {
                                d.version = non_empty(v);
                            }
                        }
                    }
                }
                _ => {}
            }
            continue;
        }
        let Some((k, v)) = line.split_once(':') else { continue };
        let v = v.trim();
        section = "";
        match k {
            "name" => d.name = v.to_string(),
            "summary" => d.description = non_empty(v),
            "description" => section = "description",
            "channels" => section = "channels",
            "license" => d.license = non_empty(v),
            "store-url" => d.homepage = non_empty(v),
            "publisher" => {
                d.metadata.insert("publisher".into(), v.to_string());
            }
            _ => {}
        }
    }
    if d.name.is_empty() {
        return Err(crate::error::AppError::Parse("snap info: no name field".into()));
    }
    if !desc.is_empty() {
        let text = desc.join(" ");
        d.description = Some(match d.description.take() {
            Some(s) => format!("{s}. {text}"),
            None => text,
        });
    }
    Ok(d)
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

    #[test]
    fn parses_find_by_columns() {
        // Real `snap find hello` output from Ubuntu (WSL), trimmed.
        let out = "Name                    Version               Publisher           Notes  Summary\nhello                   2.10                  canonical**         -      GNU Hello, the \"hello world\" snap\nhello-pasman            0.25                  pasman1             -      Hello and welcome greetings.\nhello-lhc               1.0                   cprov               -      Hello world application for LHC\u{2602}\n";
        let h = parse_find(out);
        assert_eq!(h.len(), 3);
        assert_eq!(h[0].name, "hello");
        assert_eq!(h[0].version.as_deref(), Some("2.10"));
        assert_eq!(h[0].description.as_deref(), Some("GNU Hello, the \"hello world\" snap"));
        assert!(h[2].description.as_deref().unwrap().ends_with('\u{2602}'));
        assert!(parse_find("").is_empty());
        assert!(parse_find("garbage").is_empty());
    }

    #[test]
    fn parses_info() {
        let out = "name:      hello-world\nsummary:   The 'hello-world' of snaps\npublisher: Canonical**\nstore-url: https://snapcraft.io/hello-world\nlicense:   MIT\ndescription: |\n  This is a simple hello world example.\nsnap-id: abc\nchannels:\n  latest/stable:    6.4 2024-02-27 (29) 20.5kB -\n  latest/edge:      6.5 2024-02-27 (29) 20.5kB -\n";
        let d = parse_info(out).unwrap();
        assert_eq!(d.name, "hello-world");
        assert_eq!(d.version.as_deref(), Some("6.4"));
        assert_eq!(d.license.as_deref(), Some("MIT"));
        assert_eq!(d.homepage.as_deref(), Some("https://snapcraft.io/hello-world"));
        assert!(d.description.unwrap().contains("simple hello world"));
        assert!(parse_info("nothing").is_err());
    }

    #[test]
    fn validates_snap_names() {
        for ok in ["hello", "hello-world", "core22", "a1"] {
            assert!(validate_snap_name(ok).is_ok(), "{ok}");
        }
        for bad in ["a", "-x", "x-", "a--b", "Hello", "a b", "a;b", "--classic", &"a".repeat(41)] {
            assert!(validate_snap_name(bad).is_err(), "{bad}");
        }
    }
}
