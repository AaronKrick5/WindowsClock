//! The settings file. One file for each Windows user.
//!
//! Rules:
//! - A change is saved immediately.
//! - A monitor that is not in the file has no clock (default: off).
//! - The file is written to a temporary file first and then replaces the old
//!   file, so a power loss cannot leave a half-written file.
//!
//! Monitor matching rule:
//! - If exactly one connected monitor has a model code and exactly one saved
//!   entry has that model code, they match. The connector does not matter.
//! - In all other cases (two monitors of the same model), the connector part
//!   must also be the same. If it is not, the monitor counts as new: no clock.

use crate::monitors;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Version of the file format. Increase it when the format changes.
/// Version 1 (build M1, first test): key was the full hardware path.
/// Version 2: model code and connector part.
pub const CURRENT_VERSION: u32 = 2;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonitorSetting {
    /// Model code of the monitor, lower case.
    pub model: String,
    /// Connector part of the hardware path when the entry was last saved.
    #[serde(default)]
    pub instance: String,
    /// True if the clock shows on this monitor.
    pub enabled: bool,
    /// Last known name of the monitor. Only for the user who reads the file.
    #[serde(default)]
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    pub version: u32,
    #[serde(default)]
    pub monitors: Vec<MonitorSetting>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            version: CURRENT_VERSION,
            monitors: Vec::new(),
        }
    }
}

/// File format of version 1.
#[derive(Deserialize)]
struct ConfigV1 {
    #[serde(default)]
    monitors: BTreeMap<String, MonitorSettingV1>,
}

#[derive(Deserialize)]
struct MonitorSettingV1 {
    enabled: bool,
    #[serde(default)]
    name: String,
}

impl From<ConfigV1> for Config {
    fn from(old: ConfigV1) -> Self {
        let mut config = Config::default();
        for (path, setting) in old.monitors {
            if let Some((model, instance)) = monitors::split_device_path(&path) {
                // Two old entries for the same monitor on different connectors:
                // keep one entry. "Selected" wins only if all agree, so that a
                // clock never shows where the user said no.
                if let Some(existing) = config
                    .monitors
                    .iter_mut()
                    .find(|m| m.model == model && m.instance == instance)
                {
                    existing.enabled = existing.enabled && setting.enabled;
                } else {
                    config.monitors.push(MonitorSetting {
                        model,
                        instance,
                        enabled: setting.enabled,
                        name: setting.name,
                    });
                }
            }
        }
        config
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
    /// Find the saved entry for a connected monitor.
    ///
    /// `connected_same_model` is the number of connected monitors that have
    /// this model code (the monitor itself included).
    fn find(&self, model: &str, instance: &str, connected_same_model: usize) -> Option<usize> {
        if model.is_empty() {
            return None;
        }
        let same_model: Vec<usize> = self
            .monitors
            .iter()
            .enumerate()
            .filter(|(_, m)| m.model == model)
            .map(|(i, _)| i)
            .collect();
        let exact = same_model
            .iter()
            .copied()
            .find(|&i| self.monitors[i].instance == instance);
        if exact.is_some() || connected_same_model > 1 {
            return exact;
        }
        // One connected monitor of this model, and its connector part is not
        // in the file.
        match same_model.as_slice() {
            [] => None,
            [only] => Some(*only),
            // More than one saved entry (the same monitor seen on changed
            // connectors, or two monitors of one model). Use them only if all
            // entries agree, so that a clock never shows where the user said no.
            [first, rest @ ..] => {
                let enabled = self.monitors[*first].enabled;
                if rest.iter().all(|&i| self.monitors[i].enabled == enabled) {
                    Some(*first)
                } else {
                    None
                }
            }
        }
    }

    /// True if the clock is selected for this monitor.
    pub fn is_enabled(&self, model: &str, instance: &str, connected_same_model: usize) -> bool {
        self.find(model, instance, connected_same_model)
            .is_some_and(|i| self.monitors[i].enabled)
    }

    /// Set the selection for one monitor. Returns true if the settings changed.
    pub fn set_enabled(
        &mut self,
        model: &str,
        instance: &str,
        name: &str,
        connected_same_model: usize,
        enabled: bool,
    ) -> bool {
        if model.is_empty() {
            return false;
        }
        let new = MonitorSetting {
            model: model.to_string(),
            instance: instance.to_string(),
            enabled,
            name: name.to_string(),
        };
        match self.find(model, instance, connected_same_model) {
            Some(i) => {
                let changed = self.monitors[i] != new;
                self.monitors[i] = new;
                changed
            }
            None => {
                self.monitors.push(new);
                true
            }
        }
    }

    /// Read settings from text. Converts older file versions.
    fn parse(text: &str) -> Result<Config, String> {
        let value: serde_json::Value =
            serde_json::from_str(text).map_err(|e| format!("cannot understand file: {e}"))?;
        let version = value
            .get("version")
            .and_then(|v| v.as_u64())
            .ok_or("file has no version number")?;
        match version {
            1 => serde_json::from_value::<ConfigV1>(value)
                .map(Config::from)
                .map_err(|e| format!("cannot understand version 1 file: {e}")),
            2 => serde_json::from_value::<Config>(value)
                .map_err(|e| format!("cannot understand file: {e}")),
            newer => Err(format!("file version {newer} is newer than this program")),
        }
    }

    /// Read the settings file.
    pub fn load(path: &Path) -> Loaded {
        let text = match fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Loaded::Missing,
            Err(e) => return Loaded::Damaged(format!("cannot read file: {e}")),
        };
        match Config::parse(&text) {
            Ok(config) => Loaded::Ok(config),
            Err(reason) => {
                // Keep the file for analysis, but do not use it.
                let _ = fs::rename(path, path.with_extension("json.damaged"));
                Loaded::Damaged(reason)
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
        assert!(!c.is_enabled("gsm7727", "a", 1));
    }

    #[test]
    fn monitor_without_hardware_data_is_never_selected() {
        let mut c = Config::default();
        assert!(!c.set_enabled("", "x", "DISPLAY5", 1, true));
        assert!(!c.is_enabled("", "x", 1));
        assert!(c.monitors.is_empty());
    }

    #[test]
    fn reconnect_on_changed_connector_keeps_selection() {
        // Test case TC-4: the connector part changed from uid512 to uid516.
        let mut c = Config::default();
        c.set_enabled("gsm7727", "5&2cacaf54&0&uid512", "34GK950F", 1, true);
        c.set_enabled("dela246", "5&37e83992&0&uid4357", "AW3225QF", 1, false);
        assert!(c.is_enabled("gsm7727", "5&2cacaf54&0&uid516", 1));
        assert!(!c.is_enabled("dela246", "5&37e83992&0&uid9999", 1));
        // A later change updates the same entry. No second entry is made.
        assert!(c.set_enabled("gsm7727", "5&2cacaf54&0&uid516", "34GK950F", 1, false));
        assert_eq!(c.monitors.len(), 2);
        assert!(!c.is_enabled("gsm7727", "5&2cacaf54&0&uid512", 1));
    }

    #[test]
    fn two_monitors_of_the_same_model_need_the_same_connector() {
        let mut c = Config::default();
        c.set_enabled("abc1234", "left", "Twin", 2, true);
        c.set_enabled("abc1234", "right", "Twin", 2, false);
        assert_eq!(c.monitors.len(), 2);
        assert!(c.is_enabled("abc1234", "left", 2));
        assert!(!c.is_enabled("abc1234", "right", 2));
        // Connector changed: counts as a new monitor, so no clock.
        assert!(!c.is_enabled("abc1234", "other", 2));
        // Only one of the two is connected: still matched by connector.
        assert!(c.is_enabled("abc1234", "left", 1));
        assert!(!c.is_enabled("abc1234", "other", 1));
    }

    #[test]
    fn second_monitor_of_a_saved_model_is_new() {
        let mut c = Config::default();
        c.set_enabled("abc1234", "left", "Twin", 1, true);
        // A second monitor of the same model is connected.
        assert!(c.is_enabled("abc1234", "left", 2));
        assert!(!c.is_enabled("abc1234", "right", 2));
    }

    #[test]
    fn save_and_load_keeps_selection() {
        let path = temp_file("roundtrip");
        let mut c = Config::default();
        assert!(c.set_enabled("mon-a", "1", "IPS", 1, true));
        assert!(c.set_enabled("mon-b", "2", "OLED", 1, false));
        assert!(!c.set_enabled("mon-a", "1", "IPS", 1, true));
        c.save(&path).unwrap();
        // Save a second time: the old file must be replaced.
        c.save(&path).unwrap();
        match Config::load(&path) {
            Loaded::Ok(loaded) => {
                assert_eq!(loaded, c);
                assert!(loaded.is_enabled("mon-a", "1", 1));
                assert!(!loaded.is_enabled("mon-b", "2", 1));
            }
            _ => panic!("file not loaded"),
        }
    }

    #[test]
    fn version_1_file_is_converted() {
        // Content of a real settings file from the first M1 test.
        let text = r#"{
          "version": 1,
          "monitors": {
            "\\\\?\\display#dela246#5&37e83992&0&uid4357#{e6f07b5f-ee97-4a90-b076-33f57bf4eaa7}": { "enabled": false, "name": "AW3225QF" },
            "\\\\?\\display#gsm7727#5&2cacaf54&0&uid512#{e6f07b5f-ee97-4a90-b076-33f57bf4eaa7}": { "enabled": true, "name": "34GK950F" },
            "\\\\?\\display#gsm7727#5&2cacaf54&0&uid516#{e6f07b5f-ee97-4a90-b076-33f57bf4eaa7}": { "enabled": true, "name": "34GK950F" }
          }
        }"#;
        let c = Config::parse(text).unwrap();
        assert_eq!(c.version, CURRENT_VERSION);
        assert_eq!(c.monitors.len(), 3);
        assert!(!c.is_enabled("dela246", "5&37e83992&0&uid4357", 1));
        // Two saved entries for the model: matched by connector.
        assert!(c.is_enabled("gsm7727", "5&2cacaf54&0&uid516", 1));
        assert!(c.is_enabled("gsm7727", "5&2cacaf54&0&uid512", 1));
        // A third connector value: both saved entries agree, so the clock shows.
        assert!(c.is_enabled("gsm7727", "5&2cacaf54&0&uid520", 1));
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
        fs::write(&path, r#"{"version": 999, "monitors": []}"#).unwrap();
        assert!(matches!(Config::load(&path), Loaded::Damaged(_)));
    }
}
