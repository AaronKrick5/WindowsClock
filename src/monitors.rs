//! Monitor list and stable monitor identification.
//! No Windows calls. The `win` module supplies the raw data.
//!
//! Identification rule:
//! - A monitor is identified by its model code (manufacturer and product code
//!   that the monitor reports, for example `gsm7727`).
//! - The connector part of the hardware path can change when a cable is
//!   disconnected and connected again. It is used only to tell two monitors
//!   of the same model apart.

use crate::geometry::Rect;

/// One monitor as Windows reports it for window positions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawMonitor {
    /// Windows display name, for example `\\.\DISPLAY1`. Not stable.
    pub gdi_name: String,
    pub rect: Rect,
    pub dpi: u32,
    pub primary: bool,
}

/// Hardware data for one active display output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisplayTarget {
    /// Windows display name that this output shows.
    pub gdi_name: String,
    /// Hardware device path of the monitor, for example
    /// `\\?\DISPLAY#GSM7727#5&2cacaf54&0&UID512#{...}`.
    pub device_path: String,
    /// Name from the monitor, for example `34GK950F`. Can be empty.
    pub friendly_name: String,
}

/// One connected monitor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Monitor {
    /// Model code, lower case. Empty if Windows gave no hardware data.
    pub model: String,
    /// Connector part of the hardware path, lower case. Can change on a reconnect.
    pub instance: String,
    /// Name to show to the user.
    pub name: String,
    pub rect: Rect,
    pub dpi: u32,
    pub primary: bool,
    /// False if Windows gave no hardware data. Such a monitor cannot be
    /// selected and never gets a clock.
    pub stable: bool,
}

impl Monitor {
    /// Key that is unique in the list of connected monitors. Only valid
    /// until the next display change. Never saved.
    pub fn runtime_key(&self) -> String {
        format!("{}|{}", self.model, self.instance)
    }

    /// Text for menus and questions, for example `34GK950F (3440 x 1440)`.
    pub fn label(&self) -> String {
        let main = if self.primary { ", main monitor" } else { "" };
        format!(
            "{} ({} x {}{})",
            self.name,
            self.rect.width(),
            self.rect.height(),
            main
        )
    }
}

/// Split a hardware device path into the model code and the connector part.
///
/// `\\?\DISPLAY#GSM7727#5&2cacaf54&0&UID512#{guid}` gives
/// (`gsm7727`, `5&2cacaf54&0&uid512`).
pub fn split_device_path(path: &str) -> Option<(String, String)> {
    let lower = path.trim().to_ascii_lowercase();
    let mut parts = lower.split('#');
    let _prefix = parts.next()?;
    let model = parts.next()?.trim();
    let instance = parts.next().unwrap_or("").trim();
    if model.is_empty() {
        return None;
    }
    Some((model.to_string(), instance.to_string()))
}

/// Number of connected monitors with the given model code.
pub fn count_model(monitors: &[Monitor], model: &str) -> usize {
    monitors
        .iter()
        .filter(|m| m.stable && m.model == model)
        .count()
}

/// Combine position data and hardware data into the monitor list.
pub fn combine(raw: &[RawMonitor], targets: &[DisplayTarget]) -> Vec<Monitor> {
    let mut list: Vec<Monitor> = raw
        .iter()
        .map(|r| {
            let found = targets
                .iter()
                .filter(|t| t.gdi_name.eq_ignore_ascii_case(&r.gdi_name))
                .find_map(|t| split_device_path(&t.device_path).map(|ids| (t, ids)));
            match found {
                Some((t, (model, instance))) => Monitor {
                    model,
                    instance,
                    name: if t.friendly_name.trim().is_empty() {
                        short_name(&r.gdi_name)
                    } else {
                        t.friendly_name.trim().to_string()
                    },
                    rect: r.rect,
                    dpi: r.dpi,
                    primary: r.primary,
                    stable: true,
                },
                None => Monitor {
                    model: String::new(),
                    instance: r.gdi_name.to_ascii_lowercase(),
                    name: short_name(&r.gdi_name),
                    rect: r.rect,
                    dpi: r.dpi,
                    primary: r.primary,
                    stable: false,
                },
            }
        })
        .collect();
    // Same order each time: left to right, then top to bottom.
    list.sort_by_key(|m| (m.rect.left, m.rect.top));
    list
}

fn short_name(gdi_name: &str) -> String {
    gdi_name.trim_start_matches(r"\\.\").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(left: i32, width: i32) -> Rect {
        Rect {
            left,
            top: 0,
            right: left + width,
            bottom: 1440,
        }
    }

    #[test]
    fn device_path_is_split() {
        let (model, instance) = split_device_path(
            r"\\?\DISPLAY#GSM7727#5&2cacaf54&0&UID512#{e6f07b5f-ee97-4a90-b076-33f57bf4eaa7}",
        )
        .unwrap();
        assert_eq!(model, "gsm7727");
        assert_eq!(instance, "5&2cacaf54&0&uid512");
        assert!(split_device_path("").is_none());
        assert!(split_device_path(r"\\?\display").is_none());
    }

    #[test]
    fn same_monitor_keeps_model_after_reconnect() {
        // Values from a real log: the connector part changed from uid512 to uid516.
        let before = split_device_path(r"\\?\display#gsm7727#5&2cacaf54&0&uid512#{x}").unwrap();
        let after = split_device_path(r"\\?\display#gsm7727#5&2cacaf54&0&uid516#{x}").unwrap();
        assert_eq!(before.0, after.0);
        assert_ne!(before.1, after.1);
    }

    #[test]
    fn monitors_get_hardware_ids() {
        let raw = vec![
            RawMonitor {
                gdi_name: r"\\.\DISPLAY1".into(),
                rect: rect(0, 3840),
                dpi: 144,
                primary: true,
            },
            RawMonitor {
                gdi_name: r"\\.\DISPLAY2".into(),
                rect: rect(3840, 2560),
                dpi: 96,
                primary: false,
            },
        ];
        let targets = vec![
            DisplayTarget {
                gdi_name: r"\\.\DISPLAY2".into(),
                device_path: r"\\?\DISPLAY#ABC1234#5&1&0&UID1#{guid}".into(),
                friendly_name: "IPS Wide".into(),
            },
            DisplayTarget {
                gdi_name: r"\\.\DISPLAY1".into(),
                device_path: r"\\?\DISPLAY#XYZ9999#5&2&0&UID2#{guid}".into(),
                friendly_name: "".into(),
            },
        ];
        let list = combine(&raw, &targets);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].model, "xyz9999");
        assert_eq!(list[0].instance, "5&2&0&uid2");
        assert_eq!(list[0].name, "DISPLAY1");
        assert!(list[0].stable);
        assert_eq!(list[1].name, "IPS Wide");
        assert_eq!(list[1].label(), "IPS Wide (2560 x 1440)");
        assert_eq!(list[0].label(), "DISPLAY1 (3840 x 1440, main monitor)");
        assert_ne!(list[0].runtime_key(), list[1].runtime_key());
    }

    #[test]
    fn monitor_without_hardware_data_is_not_stable() {
        let raw = vec![RawMonitor {
            gdi_name: r"\\.\DISPLAY7".into(),
            rect: rect(0, 1920),
            dpi: 96,
            primary: true,
        }];
        let list = combine(&raw, &[]);
        assert!(!list[0].stable);
        assert!(list[0].model.is_empty());
        assert_eq!(count_model(&list, ""), 0);
    }

    #[test]
    fn order_is_left_to_right() {
        let raw = vec![
            RawMonitor {
                gdi_name: "B".into(),
                rect: rect(0, 100),
                dpi: 96,
                primary: true,
            },
            RawMonitor {
                gdi_name: "A".into(),
                rect: rect(-100, 100),
                dpi: 96,
                primary: false,
            },
        ];
        let list = combine(&raw, &[]);
        assert_eq!(list[0].name, "A");
    }
}
