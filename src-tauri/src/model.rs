//! Shared, manager-agnostic data model. Unknown values are `None`, never invented.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// How UniManage relates to a package.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagementStatus {
    /// Installed and tracked by a package manager adapter we implement.
    Managed,
    /// Installed by something else (reserved: detected but not controllable).
    External,
    /// Executable detected with no known installation source (reserved).
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateStatus {
    /// Update check was not run or the manager cannot report updates.
    Unknown,
    UpToDate,
    Available,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageKind {
    Application,
    Runtime,
    Library,
    System,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Package {
    /// Unique per installation source, e.g. `apt:git:amd64`.
    pub id: String,
    pub name: String,
    pub display_name: Option<String>,
    pub version: Option<String>,
    pub latest_version: Option<String>,
    pub description: Option<String>,
    pub package_manager: String,
    pub category: Option<String>,
    pub kind: PackageKind,
    pub installation_source: Option<String>,
    pub installation_path: Option<String>,
    pub architecture: Option<String>,
    pub installed_date: Option<String>,
    /// `None` = unknown (not checked / unsupported), never guessed.
    pub update_available: Option<bool>,
    pub update_status: UpdateStatus,
    pub management_status: ManagementStatus,
    pub homepage: Option<String>,
    pub license: Option<String>,
    pub dependencies: Vec<String>,
    pub metadata: BTreeMap<String, String>,
    /// Other managers that report a package with the same normalized name.
    /// Rows are never merged: this only surfaces the overlap.
    pub also_found_in: Vec<String>,
}

impl Package {
    pub fn new(manager: &str, id: String, name: impl Into<String>) -> Self {
        Package {
            id,
            name: name.into(),
            display_name: None,
            version: None,
            latest_version: None,
            description: None,
            package_manager: manager.to_string(),
            category: None,
            kind: PackageKind::Unknown,
            installation_source: None,
            installation_path: None,
            architecture: None,
            installed_date: None,
            update_available: None,
            update_status: UpdateStatus::Unknown,
            management_status: ManagementStatus::Managed,
            homepage: None,
            license: None,
            dependencies: Vec::new(),
            metadata: BTreeMap::new(),
            also_found_in: Vec::new(),
        }
    }
}

/// An update reported natively by a package manager.
#[derive(Debug, Clone, PartialEq)]
pub struct UpdateInfo {
    pub name: String,
    pub latest_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagerInfo {
    pub id: String,
    pub display_name: String,
    pub detected: bool,
    pub path: Option<String>,
    pub version: Option<String>,
    /// Whether this adapter can report updates natively.
    pub supports_updates: bool,
    pub package_count: u32,
    pub last_error: Option<String>,
}
