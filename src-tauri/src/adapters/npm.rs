use super::{first_line, Adapter};
use crate::error::{AppError, Result};
use crate::exec::Runner;
use crate::model::{DependencyEdge, Package, PackageDependencies, PackageDetails, PackageKind, SearchHit, UpdateInfo};
use serde_json::Value;

/// Global npm packages only. Project-level dependencies are deliberately
/// out of scope and must never be mixed into this adapter.
pub struct Npm;

impl Adapter for Npm {
    fn id(&self) -> &'static str {
        "npm"
    }
    fn display_name(&self) -> &'static str {
        "npm (global)"
    }
    fn binary(&self) -> &'static str {
        "npm"
    }
    fn supports_updates(&self) -> bool {
        true
    }

    fn version(&self, r: &dyn Runner) -> Result<String> {
        Ok(first_line(&r.run("npm", &["--version"])?.ok()?.stdout))
    }

    fn list_installed(&self, r: &dyn Runner) -> Result<Vec<Package>> {
        // npm exits non-zero for e.g. extraneous/invalid trees yet still prints
        // valid JSON, so exit status is not checked here.
        let out = r.run("npm", &["ls", "-g", "--depth=0", "--json"])?;
        let root = r.run("npm", &["root", "-g"]).ok().filter(|o| o.success()).map(|o| first_line(&o.stdout));
        parse_ls(&out.stdout, root.as_deref())
    }

    fn check_updates(&self, r: &dyn Runner) -> Result<Vec<UpdateInfo>> {
        // Exit code 1 simply means "something is outdated".
        let out = r.run("npm", &["outdated", "-g", "--json"])?;
        if !matches!(out.status, Some(0 | 1)) {
            return out.ok().map(|_| vec![]);
        }
        parse_outdated(&out.stdout)
    }

    fn supports_search(&self) -> bool {
        true
    }

    fn search(&self, r: &dyn Runner, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
        let terms = super::validate_query(query)?;
        let limit_arg = format!("--searchlimit={}", limit.clamp(1, 50));
        let mut args: Vec<&str> = vec!["search", "--json", &limit_arg];
        args.extend(terms.iter().map(String::as_str));
        let out = r.run("npm", &args)?.ok()?;
        parse_search(&out.stdout)
    }

    fn details(&self, r: &dyn Runner, name: &str) -> Result<PackageDetails> {
        crate::ops::validate_npm_name(name)?;
        let out = r.run("npm", &["view", name, "--json"])?.ok()?;
        parse_view(&out.stdout)
    }

    fn supports_dependencies(&self) -> bool {
        true
    }

    fn dependencies(&self, r: &dyn Runner, name: &str) -> Result<PackageDependencies> {
        crate::ops::validate_npm_name(name)?;
        // Like `npm ls -g --depth=0`, a non-zero exit (extraneous/invalid
        // entries) still comes with usable JSON, so the status is ignored.
        let out = r.run("npm", &["ls", "-g", "--all", "--json"])?;
        parse_dependencies(&out.stdout, name)
    }
}

/// `npm ls -g --all --json`: `{dependencies: {name: {version, dependencies}}}`.
/// An entry without `version` (usually `{}`) is declared but not installed,
/// typically an optional dependency for another platform.
pub(crate) fn parse_dependencies(stdout: &str, name: &str) -> Result<PackageDependencies> {
    let root: Value = serde_json::from_str(stdout)?;
    let top = root.get("dependencies").and_then(Value::as_object);
    let subject = top
        .and_then(|t| t.get(name))
        .ok_or_else(|| AppError::Other(format!("`{name}` is not a globally installed npm package")))?;

    let edge = |n: &str, v: &Value, kind: &str| DependencyEdge {
        name: n.to_string(),
        version: v.get("version").and_then(Value::as_str).map(String::from),
        kind: kind.to_string(),
        installed: v.get("version").is_some(),
    };
    let children = |v: &Value| v.get("dependencies").and_then(Value::as_object).cloned().unwrap_or_default();

    let depends_on: Vec<DependencyEdge> = children(subject)
        .iter()
        .map(|(n, v)| edge(n, v, if v.get("version").is_some() { "dependency" } else { "optional / not installed" }))
        .collect();

    // Everything below the direct level, de-duplicated by name@version.
    let mut seen: std::collections::HashSet<(String, Option<String>)> =
        depends_on.iter().map(|e| (e.name.clone(), e.version.clone())).collect();
    let mut transitive = Vec::new();
    let mut stack: Vec<Value> = children(subject).into_values().collect();
    while let Some(node) = stack.pop() {
        for (n, v) in children(&node) {
            let e = edge(&n, &v, "transitive");
            if e.installed && seen.insert((e.name.clone(), e.version.clone())) {
                transitive.push(e);
            }
            stack.push(v);
        }
    }
    transitive.sort_by(|a, b| a.name.cmp(&b.name));

    // Other global packages whose trees contain this package.
    let mut required_by = Vec::new();
    for (parent, pv) in top.into_iter().flatten().filter(|(n, _)| n.as_str() != name) {
        let mut stack = vec![(parent.clone(), pv.clone())];
        while let Some((pname, node)) = stack.pop() {
            for (n, v) in children(&node) {
                if n == name && v.get("version").is_some() {
                    let e = DependencyEdge {
                        name: pname.clone(),
                        version: node.get("version").and_then(Value::as_str).map(String::from),
                        kind: "dependency".into(),
                        installed: true,
                    };
                    if !required_by.contains(&e) {
                        required_by.push(e);
                    }
                }
                stack.push((n, v));
            }
        }
    }

    Ok(PackageDependencies {
        manager_id: "npm".into(),
        name: name.to_string(),
        version: subject.get("version").and_then(Value::as_str).map(String::from),
        install_reason: Some("Installed explicitly (top-level global package)".into()),
        depends_on,
        transitive,
        required_by,
        orphan_hint: None,
        notes: vec![
            "npm gives every global package its own dependency tree, so the same library can appear in several trees.".into(),
            "Reverse dependencies are only searched among globally installed packages.".into(),
            "Entries marked \"optional / not installed\" are declared by the package but absent on this machine (usually other platforms).".into(),
        ],
    })
}

/// `npm search --json`: an array of `{name, version, description, ...}`.
pub(crate) fn parse_search(stdout: &str) -> Result<Vec<SearchHit>> {
    if stdout.trim().is_empty() {
        return Ok(vec![]);
    }
    let v: Value = serde_json::from_str(stdout)?;
    let arr = v.as_array().ok_or_else(|| AppError::Parse("npm search: expected array".into()))?;
    Ok(arr
        .iter()
        .filter_map(|e| {
            Some(SearchHit {
                name: e.get("name")?.as_str()?.to_string(),
                version: e.get("version").and_then(Value::as_str).map(String::from),
                description: e.get("description").and_then(Value::as_str).filter(|d| !d.is_empty()).map(String::from),
            })
        })
        .collect())
}

/// `npm view <pkg> --json`: an object (npm <=10) or an array of objects (npm 12).
/// Maintainer e-mail addresses are deliberately dropped.
pub(crate) fn parse_view(stdout: &str) -> Result<PackageDetails> {
    let v: Value = serde_json::from_str(stdout)?;
    let e = match &v {
        Value::Array(a) => a.last().ok_or_else(|| AppError::Parse("npm view: empty result".into()))?,
        o => o,
    };
    let s = |k: &str| e.get(k).and_then(Value::as_str).map(String::from);
    let mut d = PackageDetails {
        manager_id: "npm".into(),
        name: s("name").ok_or_else(|| AppError::Parse("npm view: no name".into()))?,
        version: s("version"),
        description: s("description"),
        homepage: s("homepage"),
        license: s("license"),
        ..Default::default()
    };
    if let Some(deps) = e.get("dependencies").and_then(Value::as_object) {
        d.dependencies = deps.iter().map(|(k, v)| format!("{k} {}", v.as_str().unwrap_or(""))).map(|x| x.trim().to_string()).collect();
    }
    if let Some(m) = e.get("maintainers").and_then(Value::as_array) {
        let names: Vec<String> = m
            .iter()
            .filter_map(|x| match x {
                Value::String(s) => Some(s.split('<').next().unwrap_or("").trim().to_string()),
                o => o.get("name").or_else(|| o.get("username")).and_then(Value::as_str).map(String::from),
            })
            .filter(|n| !n.is_empty())
            .collect();
        if !names.is_empty() {
            d.metadata.insert("maintainers".into(), names.join(", "));
        }
    }
    if let Some(k) = e.get("keywords").and_then(Value::as_array) {
        let ks: Vec<&str> = k.iter().filter_map(Value::as_str).collect();
        if !ks.is_empty() {
            d.metadata.insert("keywords".into(), ks.join(", "));
        }
    }
    if let Some(u) = e.get("repository").and_then(|r| r.get("url")).and_then(Value::as_str) {
        d.metadata.insert("repository".into(), u.to_string());
    }
    if let Some(t) = e.get("time").and_then(|t| t.get("modified")).and_then(Value::as_str) {
        d.metadata.insert("lastPublished".into(), t.to_string());
    }
    Ok(d)
}

pub(crate) fn parse_ls(stdout: &str, root: Option<&str>) -> Result<Vec<Package>> {
    let v: Value = serde_json::from_str(stdout)?;
    let deps = match v.get("dependencies").and_then(Value::as_object) {
        Some(d) => d,
        None => return Ok(vec![]),
    };
    Ok(deps
        .iter()
        .map(|(name, e)| {
            let mut p = Package::new("npm", format!("npm:{name}"), name.as_str());
            p.version = e.get("version").and_then(Value::as_str).map(String::from);
            p.kind = PackageKind::Library;
            p.installation_path = root.map(|r| format!("{r}/{name}"));
            p.installation_source = e.get("resolved").and_then(Value::as_str).map(String::from);
            p.metadata.insert("scope".into(), "global".into());
            p
        })
        .collect())
}

/// `{ "typescript": { "current": "5.4.0", "wanted": "5.4.5", "latest": "5.5.0" } }`
pub(crate) fn parse_outdated(stdout: &str) -> Result<Vec<UpdateInfo>> {
    if stdout.trim().is_empty() {
        return Ok(vec![]);
    }
    let v: Value = serde_json::from_str(stdout)?;
    let obj = v.as_object().ok_or_else(|| AppError::Parse("npm outdated: expected object".into()))?;
    Ok(obj
        .iter()
        .filter_map(|(k, e)| {
            Some(UpdateInfo { name: k.clone(), latest_version: e.get("latest")?.as_str()?.into() })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ls() {
        let j = r#"{"name":"lib","dependencies":{"typescript":{"version":"5.4.0","resolved":"https://registry.npmjs.org/typescript/-/typescript-5.4.0.tgz"},"npm":{"version":"10.5.0"}}}"#;
        let p = parse_ls(j, Some("/usr/lib/node_modules")).unwrap();
        assert_eq!(p.len(), 2);
        let ts = p.iter().find(|p| p.name == "typescript").unwrap();
        assert_eq!(ts.installation_path.as_deref(), Some("/usr/lib/node_modules/typescript"));
        assert_eq!(ts.metadata["scope"], "global");
    }

    /// Real output captured from npm 12.1 on Windows 11.
    #[test]
    fn parses_real_npm12_output() {
        let ls = r#"{"name":"npm","dependencies":{"@qwen-code/qwen-code":{"version":"0.24.0","overridden":false},"npm":{"version":"12.1.0","overridden":false}}}"#;
        let p = parse_ls(ls, None).unwrap();
        assert_eq!(p.len(), 2);
        assert!(p.iter().any(|p| p.name == "@qwen-code/qwen-code" && p.version.as_deref() == Some("0.24.0")));
        let out = r#"{"npm":{"current":"12.1.0","wanted":"12.2.2","latest":"12.2.2","dependent":"global","location":"C:\\Users\\x\\npm"}}"#;
        let u = parse_outdated(out).unwrap();
        assert_eq!(u[0].latest_version, "12.2.2");
    }

    /// Shapes captured from npm 12.1 (`npm search --json`, `npm view --json`).
    #[test]
    fn parses_real_search_and_view() {
        let search = "[\n\n{\"name\":\"is-odd\",\"version\":\"3.0.1\",\"description\":\"Returns true if odd\",\"publisher\":{\"email\":\"x@y.z\",\"username\":\"jon\"}}\n\n,\n\n{\"name\":\"is-odd-ai\",\"version\":\"1.0.2\",\"description\":\"\"}\n]";
        let hits = parse_search(search).unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].name, "is-odd");
        assert_eq!(hits[0].version.as_deref(), Some("3.0.1"));
        assert_eq!(hits[1].description, None);
        assert!(parse_search("").unwrap().is_empty());

        let view = r#"[{"name":"is-odd","version":"3.0.1","description":"d","license":"MIT","homepage":"https://h","dependencies":{"is-number":"^6.0.0"},"maintainers":["doowb <brian@x.com>","jon <j@x.com>"],"keywords":["odd","math"],"repository":{"url":"git+https://g"},"time":{"modified":"2026-04-14"}}]"#;
        let d = parse_view(view).unwrap();
        assert_eq!((d.name.as_str(), d.version.as_deref(), d.license.as_deref()), ("is-odd", Some("3.0.1"), Some("MIT")));
        assert_eq!(d.dependencies, vec!["is-number ^6.0.0"]);
        assert_eq!(d.metadata["maintainers"], "doowb, jon");
        assert!(!d.metadata.values().any(|v| v.contains('@') && v.contains("x.com")), "emails must not leak");
        // npm <= 10 returns a bare object
        assert!(parse_view(r#"{"name":"a","version":"1.0.0"}"#).is_ok());
        assert!(parse_view("[]").is_err());
    }

    #[test]
    fn search_and_details_validate_input() {
        use crate::exec::fake::FakeRunner;
        let r = FakeRunner::new(&["npm"]);
        assert!(Npm.search(&r, "--registry=x", 10).is_err());
        assert!(Npm.details(&r, "bad name").is_err());
        let r = FakeRunner::new(&["npm"]).on("npm search --json --searchlimit=10 is odd", 0, "[]");
        assert!(Npm.search(&r, "is odd", 10).unwrap().is_empty());
    }

    #[test]
    fn dependency_tree_direct_transitive_reverse() {
        let j = r#"{"name":"npm","dependencies":{
          "app":{"version":"1.0.0","dependencies":{
              "left":{"version":"2.0.0","dependencies":{"deep":{"version":"3.1.0"}}},
              "native-win":{"version":"1.1.0"},
              "native-mac":{}}},
          "tool":{"version":"0.5.0","dependencies":{"left":{"version":"2.0.0"}}},
          "left":{"version":"2.0.0","dependencies":{"deep":{"version":"3.1.0"}}}}}"#;
        let d = parse_dependencies(j, "app").unwrap();
        assert_eq!(d.version.as_deref(), Some("1.0.0"));
        assert_eq!(d.depends_on.len(), 3);
        let mac = d.depends_on.iter().find(|e| e.name == "native-mac").unwrap();
        assert!(!mac.installed && mac.version.is_none());
        assert_eq!(d.transitive.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), vec!["deep"]);
        assert!(d.required_by.is_empty());
        assert!(d.orphan_hint.is_none());

        // `left` is a global package of its own AND a dependency of app and tool.
        let l = parse_dependencies(j, "left").unwrap();
        let mut by: Vec<_> = l.required_by.iter().map(|e| e.name.as_str()).collect();
        by.sort();
        assert_eq!(by, vec!["app", "tool"]);
        assert_eq!(l.depends_on[0].name, "deep");
    }

    #[test]
    fn dependency_tree_rejects_unknown_and_invalid() {
        let j = r#"{"dependencies":{"app":{"version":"1.0.0"}}}"#;
        assert!(parse_dependencies(j, "nope").is_err());
        let r = crate::exec::fake::FakeRunner::new(&["npm"]);
        assert!(Npm.dependencies(&r, "bad name; rm").is_err());
        assert!(parse_dependencies("", "app").is_err());
    }

    #[test]
    fn empty_tree_is_ok() {
        assert!(parse_ls("{}", None).unwrap().is_empty());
    }

    #[test]
    fn parses_outdated() {
        let u = parse_outdated(r#"{"typescript":{"current":"5.4.0","wanted":"5.4.5","latest":"5.5.0"}}"#).unwrap();
        assert_eq!(u[0].latest_version, "5.5.0");
    }
}
