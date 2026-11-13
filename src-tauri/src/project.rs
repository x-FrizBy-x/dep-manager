//! Project-level dependencies: what a project folder *declares* (package.json,
//! requirements.txt, pyproject.toml, Cargo.toml) and what its lockfile or
//! `node_modules` says is resolved. Strictly read-only: files are parsed, no
//! command is ever run in the project, and nothing is installed. Kept apart
//! from the global package inventory on purpose.

use crate::error::{AppError, Result};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

const MAX_FILE: u64 = 5 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDep {
    pub name: String,
    /// The requirement exactly as declared (`^1.2.0`, `>=2,<3`, ...).
    pub spec: Option<String>,
    /// `runtime`, `dev`, `optional`, `peer`, `build`, or `group:<name>` / `optional:<extra>`.
    pub group: String,
    /// Version pinned by the lockfile, if one was read.
    pub resolved: Option<String>,
    /// Version found in `node_modules` (npm only).
    pub installed: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Ecosystem {
    /// `npm`, `python` or `cargo`.
    pub id: String,
    pub label: String,
    pub files: Vec<String>,
    pub lockfile: Option<String>,
    pub dependencies: Vec<ProjectDep>,
    /// What was not checked, and why. Shown to the user.
    pub notes: Vec<String>,
    /// Set when a manifest could not be parsed; the rest of the scan still works.
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectScan {
    pub path: String,
    pub name: String,
    pub ecosystems: Vec<Ecosystem>,
    /// Manifests of ecosystems UniManage does not read yet.
    pub unsupported: Vec<String>,
}

fn dep(name: &str, spec: Option<String>, group: &str) -> ProjectDep {
    ProjectDep { name: name.to_string(), spec, group: group.to_string(), resolved: None, installed: None }
}

fn parse_err(what: &str, e: impl std::fmt::Display) -> AppError {
    AppError::Parse(format!("{what}: {e}"))
}

// ---- npm ------------------------------------------------------------------

pub(crate) fn parse_package_json(s: &str) -> Result<Vec<ProjectDep>> {
    let v: serde_json::Value = serde_json::from_str(s).map_err(|e| parse_err("package.json", e))?;
    let mut out = Vec::new();
    for (key, group) in [
        ("dependencies", "runtime"),
        ("devDependencies", "dev"),
        ("optionalDependencies", "optional"),
        ("peerDependencies", "peer"),
    ] {
        if let Some(obj) = v.get(key).and_then(|d| d.as_object()) {
            let mut names: Vec<_> = obj.iter().collect();
            names.sort_by(|a, b| a.0.cmp(b.0));
            for (n, spec) in names {
                if let Some(spec) = spec.as_str() {
                    out.push(dep(n, Some(spec.to_string()), group));
                }
            }
        }
    }
    Ok(out)
}

/// Top-level resolved versions from `package-lock.json` (lockfile v2/v3 `packages`, or v1 `dependencies`).
pub(crate) fn parse_package_lock(s: &str) -> Result<HashMap<String, String>> {
    let v: serde_json::Value = serde_json::from_str(s).map_err(|e| parse_err("package-lock.json", e))?;
    let mut out = HashMap::new();
    if let Some(pk) = v.get("packages").and_then(|p| p.as_object()) {
        for (k, val) in pk {
            if let Some(name) = k.strip_prefix("node_modules/").filter(|n| !n.contains("node_modules/")) {
                if let Some(ver) = val.get("version").and_then(|x| x.as_str()) {
                    out.insert(name.to_string(), ver.to_string());
                }
            }
        }
    } else if let Some(d) = v.get("dependencies").and_then(|p| p.as_object()) {
        for (name, val) in d {
            if let Some(ver) = val.get("version").and_then(|x| x.as_str()) {
                out.insert(name.clone(), ver.to_string());
            }
        }
    }
    Ok(out)
}

fn read_capped(path: &Path) -> Result<String> {
    let meta = std::fs::metadata(path).map_err(|e| AppError::Other(format!("{}: {e}", path.display())))?;
    if meta.len() > MAX_FILE {
        return Err(AppError::Other(format!("{} is larger than 5 MB", path.display())));
    }
    std::fs::read_to_string(path).map_err(|e| AppError::Other(format!("{}: {e}", path.display())))
}

fn installed_npm_version(dir: &Path, name: &str) -> Option<String> {
    // The name is validated first so it cannot walk out of node_modules.
    crate::ops::validate_npm_name(name).ok()?;
    let p = dir.join("node_modules").join(name).join("package.json");
    let v: serde_json::Value = serde_json::from_str(&read_capped(&p).ok()?).ok()?;
    v.get("version")?.as_str().map(str::to_string)
}

// ---- python ---------------------------------------------------------------

/// One PEP 508 requirement (`name[extra]>=1.0; marker`) or requirements.txt line.
pub(crate) fn parse_requirement(line: &str, group: &str) -> Option<ProjectDep> {
    let line = line.split(" #").next().unwrap_or("").trim();
    let line = line.split(';').next().unwrap_or("").trim();
    if line.is_empty() || line.starts_with('-') || line.starts_with('#') {
        return None;
    }
    let end = line.find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))).unwrap_or(line.len());
    let name = &line[..end];
    if name.is_empty() || !name.starts_with(|c: char| c.is_ascii_alphanumeric()) {
        return None;
    }
    let mut rest = line[end..].trim();
    if rest.starts_with('[') {
        rest = rest.split_once(']').map(|(_, r)| r.trim()).unwrap_or("");
    }
    let rest = rest.trim_start_matches('(').trim_end_matches(')').trim();
    Some(dep(name, (!rest.is_empty()).then(|| rest.to_string()), group))
}

pub(crate) fn parse_requirements(s: &str, group: &str) -> Vec<ProjectDep> {
    s.lines().filter_map(|l| parse_requirement(l, group)).collect()
}

pub(crate) fn parse_pyproject(s: &str) -> Result<Vec<ProjectDep>> {
    let t: toml::Table = s.parse().map_err(|e| parse_err("pyproject.toml", e))?;
    let mut out = Vec::new();
    let strings = |v: &toml::Value, group: &str, out: &mut Vec<ProjectDep>| {
        if let Some(a) = v.as_array() {
            out.extend(a.iter().filter_map(|x| x.as_str()).filter_map(|l| parse_requirement(l, group)));
        }
    };
    if let Some(p) = t.get("project").and_then(|p| p.as_table()) {
        if let Some(d) = p.get("dependencies") {
            strings(d, "runtime", &mut out);
        }
        if let Some(o) = p.get("optional-dependencies").and_then(|o| o.as_table()) {
            for (extra, v) in o {
                strings(v, &format!("optional:{extra}"), &mut out);
            }
        }
    }
    if let Some(g) = t.get("dependency-groups").and_then(|g| g.as_table()) {
        for (name, v) in g {
            strings(v, &format!("group:{name}"), &mut out);
        }
    }
    if let Some(poetry) = t.get("tool").and_then(|x| x.get("poetry")).and_then(|x| x.as_table()) {
        let table = |v: &toml::Value, group: &str, out: &mut Vec<ProjectDep>| {
            if let Some(d) = v.as_table() {
                for (n, spec) in d.iter().filter(|(n, _)| n.as_str() != "python") {
                    let spec = match spec {
                        toml::Value::String(s) => Some(s.clone()),
                        toml::Value::Table(t) => t.get("version").and_then(|v| v.as_str()).map(str::to_string),
                        _ => None,
                    };
                    out.push(dep(n, spec, group));
                }
            }
        };
        if let Some(d) = poetry.get("dependencies") {
            table(d, "runtime", &mut out);
        }
        if let Some(groups) = poetry.get("group").and_then(|g| g.as_table()) {
            for (g, v) in groups {
                if let Some(d) = v.get("dependencies") {
                    table(d, &format!("group:{g}"), &mut out);
                }
            }
        }
    }
    Ok(out)
}

// ---- cargo ----------------------------------------------------------------

pub(crate) fn parse_cargo_toml(s: &str) -> Result<Vec<ProjectDep>> {
    let t: toml::Table = s.parse().map_err(|e| parse_err("Cargo.toml", e))?;
    let mut out = Vec::new();
    for (key, group) in [("dependencies", "runtime"), ("dev-dependencies", "dev"), ("build-dependencies", "build")] {
        if let Some(d) = t.get(key).and_then(|d| d.as_table()) {
            for (name, v) in d {
                let spec = match v {
                    toml::Value::String(s) => Some(s.clone()),
                    toml::Value::Table(t) => {
                        if let Some(v) = t.get("version").and_then(|v| v.as_str()) {
                            Some(v.to_string())
                        } else if let Some(p) = t.get("path").and_then(|v| v.as_str()) {
                            Some(format!("path {p}"))
                        } else if let Some(g) = t.get("git").and_then(|v| v.as_str()) {
                            Some(format!("git {g}"))
                        } else if t.get("workspace").and_then(|v| v.as_bool()) == Some(true) {
                            Some("workspace".to_string())
                        } else {
                            None
                        }
                    }
                    _ => None,
                };
                out.push(dep(name, spec, group));
            }
        }
    }
    Ok(out)
}

/// Resolved versions from `Cargo.lock`. A crate locked at several versions is left out
/// rather than guessed.
pub(crate) fn parse_cargo_lock(s: &str) -> Result<HashMap<String, String>> {
    let t: toml::Table = s.parse().map_err(|e| parse_err("Cargo.lock", e))?;
    let mut seen: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for p in t.get("package").and_then(|p| p.as_array()).into_iter().flatten() {
        if let (Some(n), Some(v)) = (p.get("name").and_then(|x| x.as_str()), p.get("version").and_then(|x| x.as_str())) {
            let e = seen.entry(n.to_string()).or_default();
            if !e.iter().any(|x| x == v) {
                e.push(v.to_string());
            }
        }
    }
    Ok(seen.into_iter().filter_map(|(n, mut v)| (v.len() == 1).then(|| (n, v.remove(0)))).collect())
}

// ---- scanning a folder ----------------------------------------------------

/// Validates and canonicalizes a user-supplied project folder.
pub fn resolve_dir(input: &str) -> Result<PathBuf> {
    let input = input.trim();
    if input.is_empty() || input.len() > 1024 || input.chars().any(char::is_control) {
        return Err(AppError::Other("enter a folder path".into()));
    }
    let p = std::fs::canonicalize(input).map_err(|e| AppError::Other(format!("cannot open `{input}`: {e}")))?;
    if !p.is_dir() {
        return Err(AppError::Other(format!("`{input}` is not a folder")));
    }
    // Windows canonical paths carry a `\\?\` prefix that is noise in the UI.
    let s = p.to_string_lossy();
    Ok(PathBuf::from(s.strip_prefix(r"\\?\").unwrap_or(&s).to_string()))
}

fn exists(dir: &Path, f: &str) -> bool {
    dir.join(f).is_file()
}

fn scan_npm(dir: &Path) -> Option<Ecosystem> {
    if !exists(dir, "package.json") {
        return None;
    }
    let mut e = Ecosystem {
        id: "npm".into(),
        label: "npm (package.json)".into(),
        files: vec!["package.json".into()],
        lockfile: None,
        dependencies: Vec::new(),
        notes: Vec::new(),
        error: None,
    };
    match read_capped(&dir.join("package.json")).and_then(|s| parse_package_json(&s)) {
        Ok(d) => e.dependencies = d,
        Err(err) => {
            e.error = Some(err.to_string());
            return Some(e);
        }
    }
    let mut lock = HashMap::new();
    if exists(dir, "package-lock.json") {
        match read_capped(&dir.join("package-lock.json")).and_then(|s| parse_package_lock(&s)) {
            Ok(l) => {
                lock = l;
                e.lockfile = Some("package-lock.json".into());
            }
            Err(err) => e.notes.push(format!("package-lock.json could not be read: {err}")),
        }
    } else {
        for other in ["yarn.lock", "pnpm-lock.yaml", "bun.lockb", "bun.lock"] {
            if exists(dir, other) {
                e.notes.push(format!("{other} found but not read (only package-lock.json is supported); resolved versions are unknown"));
            }
        }
    }
    let have_modules = dir.join("node_modules").is_dir();
    if !have_modules {
        e.notes.push("node_modules not found: dependencies are not installed in this folder".into());
    }
    for d in &mut e.dependencies {
        d.resolved = lock.get(&d.name).cloned();
        if have_modules {
            d.installed = installed_npm_version(dir, &d.name);
        }
    }
    Some(e)
}

fn scan_python(dir: &Path) -> Option<Ecosystem> {
    let reqs: Vec<&str> = ["requirements.txt", "requirements-dev.txt"].into_iter().filter(|f| exists(dir, f)).collect();
    let pyproject = exists(dir, "pyproject.toml");
    if reqs.is_empty() && !pyproject {
        return None;
    }
    let mut e = Ecosystem {
        id: "python".into(),
        label: "Python".into(),
        files: Vec::new(),
        lockfile: None,
        dependencies: Vec::new(),
        notes: vec!["Installed versions are not checked: that would mean running Python in the project".into()],
        error: None,
    };
    let mut errors = Vec::new();
    if pyproject {
        e.files.push("pyproject.toml".into());
        match read_capped(&dir.join("pyproject.toml")).and_then(|s| parse_pyproject(&s)) {
            Ok(d) => e.dependencies.extend(d),
            Err(err) => errors.push(err.to_string()),
        }
    }
    for f in reqs {
        e.files.push(f.to_string());
        let group = if f.contains("dev") { "dev" } else { "runtime" };
        match read_capped(&dir.join(f)) {
            Ok(s) => e.dependencies.extend(parse_requirements(&s, group)),
            Err(err) => errors.push(err.to_string()),
        }
    }
    for l in ["poetry.lock", "uv.lock", "Pipfile.lock"] {
        if exists(dir, l) {
            e.notes.push(format!("{l} found but not read; resolved versions are unknown"));
        }
    }
    if !errors.is_empty() {
        e.error = Some(errors.join("; "));
    }
    Some(e)
}

fn scan_cargo(dir: &Path) -> Option<Ecosystem> {
    if !exists(dir, "Cargo.toml") {
        return None;
    }
    let mut e = Ecosystem {
        id: "cargo".into(),
        label: "Rust (Cargo.toml)".into(),
        files: vec!["Cargo.toml".into()],
        lockfile: None,
        dependencies: Vec::new(),
        notes: Vec::new(),
        error: None,
    };
    match read_capped(&dir.join("Cargo.toml")).and_then(|s| parse_cargo_toml(&s)) {
        Ok(d) => e.dependencies = d,
        Err(err) => {
            e.error = Some(err.to_string());
            return Some(e);
        }
    }
    if exists(dir, "Cargo.lock") {
        match read_capped(&dir.join("Cargo.lock")).and_then(|s| parse_cargo_lock(&s)) {
            Ok(lock) => {
                e.lockfile = Some("Cargo.lock".into());
                e.notes.push("Locked shows a dash when Cargo.lock pins several versions of that crate (not guessed)".into());
                for d in &mut e.dependencies {
                    d.resolved = lock.get(&d.name).cloned();
                }
            }
            Err(err) => e.notes.push(format!("Cargo.lock could not be read: {err}")),
        }
    } else {
        e.notes.push("no Cargo.lock: resolved versions are unknown".into());
    }
    e.notes.push("Workspace members in sub-folders are not scanned; point at a member folder to see its dependencies".into());
    Some(e)
}

/// Reads the manifests directly inside `dir` (no recursion).
pub fn scan_dir(dir: &Path) -> ProjectScan {
    let ecosystems: Vec<Ecosystem> = [scan_npm(dir), scan_python(dir), scan_cargo(dir)].into_iter().flatten().collect();
    let unsupported = ["go.mod", "Gemfile", "composer.json", "pom.xml", "build.gradle", "build.gradle.kts", "mix.exs", "pubspec.yaml"]
        .into_iter()
        .filter(|f| exists(dir, f))
        .map(str::to_string)
        .collect();
    ProjectScan {
        path: dir.to_string_lossy().into_owned(),
        name: dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| dir.to_string_lossy().into_owned()),
        ecosystems,
        unsupported,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(d: &[ProjectDep]) -> Vec<String> {
        d.iter().map(|x| format!("{}:{}", x.group, x.name)).collect()
    }

    #[test]
    fn package_json_groups_and_non_string_specs() {
        let d = parse_package_json(
            r#"{"dependencies":{"react":"^19","a":"1"},"devDependencies":{"vitest":"^5"},"peerDependencies":{"x":"*"},"optionalDependencies":{"y":{"bad":1}}}"#,
        )
        .unwrap();
        assert_eq!(names(&d), ["runtime:a", "runtime:react", "dev:vitest", "peer:x"]);
        assert_eq!(d[1].spec.as_deref(), Some("^19"));
        assert!(parse_package_json("nope").is_err());
        assert!(parse_package_json("{}").unwrap().is_empty());
    }

    #[test]
    fn package_lock_v3_and_v1_top_level_only() {
        let v3 = r#"{"packages":{"":{"name":"p"},"node_modules/react":{"version":"19.1.0"},"node_modules/@a/b":{"version":"2.0.0"},"node_modules/react/node_modules/inner":{"version":"9.9.9"}}}"#;
        let l = parse_package_lock(v3).unwrap();
        assert_eq!(l["react"], "19.1.0");
        assert_eq!(l["@a/b"], "2.0.0");
        assert!(!l.contains_key("inner") && !l.contains_key("react/node_modules/inner"));
        let v1 = parse_package_lock(r#"{"dependencies":{"left-pad":{"version":"1.3.0"}}}"#).unwrap();
        assert_eq!(v1["left-pad"], "1.3.0");
    }

    #[test]
    fn requirement_lines() {
        let d = parse_requirements(
            "# comment\nrequests==2.31.0  # pinned\nflask[async]>=2.0,<3 ; python_version>'3.8'\n-r base.txt\n-e .\n--index-url https://x\nnumpy\n\n  pytest (>=7)\n",
            "runtime",
        );
        let got: Vec<_> = d.iter().map(|x| (x.name.as_str(), x.spec.as_deref())).collect();
        assert_eq!(got, [("requests", Some("==2.31.0")), ("flask", Some(">=2.0,<3")), ("numpy", None), ("pytest", Some(">=7"))]);
    }

    #[test]
    fn pyproject_pep621_groups_and_poetry() {
        let d = parse_pyproject(
            "[project]\ndependencies=[\"httpx>=0.27\",\"rich\"]\n[project.optional-dependencies]\ncli=[\"click>=8\"]\n[dependency-groups]\ndev=[\"pytest\",{include-group=\"x\"}]\n[tool.poetry.dependencies]\npython=\"^3.11\"\ndjango={version=\"^5\",extras=[\"a\"]}\n[tool.poetry.group.test.dependencies]\ncoverage=\"7\"\n",
        )
        .unwrap();
        assert_eq!(
            names(&d),
            ["runtime:httpx", "runtime:rich", "optional:cli:click", "group:dev:pytest", "runtime:django", "group:test:coverage"]
        );
        assert_eq!(d[4].spec.as_deref(), Some("^5"));
        assert!(parse_pyproject("[[").is_err());
    }

    #[test]
    fn cargo_toml_and_lock() {
        let d = parse_cargo_toml(
            "[dependencies]\nserde={version=\"1\",features=[\"derive\"]}\nlocal={path=\"../l\"}\nws={workspace=true}\ntoml=\"0.8\"\n[dev-dependencies]\ntempfile=\"3\"\n",
        )
        .unwrap();
        assert_eq!(names(&d), ["runtime:local", "runtime:serde", "runtime:toml", "runtime:ws", "dev:tempfile"]);
        assert_eq!(d[0].spec.as_deref(), Some("path ../l"));
        assert_eq!(d[3].spec.as_deref(), Some("workspace"));
        let lock = parse_cargo_lock(
            "[[package]]\nname=\"serde\"\nversion=\"1.0.2\"\n[[package]]\nname=\"dup\"\nversion=\"1.0.0\"\n[[package]]\nname=\"dup\"\nversion=\"2.0.0\"\n",
        )
        .unwrap();
        assert_eq!(lock["serde"], "1.0.2");
        assert!(!lock.contains_key("dup"), "ambiguous versions are not guessed");
    }

    #[test]
    fn scans_a_real_folder() {
        let dir = std::env::temp_dir().join(format!("unimanage-proj-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("node_modules/react")).unwrap();
        std::fs::write(dir.join("package.json"), r#"{"dependencies":{"react":"^19","ghost":"1"},"devDependencies":{"../evil":"1"}}"#).unwrap();
        std::fs::write(dir.join("package-lock.json"), r#"{"packages":{"node_modules/react":{"version":"19.1.0"}}}"#).unwrap();
        std::fs::write(dir.join("node_modules/react/package.json"), r#"{"version":"19.0.0"}"#).unwrap();
        std::fs::write(dir.join("requirements.txt"), "requests==2\n").unwrap();
        std::fs::write(dir.join("go.mod"), "module x\n").unwrap();
        let s = scan_dir(&dir);
        assert_eq!(s.unsupported, ["go.mod"]);
        let npm = s.ecosystems.iter().find(|e| e.id == "npm").unwrap();
        assert_eq!(npm.lockfile.as_deref(), Some("package-lock.json"));
        let react = npm.dependencies.iter().find(|d| d.name == "react").unwrap();
        assert_eq!((react.resolved.as_deref(), react.installed.as_deref()), (Some("19.1.0"), Some("19.0.0")));
        let ghost = npm.dependencies.iter().find(|d| d.name == "ghost").unwrap();
        assert_eq!((ghost.resolved.as_deref(), ghost.installed.as_deref()), (None, None));
        // A traversal-looking name is never used to build a path.
        assert_eq!(npm.dependencies.iter().find(|d| d.name == "../evil").unwrap().installed, None);
        assert_eq!(s.ecosystems.iter().find(|e| e.id == "python").unwrap().dependencies.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn broken_manifest_is_reported_not_fatal() {
        let dir = std::env::temp_dir().join(format!("unimanage-proj-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("package.json"), "{oops").unwrap();
        std::fs::write(dir.join("Cargo.toml"), "[dependencies]\na=\"1\"\n").unwrap();
        let s = scan_dir(&dir);
        assert!(s.ecosystems.iter().find(|e| e.id == "npm").unwrap().error.is_some());
        assert_eq!(s.ecosystems.iter().find(|e| e.id == "cargo").unwrap().dependencies.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(resolve_dir("").is_err());
        assert!(resolve_dir("Z:/definitely/not/here").is_err());
    }
}
