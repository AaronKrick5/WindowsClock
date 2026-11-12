//! Decision: is a full-screen application showing below the clock?
//! No Windows calls. The `win` module supplies the window list.
//!
//! Rule:
//! - Look at the first window (from the top of the window order) that
//!   contains the position of the clock.
//! - That window is a full-screen window if its rectangle is exactly the
//!   monitor rectangle and it has no title bar, and it is not the desktop.
//! - A maximized window is not a full-screen window: it has a title bar, or
//!   its rectangle is larger than the monitor.

use crate::geometry::Rect;

/// One visible top-level window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowInfo {
    pub rect: Rect,
    /// True if the window has a standard title bar.
    pub has_caption: bool,
    /// Window class name. Used to exclude the desktop and for the log.
    pub class_name: String,
}

/// Window class names of the Windows desktop background.
const DESKTOP_CLASSES: [&str; 2] = ["Progman", "WorkerW"];

fn contains(rect: Rect, point: (i32, i32)) -> bool {
    point.0 >= rect.left && point.0 < rect.right && point.1 >= rect.top && point.1 < rect.bottom
}

/// True if this window is a full-screen application window on the monitor.
pub fn is_fullscreen_window(window: &WindowInfo, monitor: Rect) -> bool {
    window.rect == monitor
        && !window.has_caption
        && !DESKTOP_CLASSES
            .iter()
            .any(|c| window.class_name.eq_ignore_ascii_case(c))
}

/// Find the full-screen window below the clock, if there is one.
///
/// `windows` must be in window order from top to bottom, and must not
/// contain the clock windows.
pub fn fullscreen_below(
    windows: &[WindowInfo],
    monitor: Rect,
    clock_point: (i32, i32),
) -> Option<&WindowInfo> {
    let first = windows.iter().find(|w| contains(w.rect, clock_point))?;
    if is_fullscreen_window(first, monitor) {
        Some(first)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MONITOR: Rect = Rect {
        left: -3440,
        top: 0,
        right: 0,
        bottom: 1440,
    };
    const POINT: (i32, i32) = (-20, 1420);

    fn window(rect: Rect, has_caption: bool, class_name: &str) -> WindowInfo {
        WindowInfo {
            rect,
            has_caption,
            class_name: class_name.into(),
        }
    }

    #[test]
    fn video_in_full_screen_hides_the_clock() {
        let list = [
            window(MONITOR, false, "Chrome_WidgetWin_1"),
            window(MONITOR, false, "Progman"),
        ];
        assert!(fullscreen_below(&list, MONITOR, POINT).is_some());
    }

    #[test]
    fn desktop_does_not_hide_the_clock() {
        let list = [
            window(MONITOR, false, "WorkerW"),
            window(MONITOR, false, "Progman"),
        ];
        assert!(fullscreen_below(&list, MONITOR, POINT).is_none());
    }

    #[test]
    fn maximized_window_does_not_hide_the_clock() {
        // A maximized window is 8 pixels larger than the monitor on each side.
        let maximized = Rect {
            left: MONITOR.left - 8,
            top: MONITOR.top - 8,
            right: MONITOR.right + 8,
            bottom: MONITOR.bottom + 8,
        };
        let list = [window(maximized, true, "Notepad")];
        assert!(fullscreen_below(&list, MONITOR, POINT).is_none());
        // Also a maximized window without a standard title bar.
        let list = [window(maximized, false, "Chrome_WidgetWin_1")];
        assert!(fullscreen_below(&list, MONITOR, POINT).is_none());
    }

    #[test]
    fn normal_window_above_a_full_screen_window_shows_the_clock() {
        let small = Rect {
            left: -500,
            top: 1000,
            right: 0,
            bottom: 1440,
        };
        let list = [
            window(small, true, "Notepad"),
            window(MONITOR, false, "Chrome_WidgetWin_1"),
        ];
        assert!(fullscreen_below(&list, MONITOR, POINT).is_none());
    }

    #[test]
    fn full_screen_window_on_a_different_monitor_has_no_effect() {
        let other = Rect {
            left: 0,
            top: 0,
            right: 3840,
            bottom: 2160,
        };
        let list = [
            window(other, false, "GameWindow"),
            window(MONITOR, false, "Progman"),
        ];
        assert!(fullscreen_below(&list, MONITOR, POINT).is_none());
    }

    #[test]
    fn window_that_is_not_below_the_clock_is_ignored() {
        let elsewhere = Rect {
            left: -3440,
            top: 0,
            right: -2000,
            bottom: 700,
        };
        let list = [
            window(elsewhere, true, "Notepad"),
            window(MONITOR, false, "GameWindow"),
        ];
        assert!(fullscreen_below(&list, MONITOR, POINT).is_some());
    }
}
