//! Environment manifests: a per-manager snapshot of what is installed, and a
//! diff against the current inventory. Pure functions only: nothing here runs
//! a command or touches the system.
//!
//! A manifest is a record of intent, not a reproducible-build guarantee:
//! versions may no longer be available, other managers' packages are never
//! merged, and nothing is ever removed to "match" a manifest.

use crate::error::{AppError, Result};
use crate::model::{ManagerInfo, Package, PackageKind};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

pub const FORMAT: &str = "unimanage-manifest";
pub const VERSION: u32 = 1;
const MAX_BYTES: usize = 5 * 1024 * 1024;
const MAX_PACKAGES: usize = 50_000;
const MAX_FIELD: usize = 300;

pub const NOTE: &str = "Records what was installed per package manager. It does not guarantee an exact reproduction: \
versions may no longer be available and nothing is removed on restore.";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub format: String,
    pub version: u32,
    pub name: String,
    pub created_at: i64,
    pub platform: String,
    #[serde(default)]
    pub note: String,
    pub managers: BTreeMap<String, ManifestManager>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestManager {
    pub manager_version: Option<String>,
    pub packages: Vec<ManifestPackage>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestPackage {
    pub name: String,
    pub version: Option<String>,
    pub kind: PackageKind,
    pub source: Option<String>,
}

/// Summary row for the manifest list.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestSummary {
    pub id: i64,
    pub name: String,
    pub platform: String,
    pub created_at: i64,
    pub package_count: usize,
    pub managers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DiffEntry {
    pub manager_id: String,
    pub name: String,
    pub wanted_version: Option<String>,
    pub installed_version: Option<String>,
    /// `missing` (not installed) or `different` (installed, other version).
    pub status: String,
    /// Whether UniManage has an install pipeline for it right now.
    pub restorable: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ManagerDiff {
    pub manager_id: String,
    pub detected: bool,
    pub same: usize,
    /// Installed here but not in the manifest. Informational: never removed.
    pub extra: usize,
    pub entries: Vec<DiffEntry>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ManifestDiff {
    pub manifest_id: i64,
    pub name: String,
    pub managers: Vec<ManagerDiff>,
}

/// Packages worth recording: not runtimes, and for apt not auto-installed dependencies.
fn included(p: &Package) -> bool {
    p.kind != PackageKind::Runtime && p.metadata.get("installReason").map(String::as_str) != Some("automatic")
}

pub fn build(packages: &[Package], managers: &[ManagerInfo], name: &str, platform: &str, now: i64) -> Manifest {
    let mut out: BTreeMap<String, ManifestManager> = BTreeMap::new();
    for m in managers.iter().filter(|m| m.detected) {
        out.insert(m.id.clone(), ManifestManager { manager_version: m.version.clone(), packages: Vec::new() });
    }
    for p in packages.iter().filter(|p| included(p)) {
        if let Some(m) = out.get_mut(&p.package_manager) {
            if !m.packages.iter().any(|x| x.name == p.name) {
                m.packages.push(ManifestPackage {
                    name: p.name.clone(),
                    version: p.version.clone(),
                    kind: p.kind,
                    source: p.installation_source.clone(),
                });
            }
        }
    }
    for m in out.values_mut() {
        m.packages.sort_by(|a, b| a.name.cmp(&b.name));
    }
    out.retain(|_, m| !m.packages.is_empty());
    Manifest {
        format: FORMAT.into(),
        version: VERSION,
        name: name.to_string(),
        created_at: now,
        platform: platform.to_string(),
        note: NOTE.into(),
        managers: out,
    }
}

fn check_text(what: &str, s: &str) -> Result<()> {
    if s.chars().count() > MAX_FIELD || s.chars().any(char::is_control) {
        return Err(AppError::Other(format!("invalid manifest: bad {what}")));
    }
    Ok(())
}

/// Parses and validates untrusted manifest JSON.
pub fn parse(content: &str) -> Result<Manifest> {
    if content.len() > MAX_BYTES {
        return Err(AppError::Other("manifest is larger than 5 MB".into()));
    }
    let m: Manifest = serde_json::from_str(content).map_err(|e| AppError::Other(format!("not a valid manifest: {e}")))?;
    if m.format != FORMAT {
        return Err(AppError::Other(format!("not a UniManage manifest (format `{}`)", m.format)));
    }
    if m.version != VERSION {
        return Err(AppError::Other(format!("unsupported manifest version {} (expected {VERSION})", m.version)));
    }
    check_text("name", &m.name)?;
    if m.name.trim().is_empty() {
        return Err(AppError::Other("invalid manifest: empty name".into()));
    }
    check_text("platform", &m.platform)?;
    let mut total = 0usize;
    for (id, mm) in &m.managers {
        if id.is_empty() || id.len() > 32 || !id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-') {
            return Err(AppError::Other(format!("invalid manifest: bad manager id `{id}`")));
        }
        if let Some(v) = &mm.manager_version {
            check_text("manager version", v)?;
        }
        let mut seen = HashSet::new();
        for p in &mm.packages {
            total += 1;
            if total > MAX_PACKAGES {
                return Err(AppError::Other("manifest has too many packages".into()));
            }
            if p.name.is_empty() {
                return Err(AppError::Other(format!("invalid manifest: empty package name under `{id}`")));
            }
            check_text("package name", &p.name)?;
            if let Some(v) = &p.version {
                check_text("version", v)?;
            }
            if let Some(s) = &p.source {
                check_text("source", s)?;
            }
            if !seen.insert(p.name.as_str()) {
                return Err(AppError::Other(format!("invalid manifest: duplicate `{}` under `{id}`", p.name)));
            }
        }
    }
    Ok(m)
}

pub fn summary(id: i64, created_at: i64, m: &Manifest) -> ManifestSummary {
    ManifestSummary {
        id,
        name: m.name.clone(),
        platform: m.platform.clone(),
        created_at,
        package_count: m.managers.values().map(|x| x.packages.len()).sum(),
        managers: m.managers.keys().cloned().collect(),
    }
}

pub fn to_json(m: &Manifest) -> Result<String> {
    Ok(serde_json::to_string_pretty(m)?)
}

/// Hand-written YAML (for reading and for other tools). Scalars are emitted as
/// JSON strings, which are valid YAML double-quoted scalars. UniManage imports JSON only.
pub fn to_yaml(m: &Manifest) -> String {
    let q = |s: &str| serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into());
    let opt = |s: &Option<String>| s.as_deref().map(q).unwrap_or_else(|| "null".into());
    let mut y = String::new();
    y.push_str(&format!("format: {}\nversion: {}\nname: {}\ncreatedAt: {}\nplatform: {}\nnote: {}\nmanagers:\n", q(&m.format), m.version, q(&m.name), m.created_at, q(&m.platform), q(&m.note)));
    if m.managers.is_empty() {
        y.truncate(y.len() - "managers:\n".len());
        y.push_str("managers: {}\n");
    }
    for (id, mm) in &m.managers {
        y.push_str(&format!("  {}:\n    managerVersion: {}\n    packages:\n", q(id), opt(&mm.manager_version)));
        for p in &mm.packages {
            let kind = serde_json::to_value(p.kind).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
            y.push_str(&format!(
                "      - name: {}\n        version: {}\n        kind: {}\n        source: {}\n",
                q(&p.name),
                opt(&p.version),
                q(&kind),
                opt(&p.source)
            ));
        }
    }
    y
}

/// Compares a manifest with the current inventory. `detected` = managers present
/// at the last scan; `operable` says whether an install pipeline exists.
pub fn diff(
    manifest_id: i64,
    m: &Manifest,
    installed: &[Package],
    detected: &HashSet<String>,
    operable: impl Fn(&str) -> bool,
) -> ManifestDiff {
    let mut managers = Vec::new();
    for (id, mm) in &m.managers {
        let have: HashMap<&str, &Package> =
            installed.iter().filter(|p| &p.package_manager == id).map(|p| (p.name.as_str(), p)).collect();
        let wanted: HashSet<&str> = mm.packages.iter().map(|p| p.name.as_str()).collect();
        let is_detected = detected.contains(id);
        let mut same = 0;
        let mut entries = Vec::new();
        for p in &mm.packages {
            let cur = have.get(p.name.as_str());
            let status = match (cur, &p.version) {
                (None, _) => "missing",
                (Some(c), Some(w)) if c.version.as_deref() != Some(w.as_str()) => "different",
                _ => {
                    same += 1;
                    continue;
                }
            };
            let reason = if !is_detected {
                Some(format!("{id} is not detected on this machine"))
            } else if !operable(id) {
                Some(format!("no install pipeline for {id} yet"))
            } else if id == "npm" && p.name == "npm" {
                Some("npm itself is not replaced through UniManage".to_string())
            } else {
                None
            };
            entries.push(DiffEntry {
                manager_id: id.clone(),
                name: p.name.clone(),
                wanted_version: p.version.clone(),
                installed_version: cur.and_then(|c| c.version.clone()),
                status: status.into(),
                restorable: reason.is_none(),
                reason,
            });
        }
        let extra = installed
            .iter()
            .filter(|p| &p.package_manager == id && included(p) && !wanted.contains(p.name.as_str()))
            .count();
        managers.push(ManagerDiff { manager_id: id.clone(), detected: is_detected, same, extra, entries });
    }
    ManifestDiff { manifest_id, name: m.name.clone(), managers }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pkg(mgr: &str, name: &str, ver: &str) -> Package {
        let mut p = Package::new(mgr, format!("{mgr}:{name}"), name);
        p.version = Some(ver.into());
        p.kind = PackageKind::Library;
        p
    }
    fn info(id: &str, detected: bool) -> ManagerInfo {
        ManagerInfo {
            id: id.into(),
            display_name: id.into(),
            detected,
            path: None,
            version: Some("1".into()),
            supports_updates: true,
            package_count: 0,
            last_error: None,
        }
    }

    #[test]
    fn build_filters_and_sorts() {
        let mut auto = pkg("apt", "libfoo", "1");
        auto.metadata.insert("installReason".into(), "automatic".into());
        let mut rt = pkg("apt", "core22", "1");
        rt.kind = PackageKind::Runtime;
        let pk = vec![pkg("npm", "zed", "1.0.0"), pkg("npm", "alpha", "2.0.0"), auto, rt, pkg("apt", "curl", "8"), pkg("pipx", "x", "1")];
        let m = build(&pk, &[info("npm", true), info("apt", true), info("pipx", false)], "t", "linux", 5);
        let names: Vec<_> = m.managers["npm"].packages.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["alpha", "zed"]);
        assert_eq!(m.managers["apt"].packages.len(), 1);
        assert!(!m.managers.contains_key("pipx"));
        assert_eq!(parse(&to_json(&m).unwrap()).unwrap(), m);
    }

    #[test]
    fn yaml_is_quoted_and_complete() {
        let m = build(&[pkg("npm", "@a/b", "1.0.0: x")], &[info("npm", true)], "my \"env\"", "linux", 1);
        let y = to_yaml(&m);
        assert!(y.contains("name: \"my \\\"env\\\"\""));
        assert!(y.contains("- name: \"@a/b\""));
        assert!(y.contains("version: \"1.0.0: x\""));
        assert!(to_yaml(&build(&[], &[], "e", "linux", 1)).contains("managers: {}"));
    }

    #[test]
    fn parse_rejects_bad_input() {
        let good = to_json(&build(&[pkg("npm", "a", "1")], &[info("npm", true)], "t", "linux", 1)).unwrap();
        assert!(parse(&good).is_ok());
        assert!(parse("not json").is_err());
        assert!(parse(&good.replace(FORMAT, "other")).is_err());
        assert!(parse(&good.replace("\"version\": 1,", "\"version\": 9,")).is_err());
        assert!(parse(&good.replace("\"npm\":", "\"NPM;rm\":")).is_err());
        assert!(parse(&good.replace("\"name\": \"a\"", "\"name\": \"a\\u0007b\"")).is_err());
        assert!(parse(&"x".repeat(MAX_BYTES + 1)).is_err());
        let dup = "{\"format\":\"unimanage-manifest\",\"version\":1,\"name\":\"t\",\"createdAt\":1,\"platform\":\"l\",\"managers\":{\"npm\":{\"managerVersion\":null,\"packages\":[{\"name\":\"a\",\"version\":null,\"kind\":\"library\",\"source\":null},{\"name\":\"a\",\"version\":null,\"kind\":\"library\",\"source\":null}]}}}";
        assert!(parse(dup).is_err());
    }

    #[test]
    fn diff_classifies_and_gates_restore() {
        let m = build(
            &[pkg("npm", "a", "1"), pkg("npm", "b", "2"), pkg("npm", "c", "3"), pkg("npm", "npm", "12"), pkg("apt", "curl", "8")],
            &[info("npm", true), info("apt", true)],
            "t",
            "linux",
            1,
        );
        let now = vec![pkg("npm", "a", "1"), pkg("npm", "b", "9"), pkg("npm", "extra", "1")];
        let detected: HashSet<String> = ["npm".to_string(), "apt".to_string()].into();
        let d = diff(7, &m, &now, &detected, |id| id == "npm");
        let npm = d.managers.iter().find(|x| x.manager_id == "npm").unwrap();
        assert_eq!((npm.same, npm.extra), (1, 1));
        let by = |n: &str| npm.entries.iter().find(|e| e.name == n).unwrap();
        assert_eq!(by("b").status, "different");
        assert_eq!(by("b").installed_version.as_deref(), Some("9"));
        assert!(by("b").restorable);
        assert_eq!(by("c").status, "missing");
        assert!(by("c").restorable);
        assert!(!by("npm").restorable);
        let apt = d.managers.iter().find(|x| x.manager_id == "apt").unwrap();
        assert!(!apt.entries[0].restorable);
        assert!(apt.entries[0].reason.as_deref().unwrap().contains("no install pipeline"));
        let none: HashSet<String> = HashSet::new();
        let d2 = diff(7, &m, &now, &none, |_| true);
        assert!(d2.managers.iter().all(|m| m.entries.iter().all(|e| !e.restorable)));
    }
}
