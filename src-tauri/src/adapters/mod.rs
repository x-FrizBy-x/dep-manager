//! Package manager adapters. Milestone 1 is strictly read-only: adapters can
//! detect, list and report updates, but cannot modify the system. Install /
//! update / uninstall are intentionally absent from the trait until they are
//! implemented and tested.

use crate::error::{AppError, Result};
use crate::exec::Runner;
use crate::model::{Package, PackageDependencies, PackageDetails, SearchHit, UpdateInfo};
use std::path::PathBuf;

pub mod apt;
pub mod flatpak;
pub mod mise;
pub mod npm;
pub mod pipx;
pub mod pnpm;
pub mod snap;
pub mod uv;

pub trait Adapter: Send + Sync {
    /// Stable identifier, e.g. `apt`.
    fn id(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    /// Executable looked up in PATH.
    fn binary(&self) -> &'static str;
    /// Whether `check_updates` is implemented natively for this manager.
    fn supports_updates(&self) -> bool {
        false
    }

    fn detect(&self, r: &dyn Runner) -> Option<PathBuf> {
        r.which(self.binary())
    }
    fn version(&self, r: &dyn Runner) -> Result<String>;
    fn list_installed(&self, r: &dyn Runner) -> Result<Vec<Package>>;
    fn check_updates(&self, _r: &dyn Runner) -> Result<Vec<UpdateInfo>> {
        Err(AppError::Unsupported(format!("{} does not report updates natively", self.display_name())))
    }

    /// Whether `search` and `details` are implemented for this manager.
    fn supports_search(&self) -> bool {
        false
    }
    /// Searches the manager's registry/repositories. `query` has already been
    /// validated with [`validate_query`].
    fn search(&self, _r: &dyn Runner, _query: &str, _limit: usize) -> Result<Vec<SearchHit>> {
        Err(AppError::Unsupported(format!("{} search is not implemented", self.display_name())))
    }
    /// Whether `dependencies` is implemented for this manager.
    fn supports_dependencies(&self) -> bool {
        false
    }
    /// Direct, transitive and reverse dependencies of one installed package.
    /// `name` is validated by the implementation.
    fn dependencies(&self, _r: &dyn Runner, _name: &str) -> Result<PackageDependencies> {
        Err(AppError::Unsupported(format!("{} dependency data is not implemented", self.display_name())))
    }
    /// Details for one package. `name` is validated by the implementation.
    fn details(&self, _r: &dyn Runner, _name: &str) -> Result<PackageDetails> {
        Err(AppError::Unsupported(format!("{} package details are not implemented", self.display_name())))
    }
}

/// Validates a user search query before it reaches a command line.
/// Returns the whitespace-separated terms. Only a conservative character set
/// is allowed and no term may start with `-`, so a query can never be parsed
/// as an option, and nothing is interpreted by a shell or batch file.
pub fn validate_query(query: &str) -> Result<Vec<String>> {
    let q = query.trim();
    if q.is_empty() {
        return Err(AppError::Other("enter something to search for".into()));
    }
    if q.len() > 64 {
        return Err(AppError::Other("search text is too long (max 64 characters)".into()));
    }
    let ok = |c: char| c.is_ascii_alphanumeric() || matches!(c, ' ' | '.' | '_' | '-' | '@' | '/' | ':' | '+');
    if let Some(bad) = q.chars().find(|c| !ok(*c)) {
        return Err(AppError::Other(format!("character `{bad}` is not allowed in a search")));
    }
    let terms: Vec<String> = q.split_whitespace().map(String::from).collect();
    if terms.len() > 5 {
        return Err(AppError::Other("at most 5 search terms".into()));
    }
    if terms.iter().any(|t| t.starts_with('-')) {
        return Err(AppError::Other("search terms must not start with `-`".into()));
    }
    Ok(terms)
}

pub fn all() -> Vec<Box<dyn Adapter>> {
    vec![
        Box::new(apt::Apt),
        Box::new(flatpak::Flatpak),
        Box::new(snap::Snap),
        Box::new(mise::Mise),
        Box::new(npm::Npm),
        Box::new(pnpm::Pnpm),
        Box::new(pipx::Pipx),
        Box::new(uv::Uv),
    ]
}

/// Returns the first non-empty line of `s`, trimmed.
pub(crate) fn first_line(s: &str) -> String {
    s.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("").to_string()
}

/// `None` for empty strings and the `-` placeholder many CLIs print.
pub(crate) fn non_empty(s: &str) -> Option<String> {
    let t = s.trim();
    (!t.is_empty() && t != "-").then(|| t.to_string())
}

#[cfg(test)]
mod query_tests {
    use super::validate_query;

    #[test]
    fn accepts_normal_queries() {
        assert_eq!(validate_query("  is odd ").unwrap(), vec!["is", "odd"]);
        assert!(validate_query("@scope/pkg").is_ok());
        assert!(validate_query("g++").is_ok());
    }

    #[test]
    fn rejects_dangerous_or_option_like_queries() {
        for bad in ["", "   ", "--registry=x", "a -b", "a;b", "a&b", "a|b", "$(x)", "a%PATH%", "a\nb", "a`b", "a\"b", "a b c d e f"] {
            assert!(validate_query(bad).is_err(), "{bad:?}");
        }
        assert!(validate_query(&"a".repeat(65)).is_err());
    }
}
