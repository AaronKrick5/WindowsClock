//! Monitor list and stable monitor identification.
//! No Windows calls. The `win` module supplies the raw data.

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
    /// Hardware device path of the monitor. Stable for the same monitor on the same connector.
    pub device_path: String,
    /// Name from the monitor, for example `DELL U2723QE`. Can be empty.
    pub friendly_name: String,
}

/// One monitor with a stable identifier.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Monitor {
    /// Stable identifier. Used as the key in the settings file.
    pub id: String,
    /// Name to show to the user.
    pub name: String,
    pub rect: Rect,
    pub dpi: u32,
    pub primary: bool,
    /// False if no hardware path was found. Such a monitor cannot be selected.
    pub stable: bool,
}

impl Monitor {
    /// Text for menus and questions, for example `DELL U2723QE (2560 x 1440)`.
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

/// Combine position data and hardware data into the monitor list.
pub fn combine(raw: &[RawMonitor], targets: &[DisplayTarget]) -> Vec<Monitor> {
    let mut list: Vec<Monitor> = raw
        .iter()
        .map(|r| {
            let target = targets.iter().find(|t| {
                t.gdi_name.eq_ignore_ascii_case(&r.gdi_name) && !t.device_path.is_empty()
            });
            match target {
                Some(t) => Monitor {
                    id: t.device_path.to_ascii_lowercase(),
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
                    id: format!("unstable:{}", r.gdi_name.to_ascii_lowercase()),
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
        assert_eq!(list[0].id, r"\\?\display#xyz9999#5&2&0&uid2#{guid}");
        assert_eq!(list[0].name, "DISPLAY1");
        assert!(list[0].stable);
        assert_eq!(list[1].name, "IPS Wide");
        assert_eq!(list[1].label(), "IPS Wide (2560 x 1440)");
        assert_eq!(list[0].label(), "DISPLAY1 (3840 x 1440, main monitor)");
    }

    #[test]
    fn monitor_without_hardware_path_is_not_stable() {
        let raw = vec![RawMonitor {
            gdi_name: r"\\.\DISPLAY7".into(),
            rect: rect(0, 1920),
            dpi: 96,
            primary: true,
        }];
        let list = combine(&raw, &[]);
        assert!(!list[0].stable);
        assert!(list[0].id.starts_with("unstable:"));
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
