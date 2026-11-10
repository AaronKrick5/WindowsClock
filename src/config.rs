//! The settings file. One file for each Windows user.
//!
//! Rules:
//! - A change is saved immediately.
//! - A monitor that is not in the file has no clock (default: off).
//! - The file is written to a temporary file first and then replaces the old
//!   file, so a power loss cannot leave a half-written file.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Version of the file format. Increase it when the format changes.
pub const CURRENT_VERSION: u32 = 1;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonitorSetting {
    /// True if the clock shows on this monitor.
    pub enabled: bool,
    /// Last known name of the monitor. Only for the user who reads the file.
    #[serde(default)]
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    pub version: u32,
    /// Key: stable monitor identifier.
    #[serde(default)]
    pub monitors: BTreeMap<String, MonitorSetting>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            version: CURRENT_VERSION,
            monitors: BTreeMap::new(),
        }
    }
}

/// Result of reading the settings file.
pub enum Loaded {
    /// The file was read.
    Ok(Config),
    /// No file exists. This is the first start.
    Missing,
    /// The file exists but cannot be used. It was renamed and default settings are used.
    Damaged(String),
}

impl Config {
    /// True if the clock is selected for this monitor.
    pub fn is_enabled(&self, monitor_id: &str) -> bool {
        self.monitors.get(monitor_id).is_some_and(|m| m.enabled)
    }

    /// Set the selection for one monitor. Returns true if the settings changed.
    pub fn set_enabled(&mut self, monitor_id: &str, name: &str, enabled: bool) -> bool {
        let entry = self.monitors.entry(monitor_id.to_string()).or_default();
        let changed = entry.enabled != enabled || entry.name != name;
        entry.enabled = enabled;
        entry.name = name.to_string();
        changed
    }

    /// Read the settings file.
    pub fn load(path: &Path) -> Loaded {
        let text = match fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Loaded::Missing,
            Err(e) => return Loaded::Damaged(format!("cannot read file: {e}")),
        };
        match serde_json::from_str::<Config>(&text) {
            Ok(c) if c.version <= CURRENT_VERSION => Loaded::Ok(c),
            Ok(c) => {
                let _ = fs::rename(path, path.with_extension("json.newer"));
                Loaded::Damaged(format!(
                    "file version {} is newer than this program",
                    c.version
                ))
            }
            Err(e) => {
                let _ = fs::rename(path, path.with_extension("json.damaged"));
                Loaded::Damaged(format!("cannot understand file: {e}"))
            }
        }
    }

    /// Write the settings file safely.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let text = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        let temp = path.with_extension("json.tmp");
        fs::write(&temp, text)?;
        fs::rename(&temp, path)
    }
}

/// Folder for the settings file and the log file of the current user.
pub fn data_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("WindowsClock")
}

pub fn settings_path() -> PathBuf {
    data_dir().join("settings.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("windowsclock-test-{}-{}", std::process::id(), name));
        let _ = fs::remove_dir_all(&dir);
        dir.join("settings.json")
    }

    #[test]
    fn unknown_monitor_is_off() {
        let c = Config::default();
        assert!(!c.is_enabled("some-monitor"));
    }

    #[test]
    fn save_and_load_keeps_selection() {
        let path = temp_file("roundtrip");
        let mut c = Config::default();
        assert!(c.set_enabled("mon-a", "IPS", true));
        assert!(c.set_enabled("mon-b", "OLED", false));
        assert!(!c.set_enabled("mon-a", "IPS", true));
        c.save(&path).unwrap();
        // Save a second time: the old file must be replaced.
        c.save(&path).unwrap();
        match Config::load(&path) {
            Loaded::Ok(loaded) => {
                assert_eq!(loaded, c);
                assert!(loaded.is_enabled("mon-a"));
                assert!(!loaded.is_enabled("mon-b"));
            }
            _ => panic!("file not loaded"),
        }
    }

    #[test]
    fn missing_file_is_first_start() {
        let path = temp_file("missing");
        assert!(matches!(Config::load(&path), Loaded::Missing));
    }

    #[test]
    fn damaged_file_is_renamed_and_not_used() {
        let path = temp_file("damaged");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "{ this is not valid").unwrap();
        assert!(matches!(Config::load(&path), Loaded::Damaged(_)));
        assert!(!path.exists());
        assert!(path.with_extension("json.damaged").exists());
    }

    #[test]
    fn newer_file_version_is_not_used() {
        let path = temp_file("newer");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, r#"{"version": 999, "monitors": {}}"#).unwrap();
        assert!(matches!(Config::load(&path), Loaded::Damaged(_)));
    }
}
