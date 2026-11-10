//! Rectangles and the position calculation of the clock window.
//! No Windows calls. All functions have tests.

/// A rectangle in virtual screen coordinates. Coordinates can be negative
/// for monitors to the left of or above the main monitor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub fn width(&self) -> i32 {
        self.right - self.left
    }

    pub fn height(&self) -> i32 {
        self.bottom - self.top
    }
}

/// Convert a size in device-independent units (96 per inch) to pixels.
pub fn scale(value: i32, dpi: u32) -> i32 {
    let dpi = i64::from(dpi.max(1));
    ((i64::from(value) * dpi + 48) / 96) as i32
}

/// Convert a font size in points (72 per inch) to pixels.
pub fn points_to_pixels(points: i32, dpi: u32) -> i32 {
    let dpi = i64::from(dpi.max(1));
    ((i64::from(points) * dpi + 36) / 72) as i32
}

/// Top-left position of a window of the given size in the bottom-right
/// corner of the monitor, with a margin. The window always stays inside
/// the monitor rectangle.
pub fn bottom_right(monitor: Rect, width: i32, height: i32, margin: i32) -> (i32, i32) {
    let x = (monitor.right - margin - width).max(monitor.left);
    let y = (monitor.bottom - margin - height).max(monitor.top);
    (x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_values() {
        assert_eq!(scale(12, 96), 12);
        assert_eq!(scale(12, 144), 18);
        assert_eq!(scale(12, 192), 24);
        assert_eq!(points_to_pixels(12, 96), 16);
        assert_eq!(points_to_pixels(9, 144), 18);
    }

    #[test]
    fn bottom_right_on_main_monitor() {
        let m = Rect {
            left: 0,
            top: 0,
            right: 2560,
            bottom: 1440,
        };
        assert_eq!(bottom_right(m, 100, 30, 10), (2450, 1400));
    }

    #[test]
    fn bottom_right_on_monitor_with_negative_coordinates() {
        let m = Rect {
            left: -2560,
            top: -200,
            right: 0,
            bottom: 1240,
        };
        assert_eq!(bottom_right(m, 100, 30, 10), (-110, 1200));
    }

    #[test]
    fn window_larger_than_monitor_stays_at_monitor_origin() {
        let m = Rect {
            left: 100,
            top: 100,
            right: 150,
            bottom: 120,
        };
        assert_eq!(bottom_right(m, 500, 500, 10), (100, 100));
    }
}
