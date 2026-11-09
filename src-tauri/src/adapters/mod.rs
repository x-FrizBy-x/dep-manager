//! Package manager adapters. Milestone 1 is strictly read-only: adapters can
//! detect, list and report updates, but cannot modify the system. Install /
//! update / uninstall are intentionally absent from the trait until they are
//! implemented and tested.

use crate::error::{AppError, Result};
use crate::exec::Runner;
use crate::model::{Package, UpdateInfo};
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
