//! Application settings, stored as key/value rows in `application_settings`.
//! Values are validated on every read and write; unknown or invalid stored
//! values fall back to the defaults rather than failing.

use crate::error::{AppError, Result};
use serde::{Deserialize, Serialize};

/// Allowed background rescan intervals, in minutes.
pub const INTERVALS: [u32; 6] = [15, 30, 60, 180, 720, 1440];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// Periodically rescan in the background. Scans are read-only.
    pub rescan_enabled: bool,
    pub rescan_interval_minutes: u32,
    /// Also ask each manager for available updates during a background scan.
    pub rescan_check_updates: bool,
}

impl Default for Settings {
    fn default() -> Self {
        // Off by default: nothing runs in the background unless the user opts in.
        Settings { rescan_enabled: false, rescan_interval_minutes: 60, rescan_check_updates: true }
    }
}

pub const KEY_ENABLED: &str = "rescan_enabled";
pub const KEY_INTERVAL: &str = "rescan_interval_minutes";
pub const KEY_UPDATES: &str = "rescan_check_updates";

impl Settings {
    pub fn validate(&self) -> Result<()> {
        if INTERVALS.contains(&self.rescan_interval_minutes) {
            Ok(())
        } else {
            Err(AppError::Other(format!("rescan interval must be one of {INTERVALS:?} minutes")))
        }
    }

    /// Builds settings from stored `(key, value)` rows, ignoring anything invalid.
    pub fn from_rows(rows: &[(String, String)]) -> Settings {
        let mut s = Settings::default();
        for (k, v) in rows {
            match k.as_str() {
                KEY_ENABLED => s.rescan_enabled = v == "true",
                KEY_UPDATES => s.rescan_check_updates = v != "false",
                KEY_INTERVAL => {
                    if let Some(n) = v.parse::<u32>().ok().filter(|n| INTERVALS.contains(n)) {
                        s.rescan_interval_minutes = n;
                    }
                }
                _ => {}
            }
        }
        s
    }

    pub fn to_rows(&self) -> [(&'static str, String); 3] {
        [
            (KEY_ENABLED, self.rescan_enabled.to_string()),
            (KEY_INTERVAL, self.rescan_interval_minutes.to_string()),
            (KEY_UPDATES, self.rescan_check_updates.to_string()),
        ]
    }
}

/// Is a background scan due? `last` is the start of the most recent scan (any kind).
pub fn rescan_due(s: &Settings, last: Option<i64>, now: i64) -> bool {
    s.rescan_enabled && last.is_none_or(|t| now - t >= i64::from(s.rescan_interval_minutes) * 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_off() {
        let s = Settings::default();
        assert!(!s.rescan_enabled);
        assert!(s.validate().is_ok());
        assert_eq!(Settings::from_rows(&[]), s);
    }

    #[test]
    fn roundtrip_and_invalid_values_fall_back() {
        let s = Settings { rescan_enabled: true, rescan_interval_minutes: 30, rescan_check_updates: false };
        let rows: Vec<(String, String)> = s.to_rows().into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        assert_eq!(Settings::from_rows(&rows), s);
        let bad = vec![(KEY_INTERVAL.to_string(), "7".to_string()), ("junk".into(), "x".into())];
        assert_eq!(Settings::from_rows(&bad).rescan_interval_minutes, 60);
        assert!(Settings { rescan_interval_minutes: 7, ..s }.validate().is_err());
    }

    #[test]
    fn due_logic() {
        let on = Settings { rescan_enabled: true, rescan_interval_minutes: 15, rescan_check_updates: true };
        assert!(rescan_due(&on, None, 100));
        assert!(!rescan_due(&on, Some(100), 100 + 899));
        assert!(rescan_due(&on, Some(100), 100 + 900));
        assert!(!rescan_due(&Settings::default(), None, 1_000_000));
    }
}
